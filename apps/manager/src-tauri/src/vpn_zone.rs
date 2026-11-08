use crate::backend::AppError;
use tauri::{AppHandle, Manager};

#[derive(serde::Serialize)]
pub struct ExitLocation {
    city: String,
    country: String,
    zone: String,
}

#[tauri::command]
pub async fn vpn_exit() -> Result<ExitLocation, AppError> {
    tauri::async_runtime::spawn_blocking(platform::lookup)
        .await
        .map_err(|_| AppError::BackendFailed)?
}

fn backup(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|_| AppError::BackendFailed)?
        .join("previous-time-zone"))
}

#[tauri::command]
pub fn can_restore_zone(app: AppHandle) -> Result<bool, AppError> {
    Ok(backup(&app)?.is_file())
}

#[tauri::command]
pub async fn change_vpn_zone(app: AppHandle, zone: Option<String>) -> Result<bool, AppError> {
    let path = backup(&app)?;
    tauri::async_runtime::spawn_blocking(move || platform::change(&path, zone.as_deref()))
        .await
        .map_err(|_| AppError::BackendFailed)?
}

#[cfg(windows)]
mod platform {
    use super::{AppError, ExitLocation};
    use crate::watch::vpn_adapter;
    use std::{
        fs, io::Write, os::windows::process::CommandExt, path::Path, process::Command, sync::Mutex,
    };

    const FAILED: AppError = AppError::BackendFailed;
    static CHANGE: Mutex<()> = Mutex::new(());

    #[link(name = "icu")]
    unsafe extern "C" {
        fn ucal_getWindowsTimeZoneID(
            id: *const u16,
            len: i32,
            out: *mut u16,
            capacity: i32,
            error: *mut i32,
        ) -> i32;
    }

    fn windows_zone(zone: &str) -> Result<String, AppError> {
        if zone.len() > 128 {
            return Err(FAILED);
        }
        let input: Vec<u16> = zone.encode_utf16().collect();
        let mut output = [0u16; 128];
        let mut error = 0;
        // Both buffers remain alive and ICU receives their exact capacities.
        let length = unsafe {
            ucal_getWindowsTimeZoneID(
                input.as_ptr(),
                input.len() as i32,
                output.as_mut_ptr(),
                output.len() as i32,
                &mut error,
            )
        };
        if error > 0 || length <= 0 {
            return Err(FAILED);
        }
        String::from_utf16(output.get(..length as usize).ok_or(FAILED)?).map_err(|_| FAILED)
    }

    fn run(program: &str, args: &[&str]) -> Result<Vec<u8>, AppError> {
        let system = std::env::var_os("SystemRoot").ok_or(FAILED)?;
        let output = Command::new(Path::new(&system).join("System32").join(program))
            .args(args)
            .creation_flags(0x08000000)
            .output()
            .map_err(|_| FAILED)?;
        if !output.status.success() {
            return Err(FAILED);
        }
        Ok(output.stdout)
    }

    pub fn lookup() -> Result<ExitLocation, AppError> {
        let adapter = vpn_adapter().ok_or(FAILED)?;
        let output = run(
            "curl.exe",
            &[
                "-q",
                "--fail",
                "--silent",
                "--max-time",
                "10",
                "--max-filesize",
                "65536",
                "--noproxy",
                "*",
                "https://ipwho.is/?fields=success,city,country,timezone.id",
            ],
        )?;
        if vpn_adapter().as_deref() != Some(&adapter) {
            return Err(FAILED);
        }
        let data: serde_json::Value = serde_json::from_slice(&output).map_err(|_| FAILED)?;
        if data["success"].as_bool() != Some(true) {
            return Err(FAILED);
        }
        let zone = data["timezone"]["id"].as_str().ok_or(FAILED)?;
        windows_zone(zone)?;
        Ok(ExitLocation {
            city: data["city"].as_str().unwrap_or_default().into(),
            country: data["country"].as_str().ok_or(FAILED)?.into(),
            zone: zone.into(),
        })
    }

    pub fn change(path: &Path, zone: Option<&str>) -> Result<bool, AppError> {
        let _lock = CHANGE.lock().map_err(|_| FAILED)?;
        if let Some(zone) = zone {
            vpn_adapter().ok_or(FAILED)?;
            let target = windows_zone(zone)?;
            if !path.exists() {
                let current = run("tzutil.exe", &["/g"])?;
                if current.iter().all(u8::is_ascii_whitespace) {
                    return Err(FAILED);
                }
                fs::create_dir_all(path.parent().ok_or(FAILED)?).map_err(|_| FAILED)?;
                let mut file = fs::File::create_new(path).map_err(|_| FAILED)?;
                if file
                    .write_all(&current)
                    .and_then(|_| file.sync_all())
                    .is_err()
                {
                    drop(file);
                    let _ = fs::remove_file(path);
                    return Err(FAILED);
                }
            }
            run("tzutil.exe", &["/s", &target])?;
            Ok(true)
        } else {
            let current = fs::read_to_string(path).map_err(|_| FAILED)?;
            run("tzutil.exe", &["/s", current.trim()])?;
            fs::remove_file(path).map_err(|_| FAILED)?;
            Ok(false)
        }
    }

    #[test]
    fn maps_exit_time_zones_without_hardcoding_a_country() {
        assert_eq!(
            windows_zone("Europe/Berlin").unwrap(),
            "W. Europe Standard Time"
        );
        assert_eq!(
            windows_zone("America/New_York").unwrap(),
            "Eastern Standard Time"
        );
        assert!(windows_zone("Invalid/Zone").is_err());
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{AppError, ExitLocation};
    pub fn lookup() -> Result<ExitLocation, AppError> {
        Err(AppError::BackendFailed)
    }
    pub fn change(_: &std::path::Path, _: Option<&str>) -> Result<bool, AppError> {
        Err(AppError::BackendFailed)
    }
}
