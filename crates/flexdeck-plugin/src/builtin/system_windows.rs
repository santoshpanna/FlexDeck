//! The live [`SystemBackend`] for Windows.
//!
//! Volume and mute go through the default render endpoint's
//! `IAudioEndpointVolume`; device names come from the endpoint property store;
//! switching the default device uses `IPolicyConfig`, the interface Windows'
//! own Sound settings use. It is undocumented but has been stable since
//! Windows 7, and every audio switcher relies on it — there is no documented
//! alternative.
//!
//! # Threading
//!
//! The backend is called from the sampler task and from press handlers, which
//! run on different threads. COM pointers are therefore never kept: each call
//! joins the multithreaded apartment and builds what it needs. That costs
//! microseconds per call, which is nothing next to a button press.
//!
//! # Testing
//!
//! Nothing here is exercised by the unit tests — they use `RecordingSystem`,
//! for the same reason the input tests never construct `EnigoExecutor`. The
//! `live_*` tests at the bottom are `#[ignore]`d and run by hand on a Windows
//! machine: `cargo test -p flexdeck-plugin -- --ignored live_`.

use std::ffi::c_void;

use windows::core::GUID;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::{PluginError, SystemBackend};

/// `IPolicyConfig`, in its own module because COM method names are not snake
/// case and the interface macro does not accept attributes.
#[allow(non_snake_case)]
pub(crate) mod policy {
    use std::ffi::c_void;

    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;

    /// `IPolicyConfig`. Only `SetDefaultEndpoint` is called; the other slots are
    /// declared so the vtable has the right shape.
    #[windows_core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: windows_core::IUnknown {
        fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut c_void) -> HRESULT;
        fn GetDeviceFormat(
            &self,
            device: PCWSTR,
            default: i32,
            format: *mut *mut c_void,
        ) -> HRESULT;
        fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
        fn SetDeviceFormat(
            &self,
            device: PCWSTR,
            endpoint: *mut c_void,
            mix: *mut c_void,
        ) -> HRESULT;
        fn GetProcessingPeriod(
            &self,
            device: PCWSTR,
            default: i32,
            default_period: *mut i64,
            min_period: *mut i64,
        ) -> HRESULT;
        fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut i64) -> HRESULT;
        fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn SetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
        fn GetPropertyValue(
            &self,
            device: PCWSTR,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        fn SetPropertyValue(
            &self,
            device: PCWSTR,
            key: *const c_void,
            value: *mut c_void,
        ) -> HRESULT;
        fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    /// Make `id` (NUL-terminated UTF-16) the default for every role, the way
    /// Sound settings does.
    pub fn set_default_endpoint(policy: &IPolicyConfig, id: &[u16]) -> windows::core::Result<()> {
        use windows::Win32::Media::Audio::{eCommunications, eConsole, eMultimedia};
        for role in [eConsole, eMultimedia, eCommunications] {
            unsafe { policy.SetDefaultEndpoint(PCWSTR(id.as_ptr()), role).ok()? };
        }
        Ok(())
    }
}
pub(crate) use policy::IPolicyConfig;

pub(crate) const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// The real thing. See the module docs.
#[derive(Debug, Default)]
pub struct WindowsSystem {
    /// The previous `(idle, total)` reading, because CPU usage is a rate.
    last_cpu: Option<(u64, u64)>,
}

impl WindowsSystem {
    pub fn new() -> Self {
        Self::default()
    }
}

pub(crate) fn failed(what: &str, error: windows::core::Error) -> PluginError {
    PluginError::Failed(format!("{what}: {}", error.message()))
}

pub(crate) fn enumerator() -> Result<IMMDeviceEnumerator, PluginError> {
    unsafe {
        // S_FALSE ("already initialised") and RPC_E_CHANGED_MODE (this thread
        // is single-threaded already) both leave COM usable, so neither is an
        // error worth refusing over.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|error| failed("could not reach the audio system", error))
    }
}

fn default_device() -> Result<IMMDevice, PluginError> {
    unsafe {
        enumerator()?
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|error| failed("there is no default output device", error))
    }
}

fn endpoint_volume() -> Result<IAudioEndpointVolume, PluginError> {
    unsafe {
        default_device()?
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .map_err(|error| failed("could not open the output volume", error))
    }
}

pub(crate) fn friendly_name(device: &IMMDevice) -> Result<String, PluginError> {
    unsafe {
        let store = device
            .OpenPropertyStore(STGM_READ)
            .map_err(|error| failed("could not read the device", error))?;
        let mut value = store
            .GetValue(&PKEY_Device_FriendlyName)
            .map_err(|error| failed("could not read the device name", error))?;
        let text = PropVariantToStringAlloc(&value)
            .map_err(|error| failed("the device name is not text", error))?;
        let name = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const c_void));
        let _ = PropVariantClear(&mut value);
        Ok(name)
    }
}

