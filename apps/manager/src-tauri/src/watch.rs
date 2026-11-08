//! The VPN guard: while it is on and no VPN adapter is up, every Claude
//! process and everything it started is frozen, and thawed when one returns.
//! That covers Claude Desktop on any profile, Claude Code in a terminal, and
//! the Claude panel in an editor, whose editor keeps running.
//!
//! It runs in this process, which is never frozen, so the switch in the window
//! still works at the moment it is needed. Off by default: it stops programs.
use crate::backend::AppError;
use crate::tray;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const SUPPORTED: bool = cfg!(windows);
const CHECK_EVERY: Duration = Duration::from_secs(1);

/// shortcut: an install that runs under `node` is not recognised by name, add
/// a command-line check if the npm build of Claude Code has to be covered.
const CLAUDE_EXECUTABLES: [&str; 2] = ["claude.exe", "claude-vscode.exe"];

const VPN_TERMS: [&str; 19] = [
    "vpn",
    "wireguard",
    "wintun",
    "openconnect",
    "tailscale",
    "nordlynx",
    "proton",
    "mullvad",
    "anyconnect",
    "globalprotect",
    "fortinet",
    "tap-",
    "tap_",
    "tap ",
    "tun-",
    "tun_",
    "tun ",
    "_tun",
    "-tun",
];
const IF_TYPE_PPP: u32 = 23;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchStatus {
    pub supported: bool,
    pub enabled: bool,
    /// The adapter counted as the VPN. `null` means none is up.
    pub vpn: Option<String>,
    pub frozen: usize,
}

#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone)]
struct Process {
    pid: u32,
    parent: u32,
    executable: String,
}

#[derive(Default)]
struct Inner {
    vpn: Option<String>,
    frozen: HashSet<u32>,
}

pub struct Watch {
    enabled: AtomicBool,
    /// Its existence is the stored setting.
    flag: Option<PathBuf>,
    inner: Mutex<Inner>,
}

