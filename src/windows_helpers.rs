// Windows-specific helper utilities for UX improvements

use std::io;
use std::path::{Path, PathBuf};

/// Check if running with sufficient ExecutionPolicy for PowerShell profiles
pub fn check_execution_policy() -> io::Result<bool> {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", "Get-ExecutionPolicy -Scope CurrentUser"])
        .output()?;
    
    let policy = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
    Ok(matches!(policy.as_str(), "remotesigned" | "unrestricted" | "bypass"))
}

/// Attempt to set ExecutionPolicy for CurrentUser (requires user consent)
pub fn set_execution_policy_interactive() -> io::Result<()> {
    let status = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force"
        ])
        .status()?;
    
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Failed to set ExecutionPolicy"
        ))
    }
}

/// Move directory to Windows Recycle Bin using COM IFileOperation
/// Returns the original path on success (Recycle Bin location is opaque on Windows)
pub fn move_to_recycle_bin(path: &Path) -> io::Result<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::ffi::OsStr;
        
        // Use PowerShell's Recycle Bin COM wrapper as most reliable cross-version approach
        // Direct COM from Rust requires windows-rs crate; this keeps zero extra deps
        let path_str = path.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Path not valid UTF-8")
        })?;
        
        let script = format!(
            r#"Add-Type -AssemblyName Microsoft.VisualBasic; \
               [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory('{}', \
               [Microsoft.VisualBasic.FileIO.UIOption]::OnlyErrorDialogs, \
               [Microsoft.VisualBasic.FileIO.RecycleOption]::SendToRecycleBin)"#,
            path_str.replace('\\', "\\\\").replace('\'', "''")
        );
        
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &script])
            .output()?;
        
        if output.status.success() {
            Ok(path.to_path_buf())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // User cancelled or operation refused
            if stderr.contains("canceled") || stderr.contains("cancelled") {
                Err(io::Error::new(io::ErrorKind::Interrupted, "User cancelled"))
            } else {
                Err(io::Error::new(io::ErrorKind::Other, stderr.to_string()))
            }
        }
    }
    
    #[cfg(not(windows))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Recycle Bin only on Windows"
        ))
    }
}

/// Create a progress spinner for long operations (simplified for Windows Terminal)
pub struct ProgressSpinner {
    message: String,
    frames: Vec<&'static str>,
    current: usize,
}

impl ProgressSpinner {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            current: 0,
        }
    }
    
    pub fn tick(&mut self) {
        print!("\r{} {}", self.frames[self.current], self.message);
        use std::io::Write;
        std::io::stdout().flush().ok();
        self.current = (self.current + 1) % self.frames.len();
    }
    
    pub fn finish(&self, final_msg: &str) {
        println!("\r✓ {}", final_msg);
    }
    
    pub fn fail(&self, error_msg: &str) {
        println!("\r✗ {}", error_msg);
    }
}

/// Detect if running in Windows Terminal (supports better rendering)
pub fn is_windows_terminal() -> bool {
    std::env::var("WT_SESSION").is_ok()
}

/// Add current directory to PATH for this session (doesn't persist across shells)
pub fn add_to_path_current_session(dir: &Path) -> io::Result<()> {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let dir_str = dir.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Path not valid UTF-8")
    })?;
    
    if !current_path.contains(dir_str) {
        let new_path = format!("{};{}", dir_str, current_path);
        std::env::set_var("PATH", new_path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    #[cfg(windows)]
    fn execution_policy_check_returns_bool() {
        // Should not panic
        let _ = check_execution_policy();
    }
    
    #[test]
    fn progress_spinner_doesnt_panic() {
        let mut spinner = ProgressSpinner::new("Testing");
        spinner.tick();
        spinner.finish("Done");
    }
}

/// Enable ANSI color codes on Windows 10+ terminals
/// Returns true if colors were enabled or already supported
#[cfg(windows)]
pub fn enable_ansi_colors() -> bool {
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::STD_OUTPUT_HANDLE;
    use winapi::um::wincon::ENABLE_VIRTUAL_TERMINAL_PROCESSING;

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() {
            return false;
        }

        let mut mode: u32 = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }

        // Already enabled
        if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
            return true;
        }

        // Try to enable
        mode |= ENABLE_VIRTUAL_TERMINAL_PROCESSING;
        SetConsoleMode(handle, mode) != 0
    }
}