/// Every active output device as `(id, friendly name)`.
fn devices() -> Result<Vec<(Vec<u16>, String)>, PluginError> {
    unsafe {
        let collection = enumerator()?
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|error| failed("could not list output devices", error))?;
        let count = collection
            .GetCount()
            .map_err(|error| failed("could not count output devices", error))?;
        let mut found = Vec::with_capacity(count as usize);
        for index in 0..count {
            let device = collection
                .Item(index)
                .map_err(|error| failed("could not read an output device", error))?;
            let id = device
                .GetId()
                .map_err(|error| failed("an output device has no id", error))?;
            let wide: Vec<u16> = id.as_wide().iter().copied().chain([0]).collect();
            CoTaskMemFree(Some(id.0 as *const c_void));
            found.push((wide, friendly_name(&device)?));
        }
        Ok(found)
    }
}

fn filetime(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

impl SystemBackend for WindowsSystem {
    fn volume(&mut self) -> Result<f64, PluginError> {
        let level = unsafe {
            endpoint_volume()?
                .GetMasterVolumeLevelScalar()
                .map_err(|error| failed("could not read the volume", error))?
        };
        Ok((f64::from(level) * 100.0).round())
    }

    fn set_volume(&mut self, percent: f64) -> Result<(), PluginError> {
        let level = (percent.clamp(0.0, 100.0) / 100.0) as f32;
        unsafe {
            endpoint_volume()?
                .SetMasterVolumeLevelScalar(level, std::ptr::null())
                .map_err(|error| failed("could not set the volume", error))
        }
    }

    fn muted(&mut self) -> Result<bool, PluginError> {
        unsafe {
            endpoint_volume()?
                .GetMute()
                .map(|muted| muted.as_bool())
                .map_err(|error| failed("could not read mute", error))
        }
    }

    fn set_muted(&mut self, muted: bool) -> Result<(), PluginError> {
        unsafe {
            endpoint_volume()?
                .SetMute(muted, std::ptr::null())
                .map_err(|error| failed("could not change mute", error))
        }
    }

    fn output_device(&mut self) -> Result<String, PluginError> {
        friendly_name(&default_device()?)
    }

    fn set_output_device(&mut self, device: &str) -> Result<(), PluginError> {
        let all = devices()?;
        let wanted = device.trim();
        // Exact first, then case-insensitive, then a unique substring: people
        // type "Headphones", Windows calls it "Headphones (WH-1000XM4)".
        let exact = all.iter().find(|(_, name)| name == wanted);
        let insensitive = || {
            all.iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(wanted))
        };
        let partial = || {
            let lower = wanted.to_lowercase();
            let matches: Vec<_> = all
                .iter()
                .filter(|(_, name)| name.to_lowercase().contains(&lower))
                .collect();
            (matches.len() == 1).then(|| matches[0])
        };
        let Some((id, _)) = exact.or_else(insensitive).or_else(partial) else {
            let names: Vec<&str> = all.iter().map(|(_, name)| name.as_str()).collect();
            return Err(PluginError::Failed(format!(
                "no single output device matches '{wanted}' (there are: {})",
                names.join(", ")
            )));
        };

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let policy: IPolicyConfig =
                CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
                    .map_err(|error| failed("could not reach the sound settings", error))?;
            policy::set_default_endpoint(&policy, id)
                .map_err(|error| failed("could not switch the output device", error))?;
        }
        Ok(())
    }

    fn output_devices(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(devices()?.into_iter().map(|(_, name)| name).collect())
    }

    fn cpu_usage(&mut self) -> Result<f64, PluginError> {
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe {
            GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user))
                .map_err(|error| failed("could not read processor time", error))?;
        }
        // Kernel time *includes* idle time, so total is kernel + user.
        let idle = filetime(idle);
        let total = filetime(kernel) + filetime(user);

        let usage = match self.last_cpu.replace((idle, total)) {
            Some((last_idle, last_total)) if total > last_total => {
                let busy = (total - last_total).saturating_sub(idle - last_idle);
                busy as f64 / (total - last_total) as f64
            }
            // The first reading has nothing to compare against, so report the
            // average since boot rather than inventing a zero.
            _ if total > 0 => total.saturating_sub(idle) as f64 / total as f64,
            _ => 0.0,
        };
        Ok((usage * 100.0).clamp(0.0, 100.0).round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "touches the real audio system; run by hand on Windows"]
    fn live_reads_the_volume_mute_device_and_cpu() {
        let mut system = WindowsSystem::new();
        let volume = system.volume().expect("volume");
        assert!((0.0..=100.0).contains(&volume));
        system.muted().expect("mute");
        let device = system.output_device().expect("device");
        assert!(system.output_devices().expect("devices").contains(&device));
        system.cpu_usage().expect("cpu");
        let cpu = system.cpu_usage().expect("cpu again");
        assert!((0.0..=100.0).contains(&cpu));
    }

    #[test]
    #[ignore = "changes the real volume briefly; run by hand on Windows"]
    fn live_sets_the_volume_and_puts_it_back() {
        let mut system = WindowsSystem::new();
        let before = system.volume().expect("volume");
        let target = if before > 50.0 {
            before - 5.0
        } else {
            before + 5.0
        };
        system.set_volume(target).expect("set");
        assert_eq!(system.volume().expect("volume"), target);
        system.set_volume(before).expect("restore");
    }
}
