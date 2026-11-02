# Windows preview

[← README](../README.md) · [Русский](ru/windows-quickstart.md)

A preview of the CLI and **Claude Accounts**, the manager app, on Windows, built from the fork's `build/windows-preview` branch. It is unsigned and has not been through upstream review.

## Before you start

- Claude Code must be installed and `claude --version` must work in PowerShell. `claude-acc` runs whatever `claude` resolves to. `claude-acc doctor` warns if that is a Microsoft Store app alias under `WindowsApps` rather than Claude Code.
- The shell integration `install` adds is a PowerShell profile script. If your execution policy does not run profile scripts, `install` and `doctor` say so. To allow it:

  ```powershell
  Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
  ```

## Install

Both files come from the same preview build. They are built together; use them together.

1. In PowerShell, in the folder holding them, install the CLI:

   ```powershell
   .\claude-acc.exe install
   ```

   Use this build's `claude-acc.exe`, not the 0.21.0 release. The VS Code integration on Windows copies the installed binary, and only this build has the mode that copy runs in. `install` keeps an existing binary with the same version number, so remove `%USERPROFILE%\.claude-switch\bin\claude-acc.exe` first if 0.21.0 is there.
2. Run `Claude Accounts_<version>_x64-setup.exe`. Windows SmartScreen warns about an unsigned installer: choose **More info**, then **Run anyway**.

## What differs from macOS

- **Open Claude, login and Add account** open a console window running the bundled `claude-acc`. If the CLI fails, the window waits for a key so its message can be read. Copy that message if you report a problem.
- **Move to Trash** sends the folder to the Recycle Bin. If a folder is too big for the Recycle Bin, Windows asks before deleting it for good; cancelling leaves it in place.
- **VS Code** is pointed at `%USERPROFILE%\.claude-switch\bin\claude-vscode.exe`, a copy of `claude-acc.exe`.
- **Claude Desktop usage** unwraps the profile's key with DPAPI for your user, without a prompt. That Claude Desktop on Windows stores its token this way has not been checked on a real install yet.
- **Sharing the Desktop runtime** (`clone-runtime`) is not available: NTFS has no copy-on-write clone, so a profile downloads its own runtime.

## If something fails

Run `claude-acc doctor` and keep its output, together with the message the console window showed.
