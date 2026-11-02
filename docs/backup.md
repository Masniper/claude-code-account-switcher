# Backup and restore

[← README](../README.md) · [Русский](ru/backup.md)

`claude-acc backup` writes your accounts and projects' agent files to one encrypted file and restores them on another machine, including from macOS to Windows. Credentials never go in: every account signs in again after a restore.

- [What goes in](#what-goes-in)
- [Three kinds of backup](#three-kinds-of-backup)
- [From a Mac to Windows](#from-a-mac-to-windows)
- [Reusing skills and agents in another project](#reusing-skills-and-agents-in-another-project)
- [Conflicts, running it again, and interruptions](#conflicts-running-it-again-and-interruptions)
- [Security](#security)
- [Limits](#limits)

## What goes in

| | |
| --- | --- |
| **Travels** | `settings.json`, `CLAUDE.md`, skills, agents, commands, rules, output styles, `history.jsonl`, `projects/` (transcripts, subagent transcripts, memory), `todos/`, `plans/`, `file-history/` (checkpoints), `agent-memory*`, each account's `.claude.json` (project list, MCP servers, preferences), directory bindings and the default account. For a project: `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`, `.mcp.json`, and everything under `.claude/` and `.agents/`, including files git ignores |
| **Removed as secrets** | `.credentials.json`, `.session_ingress_token`, `hfi-auth.json`; `primaryApiKey` and `oauthAccount` from `.claude.json`; every MCP server's `headers`; `env` values whose name contains `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `AUTH`, `CREDENTIAL` or `COOKIE`, in settings and MCP configs. The backup lists each removed field by name, never by value |
| **Install again** | `plugins/` and `cowork_plugins/`: git clones and downloads with this machine's paths in them. Also any `node_modules` |
| **Left behind** | Caches, logs, telemetry, shell snapshots, live session registries (`sessions/`), locks, PID files and sockets, `.claude/worktrees` checkouts, and this tool's own identity cache |
| **Not included** | A project's source code. Copy or clone it yourself |

The left-behind list follows Claude Code 2.1.309's own lists of machine state. `backup create --dry-run` prints every entry it leaves out.

## Three kinds of backup

| You want | Command |
| --- | --- |
| Every account, whole | `claude-acc backup create env.ccbackup` |
| Some accounts | `claude-acc backup create work.ccbackup --account work --account default` |
| One project with its sessions and memory, from every account | `claude-acc backup create app.ccbackup --project ~/work/app` |
| All of it | `claude-acc backup create all.ccbackup --all-accounts --project ~/work/app` |

Add `--dry-run` to any of them to see the accounts, projects, file count, size and every left-out entry, without writing anything.

Close Claude Code first. A file that changes while it is being read fails the backup rather than producing an inconsistent one.

Restoring has the same three modes:

- with no `--account` or `--project`, it restores everything;
- `--account` alone restores those accounts;
- `--project` alone restores those projects, and their sessions and memory into the accounts of the same name.

`claude-acc backup inspect FILE` checks every checksum and shows what a backup holds.

## From a Mac to Windows

Say the projects live in `/Users/me/work` on the Mac and will live in `F:\work` on Windows.

On the Mac:

```sh
claude-acc backup create ~/env.ccbackup --all-accounts --project ~/work/app --dry-run
claude-acc backup create ~/env.ccbackup --all-accounts --project ~/work/app
```

On Windows, in PowerShell, after [installing](install.md) Claude Code and claude-acc and copying the sources to `F:\work`:

```powershell
claude-acc backup inspect F:\env.ccbackup
claude-acc backup restore F:\env.ccbackup --map /Users/me/work=F:\work --dry-run
claude-acc backup restore F:\env.ccbackup --map /Users/me/work=F:\work
```

`--map FROM=TO` rewrites a path prefix, whole folder names only: `/Users/me/work/app/sub` becomes `F:\work\app\sub`, and `/Users/me/workshop` is left alone. It is used for:

- **Session folders.** Claude Code finds a project's transcripts and memory in `projects/<folder name made from its path>`. Those folders are renamed for the new path, `-Users-me-work-app` to `F--work-app`, subagent transcripts and memory included.
- **`.claude.json`'s project list**, which is keyed by path.
- **Directory bindings.**
- **Project roots**, unless `--project SRC=DIR` names a folder of your own.

A transcript's contents are never rewritten. The old paths inside a conversation stay as they were.

The dry run lists every file it would write (`+`), every conflict (`!`), everything that cannot be written there (`x`, with the reason), each renamed session folder, and the settings that name commands or paths of the old machine (hooks, `statusLine`, MCP server commands). Those settings are not changed: check each one by hand.

Then:

1. Sign in to each account: `claude-acc login <name>`, and `claude` → `/login` for the standard one.
2. Install the plugins again.
3. Put back the removed secrets the backup listed.
4. Check a session: `cd F:\work\app` then `claude --resume`.

Restore before signing in on the new machine if you can. Signing in first creates `~/.claude.json`, and the restored one then conflicts with it.

Accounts are never merged. If an account of the same name already exists on Windows, the restore stops; restore it under another name with `--account work=work-mac`. A project-only restore is the exception: it adds that project's sessions to an existing account of the same name.

## Reusing skills and agents in another project

```sh
claude-acc backup restore app.ccbackup --reuse --project /Users/me/work/app=D:\other --select skills/deploy --select agents/review.md --dry-run
```

`--reuse` takes only `skills`, `agents`, `rules` and `commands` from the project's `.claude/` and `.agents/`. It leaves out its settings, state, checkpoints, memory, sessions and accounts. `--select` narrows it to those paths. Without `--select`, all four kinds come along.

## Conflicts, running it again, and interruptions

- **Existing files** are never overwritten unless you pass `--replace`. A file already there with the same contents counts as done.
- **`--replace`** saves each original first, in `~/.claude-switch/restore-backups/<run>/originals/`.
- **Running a restore again** writes nothing new. An account an earlier run of the same backup created carries a marker, so it is recognised rather than refused.
- **Interruptions.** Every step is logged before it is taken. A restore that fails undoes what it did. If it was killed, the next restore refuses to start, and `claude-acc backup rollback` undoes it.

## Security

- **Encryption.** The file is an [age](https://age-encryption.org) file encrypted with a passphrase (scrypt), so file names and the manifest are not readable without it.
- **Integrity.** Every file's SHA-256 is checked before anything is restored. A damaged, cut short or tampered archive is refused.
- **Passphrase.** Read from the terminal without echo, or from one line of stdin when that is not a terminal. Never from an argument, and never written down.
- **Extraction.** The archive is decrypted into a private folder under `~/.claude-switch` that is removed afterwards. An entry with an absolute path, `..`, or a link pointing out of its folder is refused.
- **Output.** Everything printed is a path, a count, an account or a field name. File contents never are.

## Limits

- **Resuming on Windows is not verified yet.** The renamed folders follow Claude Code's own naming, so `F:\work\app` becomes `F--work-app`, but nobody has yet resumed a moved session on a real Windows machine. Check one before relying on it. Claude Code names the folder from the path exactly as it records the working directory, so a drive letter in another case (`f:` versus `F:`) gives another folder.
- **Symlinks** are not created on Windows, which needs Developer Mode for them. Each one is listed; copy what it points to by hand. On macOS and Linux, a relative link inside its own folder is restored; any other link is listed.
- **Names Windows cannot hold** (`:`, `?`, `CON`, a trailing dot) and two names differing only by case are listed and skipped.
- **Sessions started in a subfolder of a project** come along with a whole account, but not with a project-only backup.
- **The zsh script** (`claude-switch.sh`) has no `backup`: it needs encryption the shell does not have.
- **Not a full machine backup.** Source code, Claude Code itself, and other tools' settings are out of scope.