impl Watch {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> WatchStatus {
        let inner = self.lock();
        WatchStatus {
            supported: SUPPORTED,
            enabled: self.enabled.load(Ordering::SeqCst),
            vpn: inner.vpn.clone(),
            frozen: inner.frozen.len(),
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_vpn_adapter(name: &str, description: &str, if_type: u32) -> bool {
    let text = format!("{name} {description}").to_lowercase();
    if_type == IF_TYPE_PPP || VPN_TERMS.iter().any(|term| text.contains(term))
}

/// Claude's processes and their descendants, from one snapshot. A branch stops
/// at `own_pid`, so a manager started from a Claude session is never frozen,
/// nor what it opens.
fn targets(processes: &[Process], own_pid: u32) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for p in processes {
        children.entry(p.parent).or_default().push(p.pid);
    }
    let mut queue: Vec<u32> = processes
        .iter()
        .filter(|p| {
            let executable = p.executable.to_lowercase();
            CLAUDE_EXECUTABLES.contains(&executable.as_str())
        })
        .map(|p| p.pid)
        .collect();
    let mut found = HashSet::new();
    while let Some(pid) = queue.pop() {
        if pid != own_pid && found.insert(pid) {
            queue.extend(children.get(&pid).into_iter().flatten());
        }
    }
    found
}

/// One check. Returns whether something became frozen when nothing was.
fn tick(watch: &Watch) -> bool {
    let vpn = os::vpn_adapter();
    let guarding = watch.enabled.load(Ordering::SeqCst) && vpn.is_none();
    let mut inner = watch.lock();
    inner.vpn = vpn;
    let was_idle = inner.frozen.is_empty();
    if guarding {
        let processes = os::processes();
        for pid in targets(&processes, std::process::id()) {
            // One suspend per process: suspensions nest, so a second here
            // would need a second resume later.
            if !inner.frozen.contains(&pid) && os::suspend(pid) {
                inner.frozen.insert(pid);
            }
        }
        let alive: HashSet<u32> = processes.iter().map(|p| p.pid).collect();
        inner.frozen.retain(|pid| alive.contains(pid));
    } else {
        for pid in inner.frozen.drain() {
            os::resume(pid);
        }
    }
    was_idle && !inner.frozen.is_empty()
}

fn flag_path(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_config_dir().ok()?.join("vpn-watch-on"))
}

pub fn start(app: &AppHandle) {
    let flag = flag_path(app);
    let enabled = SUPPORTED && flag.as_ref().is_some_and(|f| f.exists());
    app.manage(Watch {
        enabled: AtomicBool::new(enabled),
        flag,
        inner: Mutex::default(),
    });
    if !SUPPORTED {
        return;
    }
    if enabled {
        // A manager that was killed cannot have thawed what it froze. Harmless
        // on anything running, and the first check freezes again if it must.
        for pid in targets(&os::processes(), std::process::id()) {
            os::resume(pid);
        }
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            if tick(&app.state::<Watch>()) {
                // A frozen program looks hung; this says why.
                tray::show(&app);
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

/// Store the setting and apply it now, so the answer is what is in force.
pub fn set_enabled(app: &AppHandle, on: bool) -> Result<WatchStatus, AppError> {
    let watch = app.state::<Watch>();
    if !SUPPORTED {
        return Ok(watch.status());
    }
    let flag = watch.flag.as_ref().ok_or(AppError::BackendFailed)?;
    let stored = if on {
        flag.parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(flag, b""))
    } else {
        match fs::remove_file(flag) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    };
    stored.map_err(|_| AppError::BackendFailed)?;
    watch.enabled.store(on, Ordering::SeqCst);
    tick(&watch);
    Ok(watch.status())
}

/// Before quitting: nothing would be left to thaw them. The stored setting is
/// kept, so the guard is back on the next start.
pub fn release(app: &AppHandle) {
    let watch = app.state::<Watch>();
    watch.enabled.store(false, Ordering::SeqCst);
    tick(&watch);
}

#[cfg(windows)]
mod os {
    use super::{Process, is_vpn_adapter};
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BUFFER_OVERFLOW, HANDLE, INVALID_HANDLE_VALUE, NO_ERROR,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};

    // Undocumented but stable since XP; the documented alternative is one
    // thread at a time, which races with threads the process is starting.
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(process: HANDLE) -> i32;
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    pub fn processes() -> Vec<Process> {
        let mut found = Vec::new();
        // SAFETY: `entry` is initialised with its size as the API requires, and
        // the snapshot handle is closed once, on the only path that opened it.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return found;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                found.push(Process {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    executable: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        found
    }

    fn with_process(pid: u32, act: unsafe extern "system" fn(HANDLE) -> i32) -> bool {
        // SAFETY: the handle is checked before use and closed exactly once.
        unsafe {
            let handle = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
            if handle.is_null() {
                return false;
            }
            let status = act(handle);
            CloseHandle(handle);
            status >= 0
        }
    }

    pub fn suspend(pid: u32) -> bool {
        with_process(pid, NtSuspendProcess)
    }

    pub fn resume(pid: u32) {
        with_process(pid, NtResumeProcess);
    }

    unsafe fn wide(text: *const u16) -> String {
        if text.is_null() {
            return String::new();
        }
        // SAFETY: the caller passes a NUL-terminated string from the API.
        unsafe {
            let len = (0..).take_while(|&i| *text.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
        }
    }

    /// The first adapter that is up, looks like a VPN and has an address. A
    /// failed read answers `None`, which freezes: the guard fails closed.
    pub fn vpn_adapter() -> Option<String> {
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let mut size: u32 = 16 * 1024;
        for _ in 0..3 {
            // u64 for the alignment of the structures the API writes here.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let first = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            // SAFETY: `first` points at `size` writable bytes.
            let result = unsafe {
                GetAdaptersAddresses(
                    u32::from(AF_INET),
                    flags,
                    null::<c_void>(),
                    first,
                    &mut size,
                )
            };
            if result == ERROR_BUFFER_OVERFLOW {
                continue;
            }
            if result != NO_ERROR {
                return None;
            }
            let mut adapter = first;
            while !adapter.is_null() {
                // SAFETY: the list is the API's, alive in `buffer`, and its
                // strings are NUL-terminated.
                unsafe {
                    let a = &*adapter;
                    let name = wide(a.FriendlyName);
                    if a.OperStatus == IfOperStatusUp
                        && !a.FirstUnicastAddress.is_null()
                        && is_vpn_adapter(&name, &wide(a.Description), a.IfType)
                    {
                        return Some(name);
                    }
                    adapter = a.Next;
                }
            }
            return None;
        }
        None
    }
}

#[cfg(not(windows))]
mod os {
    //! `SUPPORTED` is false here, so the guard never runs and the window says
    //! it is unavailable.
    use super::Process;

    pub fn processes() -> Vec<Process> {
        Vec::new()
    }

    pub fn suspend(_pid: u32) -> bool {
        false
    }

    pub fn resume(_pid: u32) {}

    pub fn vpn_adapter() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent: u32, executable: &str) -> Process {
        Process {
            pid,
            parent,
            executable: executable.into(),
        }
    }

    fn sorted(set: HashSet<u32>) -> Vec<u32> {
        let mut all: Vec<u32> = set.into_iter().collect();
        all.sort_unstable();
        all
    }

    #[test]
    fn every_claude_surface_and_its_descendants_are_targets_and_nothing_else() {
        let table = [
            process(1, 0, "explorer.exe"),
            process(10, 1, "Code.exe"),
            process(11, 10, "claude-vscode.exe"),
            process(12, 11, "claude.exe"),
            process(13, 12, "node.exe"),
            process(20, 1, "Claude.exe"),
            process(21, 20, "claude.exe"),
            process(30, 1, "WindowsTerminal.exe"),
            process(31, 30, "claude.exe"),
            process(32, 31, "bash.exe"),
            process(33, 32, "git.exe"),
            process(40, 1, "notepad.exe"),
        ];
        assert_eq!(
            sorted(targets(&table, 999)),
            [11, 12, 13, 20, 21, 31, 32, 33]
        );
    }

    #[test]
    fn this_tool_is_not_claude_even_though_its_name_starts_that_way() {
        // Regression: claude-watch's `^claude($|[ -])` matches these, and a
        // frozen manager could not be asked to stop.
        let table = [
            process(1, 0, "claude-acc-manager.exe"),
            process(2, 0, "claude-acc.exe"),
            process(3, 0, "claudette.exe"),
        ];
        assert!(targets(&table, 999).is_empty());
    }

    #[test]
    fn a_manager_started_from_claude_stays_out_with_what_it_opened() {
        let table = [
            process(1, 0, "claude.exe"),
            process(2, 1, "claude-acc-manager.exe"),
            process(3, 2, "msedgewebview2.exe"),
            process(4, 1, "git.exe"),
        ];
        assert_eq!(sorted(targets(&table, 2)), [1, 4]);
    }

    #[test]
    fn a_parent_id_cycle_ends_the_walk() {
        // Parent ids go stale and get reused, so a cycle can really appear.
        let table = [process(1, 2, "claude.exe"), process(2, 1, "node.exe")];
        assert_eq!(sorted(targets(&table, 999)), [1, 2]);
    }

    #[test]
    fn vpn_adapters_are_told_from_ordinary_ones() {
        for (name, description, if_type) in [
            ("corp.example_3ea3e298", "OpenConnect Tunnel", 53),
            ("Ethernet 2", "Cisco AnyConnect Virtual Adapter", 6),
            ("wg0", "WireGuard Tunnel", 53),
            ("singbox_tun", "sing-box Tunnel", 53),
            ("Office", "WAN Miniport (PPP)", IF_TYPE_PPP),
        ] {
            assert!(is_vpn_adapter(name, description, if_type), "{description}");
        }
        for (name, description, if_type) in [
            ("Wi-Fi", "Intel(R) Wi-Fi 6 AX201 160MHz", 71),
            ("Ethernet", "Realtek PCIe GbE Family Controller", 6),
            ("Bluetooth Network Connection", "Bluetooth Device (PAN)", 6),
            ("Loopback", "Software Loopback Interface 1", 24),
        ] {
            assert!(!is_vpn_adapter(name, description, if_type), "{description}");
        }
    }

    #[test]
    fn status_matches_the_frontend_contract_sample() {
        let sample: serde_json::Value =
            serde_json::from_str(include_str!("../../src/contract.sample.json")).unwrap();
        let status = WatchStatus {
            supported: true,
            enabled: true,
            vpn: Some("wg0".into()),
            frozen: 0,
        };
        assert_eq!(
            sample["watch_status"],
            serde_json::to_value(status).unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn suspending_really_stops_a_process_and_resuming_restarts_it() {
        use std::process::{Command, Stdio};
        use std::thread::sleep;
        let path = std::env::temp_dir().join(format!("cc-watch-{}", std::process::id()));
        let output = fs::File::create(&path).unwrap();
        let mut child = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::from(output))
            .spawn()
            .unwrap();
        let pid = child.id();
        let size = || fs::metadata(&path).unwrap().len();
        let second = Duration::from_secs(1);

        sleep(second * 2);
        assert!(os::processes().iter().any(|p| p.pid == pid));
        assert!(os::suspend(pid));
        sleep(second);
        let frozen_at = size();
        sleep(second * 3);
        let still = size();
        os::resume(pid);
        sleep(second * 3);
        let resumed = size();

        child.kill().unwrap();
        child.wait().unwrap();
        fs::remove_file(&path).unwrap();
        assert!(frozen_at > 0, "ping wrote nothing before being frozen");
        assert_eq!(frozen_at, still, "output grew while frozen");
        assert!(resumed > still, "output did not grow after resuming");
    }
}
