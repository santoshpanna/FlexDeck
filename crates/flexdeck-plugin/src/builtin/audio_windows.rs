//! The live [`AudioBackend`] for Windows.
//!
//! Devices are endpoints from `IMMDeviceEnumerator`, each with its own
//! `IAudioEndpointVolume`. Applications are audio *sessions*: every active
//! output device keeps a list (`IAudioSessionManager2`), each session belongs to
//! a process, and its `ISimpleAudioVolume` is the slider the Volume Mixer
//! shows. One program can hold several sessions — a browser opens one per tab
//! that plays — so setting a program's volume sets all of them.
//!
//! The threading rule is `system_windows`': no COM pointer outlives a call.

use std::ffi::c_void;

use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::core::PCWSTR;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::Media::Audio::{
    eCapture, eMultimedia, eRender, EDataFlow, IAudioSessionControl2, IAudioSessionManager2,
    IMMDevice, ISimpleAudioVolume, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::audio::{exe_name, pick, AudioApp, AudioBackend, AudioDevice, Flow};
use super::system_windows::{
    enumerator, failed, friendly_name, policy, IPolicyConfig, CLSID_POLICY_CONFIG_CLIENT,
};
use crate::PluginError;

/// The real thing. See the module docs.
#[derive(Debug, Default)]
pub struct WindowsAudio;

impl WindowsAudio {
    pub fn new() -> Self {
        Self
    }
}

fn data_flow(flow: Flow) -> EDataFlow {
    match flow {
        Flow::Output => eRender,
        Flow::Input => eCapture,
    }
}

/// One endpoint: its id (NUL-terminated UTF-16), name, and the device itself.
struct Endpoint {
    id: Vec<u16>,
    name: String,
    device: IMMDevice,
}

fn endpoints(flow: Flow) -> Result<Vec<Endpoint>, PluginError> {
    unsafe {
        let collection = enumerator()?
            .EnumAudioEndpoints(data_flow(flow), DEVICE_STATE_ACTIVE)
            .map_err(|error| failed(&format!("could not list {} devices", flow.noun()), error))?;
        let count = collection
            .GetCount()
            .map_err(|error| failed("could not count devices", error))?;
        let mut found = Vec::with_capacity(count as usize);
        for index in 0..count {
            let device = collection
                .Item(index)
                .map_err(|error| failed("could not read a device", error))?;
            let id = device
                .GetId()
                .map_err(|error| failed("a device has no id", error))?;
            let wide: Vec<u16> = id.as_wide().iter().copied().chain([0]).collect();
            CoTaskMemFree(Some(id.0 as *const c_void));
            found.push(Endpoint {
                id: wide,
                name: friendly_name(&device)?,
                device,
            });
        }
        Ok(found)
    }
}

fn default_id(flow: Flow) -> Option<Vec<u16>> {
    unsafe {
        let device = enumerator()
            .ok()?
            .GetDefaultAudioEndpoint(data_flow(flow), eMultimedia)
            .ok()?;
        let id = device.GetId().ok()?;
        let wide = id.as_wide().iter().copied().chain([0]).collect();
        CoTaskMemFree(Some(id.0 as *const c_void));
        Some(wide)
    }
}

/// The endpoint `device` names, or the default when it is blank.
fn endpoint(flow: Flow, device: &str) -> Result<Endpoint, PluginError> {
    let mut all = endpoints(flow)?;
    let index = if device.trim().is_empty() {
        let default = default_id(flow).ok_or_else(|| {
            PluginError::Failed(format!("there is no default {} device", flow.noun()))
        })?;
        all.iter().position(|endpoint| endpoint.id == default)
    } else {
        let names: Vec<String> = all.iter().map(|endpoint| endpoint.name.clone()).collect();
        let name = pick(&names, device, flow)?.to_string();
        all.iter().position(|endpoint| endpoint.name == name)
    };
    index
        .map(|index| all.swap_remove(index))
        .ok_or_else(|| PluginError::Failed(format!("the {} device went away", flow.noun())))
}

fn volume_of(endpoint: &Endpoint) -> Result<IAudioEndpointVolume, PluginError> {
    unsafe {
        endpoint
            .device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .map_err(|error| failed(&format!("could not open {}'s volume", endpoint.name), error))
    }
}

/// The executable behind a process, lowercased; `None` for one that is gone or
/// that this user may not inspect.
fn process_exe(pid: u32) -> Option<String> {
    process_path(pid)?
        .rsplit(['\\', '/'])
        .next()
        .map(|name| name.to_lowercase())
}

/// The full path of a process's executable.
fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let read = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        read.ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }
}

