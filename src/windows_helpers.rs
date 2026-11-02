// Windows-only checks for `install` and `doctor`.

/// Whether PowerShell will run the profile script `install` adds: false for
/// `Restricted`, `AllSigned` and `Undefined` (which falls back to
/// `Restricted` on Windows clients). `None` if PowerShell could not be asked.
pub fn execution_policy_allows_profile() -> Option<bool> {
    let out = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-ExecutionPolicy",
        ])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| policy_allows_profile(&String::from_utf8_lossy(&out.stdout)))
}

fn policy_allows_profile(policy: &str) -> bool {
    matches!(
        policy.trim().to_ascii_lowercase().as_str(),
        "remotesigned" | "unrestricted" | "bypass"
    )
}

/// The first `claude.exe` on `PATH`, if it is one of the Microsoft Store's app
/// execution aliases (they live under `WindowsApps`). `claude-acc` runs
/// whatever `claude` resolves to, so an alias there runs something other
/// than a Claude Code install.
pub fn claude_is_store_alias() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("claude.exe"))
            .find(|exe| exe.is_file())
            .is_some_and(|exe| is_store_alias(&exe))
    })
}

fn is_store_alias(exe: &std::path::Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_policies_that_run_a_profile_script_pass() {
        for ok in ["RemoteSigned\r\n", "Unrestricted", "bypass"] {
            assert!(policy_allows_profile(ok), "{ok:?}");
        }
        for blocked in ["Restricted", "AllSigned", "Undefined", ""] {
            assert!(!policy_allows_profile(blocked), "{blocked:?}");
        }
    }

    #[test]
    fn a_store_alias_is_recognised_by_its_directory() {
        use std::path::Path;
        assert!(is_store_alias(Path::new(
            r"C:\Users\a\AppData\Local\Microsoft\WindowsApps\claude.exe"
        )));
        assert!(!is_store_alias(Path::new(
            r"C:\Users\a\.local\bin\claude.exe"
        )));
    }
}
