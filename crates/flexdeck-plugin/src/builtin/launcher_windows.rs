//! The Windows half of the launcher: the Start menu's apps, and closing a
//! program.
//!
//! # Apps
//!
//! The shell's `AppsFolder` (`shell:AppsFolder`) is the list the Start menu
//! draws: desktop programs and Store apps alike, each with a display name and a
//! parsing name that opens it — `shell:AppsFolder\<parsing name>` — whether it
//! is a shortcut or a packaged app's id. So one list covers both, and opening
//! one goes through the same `ShellExecute` everything else uses.
//!
//! # Closing
//!
//! Politely first: every visible top-level window the program owns is sent
//! `WM_CLOSE`, exactly as clicking its X would, so it can save or ask to. Only
//! if it is still running when the wait is up, and only when asked to force,
//! is it ended with `TerminateProcess` — which loses unsaved work, and is why
//! forcing is the person's choice and not a fallback.

use std::time::{Duration, Instant};

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_OBJECT_0};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem,
    KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, GW_OWNER,
    WM_CLOSE,
};

use super::launcher::{Closed, InstalledApp};
use super::system_windows::failed;
use crate::PluginError;

/// Every app in the Start menu, by name.
pub fn apps() -> Result<Vec<InstalledApp>, PluginError> {
    unsafe {
        // The shell's folders want a single-threaded apartment; on a thread
        // that is already multithreaded this fails harmlessly and the calls
        // below still work through COM's marshalling.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
            .map_err(|error| failed("could not open the Start menu's apps", error))?;
        let items: IEnumShellItems = folder
            .BindToHandler(None, &BHID_EnumItems)
            .map_err(|error| failed("could not list the Start menu's apps", error))?;

        let mut found = Vec::new();
        loop {
            let mut item = [None];
            let mut fetched = 0;
            if items.Next(&mut item, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = item[0].take() else { break };
            let (Some(name), Some(parsing)) = (
                display_name(&item, SIGDN_NORMALDISPLAY),
                display_name(&item, SIGDN_PARENTRELATIVEPARSING),
            ) else {
                continue;
            };
            // Uninstallers and help files clutter the list and are never what
            // a button is for.
            let lower = name.to_lowercase();
            if lower.starts_with("uninstall") || lower.contains("uninstaller") {
                continue;
            }
            found.push(InstalledApp {
                name,
                target: format!("shell:AppsFolder\\{parsing}"),
            });
        }
        found.sort_by_key(|app| app.name.to_lowercase());
        found.dedup_by(|a, b| a.target == b.target);
        Ok(found)
    }
}

fn display_name(item: &IShellItem, kind: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    unsafe {
        let raw: PWSTR = item.GetDisplayName(kind).ok()?;
        let text = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
        text.filter(|text| !text.trim().is_empty())
    }
}

/// Every process, as `(pid, executable lowercased)`.
fn processes() -> Result<Vec<(u32, String)>, PluginError> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|error| failed("could not list running programs", error))?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = Vec::new();
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let length = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..length]).to_lowercase();
            found.push((entry.th32ProcessID, exe));
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        Ok(found)
    }
}

/// The visible, unowned top-level windows — the ones with a taskbar button —
/// as `(window, owning process)`.
fn top_windows() -> Vec<(HWND, u32)> {
    unsafe extern "system" fn collect(window: HWND, data: LPARAM) -> BOOL {
        let found = &mut *(data.0 as *mut Vec<(HWND, u32)>);
        let unowned = GetWindow(window, GW_OWNER).map_or(true, |owner| owner.is_invalid());
        if IsWindowVisible(window).as_bool() && unowned {
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, Some(&mut pid));
            found.push((window, pid));
        }
        true.into()
    }
    let mut found: Vec<(HWND, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// The programs with a window open, by executable, without duplicates.
pub fn windowed_processes() -> Result<Vec<String>, PluginError> {
    let windows = top_windows();
    let mut names: Vec<String> = processes()?
        .into_iter()
        .filter(|(pid, _)| windows.iter().any(|(_, owner)| owner == pid))
        .map(|(_, exe)| exe)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// A process handle that closes itself.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub fn close(exe: &str, force: bool, wait: Duration) -> Result<Closed, PluginError> {
    let own = unsafe { GetCurrentProcessId() };
    let pids: Vec<u32> = processes()?
        .into_iter()
        .filter(|(pid, name)| name == exe && *pid != own)
        .map(|(pid, _)| pid)
        .collect();
    if pids.is_empty() {
        return Err(PluginError::Failed(format!("{exe} is not running")));
    }

    // Opened before anything is asked to close, so a process that exits and
    // has its id reused cannot be mistaken for the one we meant.
    let mut handles = Vec::with_capacity(pids.len());
    for &pid in &pids {
        match unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid) } {
            Ok(handle) => handles.push(Process(handle)),
            Err(error) => {
                return Err(PluginError::Failed(format!(
                    "{exe} could not be reached ({}); if it runs as administrator, \
                     flexdeck has to as well",
                    error.message()
                )))
            }
        }
    }

    let windows: Vec<HWND> = top_windows()
        .into_iter()
        .filter(|(_, owner)| pids.contains(owner))
        .map(|(window, _)| window)
        .collect();
    for window in &windows {
        unsafe {
            let _ = PostMessageW(Some(*window), WM_CLOSE, Default::default(), Default::default());
        }
    }

    let deadline = Instant::now() + if windows.is_empty() { Duration::ZERO } else { wait };
    let mut remaining: Vec<&Process> = handles.iter().collect();
    while !remaining.is_empty() {
        let left = deadline.saturating_duration_since(Instant::now());
        let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
        let exited = unsafe { WaitForSingleObject(remaining[0].0, millis) } == WAIT_OBJECT_0;
        if exited {
            remaining.remove(0);
        } else {
            break;
        }
    }
    if remaining.is_empty() {
        return Ok(Closed::Asked);
    }
    if !force {
        return Err(PluginError::Failed(if windows.is_empty() {
            format!("{exe} has no window to close; close it with force to end it")
        } else {
            format!(
                "{exe} did not close within {}s (it may be asking to save); close it with \
                 force to end it",
                wait.as_secs()
            )
        }));
    }
    for process in remaining {
        unsafe {
            TerminateProcess(process.0, 1)
                .map_err(|error| failed(&format!("could not end {exe}"), error))?;
        }
    }
    Ok(Closed::Forced)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads the real Start menu and process list; run by hand on Windows"]
    fn live_lists_apps_and_windows() {
        let apps = apps().expect("apps");
        eprintln!("{} apps, e.g. {:?}", apps.len(), apps.iter().take(3).collect::<Vec<_>>());
        assert!(!apps.is_empty());
        let running = windowed_processes().expect("windows");
        eprintln!("windows: {running:?}");
        assert!(!running.is_empty(), "this desktop has at least one window");
    }
}
