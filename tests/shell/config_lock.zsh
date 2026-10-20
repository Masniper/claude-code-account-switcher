#!/usr/bin/env zsh
#
# The script's edits of `config` and `links` take the lock `claude-acc` and
# the desktop app hold around theirs (`~/.claude-switch/.config.lock`).
#
# Without it, a `link` here and a binding saved in the app at the same moment
# each wrote back the file they had read, and one of the two silently
# vanished. These pin that every writer waits for a held lock, runs at once
# when it is free, and still works when the lock cannot be taken at all.
#
# That zsystem's fcntl lock and Rust's File::lock exclude each other is a
# property of macOS, checked by hand in both directions; it does not hold on
# Linux, where the script is not supported.
#
# Run: zsh tests/shell/config_lock.zsh

emulate -L zsh
set -u
zmodload zsh/datetime

typeset -i failures=0
scratch=$(mktemp -d)
trap 'kill ${holder:-} 2>/dev/null; rm -rf "$scratch"' EXIT

export HOME="$scratch/home"
mkdir -p "$HOME/.claude"
export CLAUDE_ACC_LANG=en
source "${0:A:h:h:h}/claude-switch.sh" >/dev/null 2>&1

# The script is written for BSD sed (`sed -i ''`). CI runs this on Linux,
# where GNU sed would read the '' as its script.
if sed --version >/dev/null 2>&1; then
    sed() {
        if [[ "$1" == -i && "$2" == "" ]]; then shift 2; command sed -i "$@"
        else command sed "$@"; fi
    }
fi

mkdir -p "$CLAUDE_SWITCH_ACCOUNTS_DIR/work" "$scratch/project"
lock="$CLAUDE_SWITCH_DIR/.config.lock"

check() {
    local label="$1" expected="$2" actual="$3"
    if [[ "$expected" == "$actual" ]]; then
        print -r -- "  ok   $label"
    else
        print -r -- "  FAIL $label: expected '$expected', got '$actual'"
        (( failures++ ))
    fi
}

# Hold the lock from another process for $1 seconds.
hold() {
    : >> "$lock"
    zsh -c "zmodload zsh/system; zsystem flock '$lock' && print held > '$scratch/held' && sleep $1" &
    holder=$!
    while [[ ! -e "$scratch/held" ]]; do sleep 0.05; done
    rm -f "$scratch/held"
}

# "waited" if `$@` took at least a second, "immediate" if under half of one.
timed() {
    local start=$EPOCHREALTIME
    "$@" >/dev/null 2>&1
    local took=$(( EPOCHREALTIME - start ))
    if (( took >= 1.0 )); then print waited
    elif (( took < 0.5 )); then print immediate
    else print "unclear (${took}s)"; fi
}

links() { print -r -- "$(<"$CLAUDE_SWITCH_LINKS")"; }

print -r -- "a held lock is waited for:"
cd "$scratch/project"
hold 1.5
check "link" waited "$(timed _claude_acc_link work)"
wait $holder
check "link wrote its line once" "$scratch/project=work" "$(links)"

hold 1.5
check "default" waited "$(timed _claude_acc_default work)"
wait $holder
check "default was written" work "$(_claude_default_account)"

hold 1.5
check "reset" waited "$(timed _claude_acc_reset)"
wait $holder
check "reset was written" "" "$(_claude_default_account)"

hold 1.5
check "unlink" waited "$(timed _claude_acc_unlink)"
wait $holder
check "unlink removed the line" "" "$(links)"

print -r -- ""
print -r -- "a free lock costs nothing:"
check "link" immediate "$(timed _claude_acc_link work)"
check "unlink" immediate "$(timed _claude_acc_unlink)"

print -r -- ""
print -r -- "a lock that cannot be taken falls back to the old unlocked edit:"
rm -f "$lock"
mkdir "$lock"
check "link" immediate "$(timed _claude_acc_link work)"
check "link still wrote" "$scratch/project=work" "$(links)"
rmdir "$lock"

print -r -- ""
if (( failures )); then
    print -r -- "$failures check(s) failed"
    exit 1
fi
print -r -- "all checks passed"
