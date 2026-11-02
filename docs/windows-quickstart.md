# Windows Quick Start Guide

**Claude Code Account Switcher** — bind different Claude accounts to different directories on Windows.

## One-Command Install

Open PowerShell and run:

```powershell
irm https://raw.githubusercontent.com/Masniper/claude-code-account-switcher/build/windows-preview/install-windows.ps1 | iex
```

This automatically:
- ✓ Checks/sets ExecutionPolicy (asks permission first)
- ✓ Downloads latest release for your architecture
- ✓ Runs `claude-acc install`
- ✓ Adds shell integration to your PowerShell profile

**After install:** Restart PowerShell (or run `. $PROFILE`)

---

## Quick Example

```powershell
# Add work account
claude-acc add work
# Claude Code TUI will open — complete login there

# Link to directory
cd C:\Projects\Work
claude-acc link work

# Now all terminals in C:\Projects\Work use work account automatically
claude chat "test"
```

---

## What's Different on Windows

### 1. **Login Flow**
Windows uses the interactive TUI flow (`claude-acc run <name>`) instead of `claude auth login` to avoid race conditions where the OAuth callback arrives before the terminal prompt renders.

### 2. **ExecutionPolicy**
PowerShell blocks profile scripts by default. The installer asks to set it to `RemoteSigned` (CurrentUser scope only).

**Manual fix if needed:**
```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

### 3. **Path Updates**
Shell integration adds `~\.claude-switch\bin` to PATH. Takes effect in **new** PowerShell sessions.

**Quick reload without restart:**
```powershell
. $PROFILE
```

### 4. **Recycle Bin**
`claude-acc remove` sends accounts to Recycle Bin (undoable). Direct delete only with `--purge`.

```powershell
claude-acc remove old-account       # → Recycle Bin
claude-acc remove old-account --purge  # → Permanent
```

---

## VS Code Integration



---

## VS Code Integration

The VS Code extension doesn't use PATH, so extra step needed:

```powershell
claude-acc vscode install
```

This sets `claudeCode.claudeProcessWrapper` to point at our wrapper. Restart VS Code after.

**Check status:**
```powershell
claude-acc vscode status
```

---

## Troubleshooting

### "ExecutionPolicy blocks profile scripts"

**Symptom:** `claude-acc` not found after install

**Fix:**
```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
. $PROFILE
```

### "PATH not updated"

**Symptom:** New terminal doesn't see `claude-acc`

**Fix:** Restart PowerShell or run `. $PROFILE`

### "Login didn't complete"

**Symptom:** Account created but not logged in

**Fix:** Use `claude-acc run <name>` to trigger TUI login:
```powershell
claude-acc run work
# Complete login in Claude Code TUI
```

### VS Code not picking up account

1. Check wrapper status: `claude-acc vscode status`
2. If "off", install it: `claude-acc vscode install`
3. **Restart VS Code** (not just reload window)
4. Verify in workspace: `claude-acc status`

---

## Common Tasks

```powershell
# Switch between accounts (automatic by directory)
cd C:\Work           # → work account
cd C:\Personal       # → personal account (or default)

# Manual one-off
claude-acc run work chat "test as work account"

# List all accounts
claude-acc list

# Check which account is active
claude-acc status

# Check rate limits
claude-acc usage

# Set default account
claude-acc default work

# Unlink directory
cd C:\SomeProject
claude-acc unlink

# Copy conversation between accounts
claude-acc session copy --to work
```

---

## Known Issues

### Issue #75: Microsoft Store version not supported
Claude Code from Microsoft Store can't run with separate profiles (Store app container restrictions). **Workaround:** Install from [claude.com/download](https://claude.com/download).

### VS Code multi-profile limitation
Only default VS Code profile gets the wrapper setting. **Workaround:** Work in default profile, or manually add `claudeCode.claudeProcessWrapper` to `settings.applyToAllProfiles`.

---

**Report issues:** [GitHub Issues](https://github.com/Masniper/claude-code-account-switcher/issues)

The VS Code extension doesn't use PATH, so extra step needed:

```powershell
claude-acc vscode install
```

This sets `claudeCode.claudeProcessWrapper` to point at our wrapper. Restart VS Code after.

**Check status:**
```powershell
claude-acc vscode status
```

---

## Troubleshooting

### "ExecutionPolicy blocks profile scripts"

**Symptom:** `claude-acc` not found after install

**Fix:**
```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
. $PROFILE
```

### "PATH not updated"

**Symptom:** New terminal doesn't see `claude-acc`

**Fix:** Restart PowerShell or run `. $PROFILE`

### "Login didn't complete"

**Symptom:** Account created but not logged in

**Fix:** Use `claude-acc run <name>` to trigger TUI login:
```powershell
claude-acc run work
# Complete login in Claude Code TUI
```

### VS Code not picking up account

1. Check wrapper status: `claude-acc vscode status`
2. If "off", install it: `claude-acc vscode install`
3. **Restart VS Code** (not just reload window)
4. Verify in workspace: `claude-acc status`

---

## Common Tasks

```powershell
# Switch between accounts (automatic by directory)
cd C:\Work           # → work account
cd C:\Personal       # → personal account (or default)

# Manual one-off
claude-acc run work chat "test as work account"

# List all accounts
claude-acc list

# Check which account is active
claude-acc status

# Check rate limits
claude-acc usage

# Set default account
claude-acc default work

# Unlink directory
cd C:\SomeProject
claude-acc unlink

# Copy conversation between accounts
claude-acc session copy --to work
```

---

## Known Issues

### Issue #75: Microsoft Store version not supported
Claude Code from Microsoft Store can't run with separate profiles (Store app container restrictions). **Workaround:** Install from [claude.com/download](https://claude.com/download).

### VS Code multi-profile limitation
Only default VS Code profile gets the wrapper setting. **Workaround:** Work in default profile, or manually add `claudeCode.claudeProcessWrapper` to `settings.applyToAllProfiles`.

---

**Report issues:** [GitHub Issues](https://github.com/Masniper/claude-code-account-switcher/issues)

**Русская версия:** [README.ru.md](../README.ru.md)