/// What a program calls itself — "OBS Studio" for `obs64.exe` — from its
/// version resource, in whichever language it lists first.
fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;

        let query = |key: &str| -> Option<(*const c_void, u32)> {
            let key: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
            let mut pointer: *mut c_void = std::ptr::null_mut();
            let mut length = 0u32;
            VerQueryValueW(data.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut pointer, &mut length)
                .as_bool()
                .then_some((pointer as *const c_void, length))
        };

        // The languages it lists, then US English Unicode: what most ship.
        let mut languages: Vec<(u16, u16)> = Vec::new();
        if let Some((pairs, bytes)) = query("\\VarFileInfo\\Translation") {
            let pairs = std::slice::from_raw_parts(pairs.cast::<u16>(), bytes as usize / 2);
            languages.extend(pairs.chunks_exact(2).map(|pair| (pair[0], pair[1])));
        }
        languages.push((0x0409, 0x04B0));
        for (language, page) in languages {
            let key = format!("\\StringFileInfo\\{language:04x}{page:04x}\\FileDescription");
            let Some((text, chars)) = query(&key) else {
                continue;
            };
            if text.is_null() || chars == 0 {
                continue;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(text.cast::<u16>(), chars as usize));
            let text = text.trim_end_matches('\0').trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
        None
    }
}

/// Every audio session on every active output device, with its program.
fn sessions() -> Result<Vec<(String, ISimpleAudioVolume)>, PluginError> {
    Ok(sessions_with_pid()?
        .into_iter()
        .map(|(exe, _, volume)| (exe, volume))
        .collect())
}

/// [`sessions`], keeping each one's process id.
fn sessions_with_pid() -> Result<Vec<(String, u32, ISimpleAudioVolume)>, PluginError> {
    let mut found = Vec::new();
    for endpoint in endpoints(Flow::Output)? {
        unsafe {
            let Ok(manager) = endpoint
                .device
                .Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)
            else {
                continue;
            };
            let Ok(list) = manager.GetSessionEnumerator() else {
                continue;
            };
            let count = list.GetCount().unwrap_or(0);
            for index in 0..count {
                let Ok(control) = list.GetSession(index) else {
                    continue;
                };
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                // Process 0 is the system-sounds session, which is no one's
                // program; a session spanning processes has no single one.
                let pid = match control2.GetProcessId() {
                    Ok(pid) if pid != 0 => pid,
                    _ => continue,
                };
                let Some(exe) = process_exe(pid) else {
                    continue;
                };
                if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                    found.push((exe, pid, volume));
                }
            }
        }
    }
    Ok(found)
}

/// The sessions belonging to `app`, refusing when it has none.
fn app_sessions(app: &str) -> Result<Vec<ISimpleAudioVolume>, PluginError> {
    let exe = exe_name(app);
    let mine: Vec<ISimpleAudioVolume> = sessions()?
        .into_iter()
        .filter(|(owner, _)| *owner == exe)
        .map(|(_, volume)| volume)
        .collect();
    if mine.is_empty() {
        return Err(PluginError::Failed(format!(
            "{exe} has no audio open right now; start it playing and try again"
        )));
    }
    Ok(mine)
}

fn init() {
    // As in `system_windows`: "already initialised" and "different mode" both
    // leave COM usable on this thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

impl AudioBackend for WindowsAudio {
    fn devices(&mut self, flow: Flow) -> Result<Vec<AudioDevice>, PluginError> {
        init();
        let default = default_id(flow);
        Ok(endpoints(flow)?
            .into_iter()
            .map(|endpoint| AudioDevice {
                default: default.as_ref() == Some(&endpoint.id),
                name: endpoint.name,
            })
            .collect())
    }

    fn device_volume(&mut self, flow: Flow, device: &str) -> Result<f64, PluginError> {
        init();
        let level = unsafe {
            volume_of(&endpoint(flow, device)?)?
                .GetMasterVolumeLevelScalar()
                .map_err(|error| failed("could not read the volume", error))?
        };
        Ok((f64::from(level) * 100.0).round())
    }

    fn set_device_volume(
        &mut self,
        flow: Flow,
        device: &str,
        percent: f64,
    ) -> Result<(), PluginError> {
        init();
        let level = (percent.clamp(0.0, 100.0) / 100.0) as f32;
        unsafe {
            volume_of(&endpoint(flow, device)?)?
                .SetMasterVolumeLevelScalar(level, std::ptr::null())
                .map_err(|error| failed("could not set the volume", error))
        }
    }

    fn device_muted(&mut self, flow: Flow, device: &str) -> Result<bool, PluginError> {
        init();
        unsafe {
            volume_of(&endpoint(flow, device)?)?
                .GetMute()
                .map(|muted| muted.as_bool())
                .map_err(|error| failed("could not read mute", error))
        }
    }

    fn set_device_muted(&mut self, flow: Flow, device: &str, muted: bool) -> Result<(), PluginError> {
        init();
        unsafe {
            volume_of(&endpoint(flow, device)?)?
                .SetMute(muted, std::ptr::null())
                .map_err(|error| failed("could not change mute", error))
        }
    }

    fn set_default_device(&mut self, flow: Flow, device: &str) -> Result<(), PluginError> {
        init();
        if device.trim().is_empty() {
            return Err(PluginError::Failed(format!(
                "say which {} device to switch to",
                flow.noun()
            )));
        }
        let target = endpoint(flow, device)?;
        unsafe {
            let policy: IPolicyConfig =
                CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                    .map_err(|error| failed("could not reach the sound settings", error))?;
            policy::set_default_endpoint(&policy, &target.id)
                .map_err(|error| failed("could not switch the device", error))
        }
    }

    fn apps(&mut self) -> Result<Vec<AudioApp>, PluginError> {
        init();
        let mut found: Vec<(String, u32)> = sessions_with_pid()?
            .into_iter()
            .map(|(exe, pid, _)| (exe, pid))
            .collect();
        found.sort();
        found.dedup_by(|a, b| a.0 == b.0);
        Ok(found
            .into_iter()
            .map(|(exe, pid)| AudioApp {
                title: process_path(pid).and_then(|path| file_description(&path)),
                exe,
            })
            .collect())
    }

    fn app_volume(&mut self, exe: &str) -> Result<f64, PluginError> {
        init();
        let first = app_sessions(exe)?.remove(0);
        let level = unsafe {
            first
                .GetMasterVolume()
                .map_err(|error| failed("could not read the program's volume", error))?
        };
        Ok((f64::from(level) * 100.0).round())
    }

    fn set_app_volume(&mut self, exe: &str, percent: f64) -> Result<(), PluginError> {
        init();
        let level = (percent.clamp(0.0, 100.0) / 100.0) as f32;
        for session in app_sessions(exe)? {
            unsafe {
                session
                    .SetMasterVolume(level, std::ptr::null())
                    .map_err(|error| failed("could not set the program's volume", error))?;
            }
        }
        Ok(())
    }

    fn app_muted(&mut self, exe: &str) -> Result<bool, PluginError> {
        init();
        let first = app_sessions(exe)?.remove(0);
        unsafe {
            first
                .GetMute()
                .map(|muted| muted.as_bool())
                .map_err(|error| failed("could not read the program's mute", error))
        }
    }

    fn set_app_muted(&mut self, exe: &str, muted: bool) -> Result<(), PluginError> {
        init();
        for session in app_sessions(exe)? {
            unsafe {
                session
                    .SetMute(muted, std::ptr::null())
                    .map_err(|error| failed("could not change the program's mute", error))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads the real audio system; run by hand on Windows"]
    fn live_lists_devices_and_programs_and_reads_their_levels() {
        let mut audio = WindowsAudio::new();
        let outputs = audio.devices(Flow::Output).expect("outputs");
        assert!(outputs.iter().filter(|d| d.default).count() <= 1);
        if let Some(first) = outputs.first() {
            let volume = audio.device_volume(Flow::Output, &first.name).expect("volume");
            assert!((0.0..=100.0).contains(&volume));
        }
        let _ = audio.devices(Flow::Input).expect("inputs");
        for app in audio.apps().expect("apps") {
            println!("{} = {:?}", app.exe, app.title);
            let volume = audio.app_volume(&app.exe).expect("app volume");
            assert!((0.0..=100.0).contains(&volume), "{}", app.exe);
        }
    }
}
