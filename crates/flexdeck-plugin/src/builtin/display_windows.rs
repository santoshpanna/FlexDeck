//! The live [`DisplayBackend`] for Windows.
//!
//! Two mechanisms, one list of screens:
//!
//! * **A laptop's own panel** is dimmed through WMI (`root\WMI`,
//!   `WmiMonitorBrightness` to read, `WmiMonitorBrightnessMethods` to write) —
//!   the same path the brightness slider in Settings takes.
//! * **An external monitor** is dimmed over DDC/CI, the VESA channel down the
//!   video cable, through `dxva2`'s physical-monitor API. Many monitors ship
//!   with DDC/CI off in their own menu, and some docks do not pass it through;
//!   those screens simply do not appear, rather than appearing and failing.
//!
//! A DDC write takes tens of milliseconds and some monitors drop writes that
//! arrive faster than they can apply them; the slider's own pacing (one run in
//! flight per slider) keeps writes from piling up.
//!
//! The threading rule is the other backends': nothing outlives a call.

use windows::core::{w, BOOL, BSTR, PCWSTR};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

use super::display::DisplayBackend;
use super::system_windows::failed;
use crate::PluginError;

/// What the built-in panel is called in the list of screens.
const BUILT_IN: &str = "Built-in display";

#[derive(Debug, Default)]
pub struct WindowsDisplay;

impl WindowsDisplay {
    pub fn new() -> Self {
        Self
    }
}

fn init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

// --- The built-in panel, through WMI -------------------------------------

fn wmi() -> Result<IWbemServices, PluginError> {
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| failed("could not reach WMI", error))?;
        let services = locator
            .ConnectServer(
                &BSTR::from("ROOT\\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
            .map_err(|error| failed("could not open WMI", error))?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
        .map_err(|error| failed("could not secure the WMI connection", error))?;
        Ok(services)
    }
}

/// The first object a WQL query returns, if any.
fn first(services: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
    unsafe {
        let rows = services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
                None,
            )
            .ok()?;
        let mut row = [None];
        let mut returned = 0;
        rows.Next(WBEM_INFINITE, &mut row, &mut returned).ok().ok()?;
        if returned == 0 {
            return None;
        }
        row[0].take()
    }
}

fn panel_brightness() -> Option<f64> {
    let services = wmi().ok()?;
    let object = first(&services, "SELECT CurrentBrightness FROM WmiMonitorBrightness")?;
    let mut value = VARIANT::default();
    unsafe {
        object
            .Get(w!("CurrentBrightness"), 0, &mut value, None, None)
            .ok()?;
    }
    // CIM uint8; the conversion widens whichever integer type it arrives as.
    i32::try_from(&value).ok().map(f64::from)
}

fn set_panel_brightness(percent: f64) -> Result<(), PluginError> {
    let services = wmi()?;
    let instance = first(&services, "SELECT * FROM WmiMonitorBrightnessMethods").ok_or_else(|| {
        PluginError::Failed("this screen's brightness cannot be set through Windows".into())
    })?;
    unsafe {
        let mut path = VARIANT::default();
        instance
            .Get(w!("__PATH"), 0, &mut path, None, None)
            .map_err(|error| failed("could not address the screen", error))?;
        let path = BSTR::try_from(&path)
            .map_err(|error| failed("the screen's address is not text", error))?;

        let mut class = None;
        services
            .GetObject(
                &BSTR::from("WmiMonitorBrightnessMethods"),
                Default::default(),
                None,
                Some(&mut class),
                None,
            )
            .map_err(|error| failed("could not find the brightness method", error))?;
        let class = class.ok_or_else(|| PluginError::Failed("no brightness method".into()))?;
        let mut signature = None;
        class
            .GetMethod(w!("WmiSetBrightness"), 0, &mut signature, std::ptr::null_mut())
            .map_err(|error| failed("could not read the brightness method", error))?;
        let parameters = signature
            .ok_or_else(|| PluginError::Failed("the brightness method takes nothing".into()))?
            .SpawnInstance(0)
            .map_err(|error| failed("could not prepare the brightness call", error))?;
        parameters
            .Put(w!("Timeout"), 0, &VARIANT::from(1i32), 0)
            .map_err(|error| failed("could not set the timeout", error))?;
        parameters
            .Put(
                w!("Brightness"),
                0,
                &VARIANT::from(percent.clamp(0.0, 100.0).round() as u8),
                0,
            )
            .map_err(|error| failed("could not set the level", error))?;
        services
            .ExecMethod(
                &path,
                &BSTR::from("WmiSetBrightness"),
                Default::default(),
                None,
                &parameters,
                None,
                None,
            )
            .map_err(|error| failed("could not change the brightness", error))?;
    }
    Ok(())
}

// --- External monitors, over DDC/CI --------------------------------------

/// One DDC/CI-capable monitor, with the handle the API needs.
struct Ddc {
    name: String,
    handle: PHYSICAL_MONITOR,
}

/// Every monitor that answers DDC/CI, named and numbered. The handles are
/// destroyed when the list is dropped.
struct DdcMonitors(Vec<Ddc>);

impl Drop for DdcMonitors {
    fn drop(&mut self) {
        let handles: Vec<PHYSICAL_MONITOR> = self.0.iter().map(|monitor| monitor.handle).collect();
        if !handles.is_empty() {
            unsafe {
                let _ = DestroyPhysicalMonitors(&handles);
            }
        }
    }
}

fn ddc_monitors() -> DdcMonitors {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        let found = &mut *(data.0 as *mut Vec<HMONITOR>);
        found.push(monitor);
        true.into()
    }

    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        );
    }

    let mut found = Vec::new();
    for handle in handles {
        unsafe {
            let mut count = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(handle, &mut count).is_err() || count == 0 {
                continue;
            }
            let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
            if GetPhysicalMonitorsFromHMONITOR(handle, &mut physical).is_err() {
                continue;
            }
            for monitor in physical {
                // A monitor without DDC/CI fails this read; it cannot be
                // dimmed, so it is not offered.
                let (mut min, mut current, mut max) = (0u32, 0u32, 0u32);
                if GetMonitorBrightness(monitor.hPhysicalMonitor, &mut min, &mut current, &mut max)
                    == 0
                {
                    let _ = DestroyPhysicalMonitors(&[monitor]);
                    continue;
                }
                // A packed struct: copy the field out before borrowing it.
                let raw = monitor.szPhysicalMonitorDescription;
                let description = String::from_utf16_lossy(&raw);
                let description = description.trim_end_matches('\0').trim().to_string();
                found.push(Ddc {
                    name: description,
                    handle: monitor,
                });
            }
        }
    }

    // Two identical monitors share a description; number them so each has a
    // name of its own.
    let names: Vec<String> = found.iter().map(|monitor| monitor.name.clone()).collect();
    for (index, monitor) in found.iter_mut().enumerate() {
        let same = names.iter().filter(|name| **name == names[index]).count();
        if same > 1 || monitor.name.is_empty() {
            let nth = names[..=index]
                .iter()
                .filter(|name| **name == names[index])
                .count();
            let base = if monitor.name.is_empty() {
                "Monitor"
            } else {
                monitor.name.as_str()
            };
            monitor.name = format!("{base} {nth}");
        }
    }
    DdcMonitors(found)
}

/// Read a DDC monitor's brightness as a percentage of its own range.
fn ddc_brightness(monitor: &Ddc) -> Result<f64, PluginError> {
    let (mut min, mut current, mut max) = (0u32, 0u32, 0u32);
    let read = unsafe {
        GetMonitorBrightness(monitor.handle.hPhysicalMonitor, &mut min, &mut current, &mut max)
    };
    if read == 0 || max <= min {
        return Err(PluginError::Failed(format!(
            "{} did not report its brightness",
            monitor.name
        )));
    }
    Ok(((f64::from(current - min) / f64::from(max - min)) * 100.0).round())
}

fn set_ddc_brightness(monitor: &Ddc, percent: f64) -> Result<(), PluginError> {
    let (mut min, mut current, mut max) = (0u32, 0u32, 0u32);
    unsafe {
        if GetMonitorBrightness(monitor.handle.hPhysicalMonitor, &mut min, &mut current, &mut max)
            == 0
            || max <= min
        {
            return Err(PluginError::Failed(format!(
                "{} did not report its brightness range",
                monitor.name
            )));
        }
        let level = min + ((percent.clamp(0.0, 100.0) / 100.0) * f64::from(max - min)).round() as u32;
        if SetMonitorBrightness(monitor.handle.hPhysicalMonitor, level) == 0 {
            return Err(PluginError::Failed(format!(
                "{} did not accept the new brightness",
                monitor.name
            )));
        }
    }
    Ok(())
}

impl DisplayBackend for WindowsDisplay {
    fn monitors(&mut self) -> Result<Vec<String>, PluginError> {
        init();
        let mut names = Vec::new();
        if panel_brightness().is_some() {
            names.push(BUILT_IN.to_string());
        }
        names.extend(ddc_monitors().0.iter().map(|monitor| monitor.name.clone()));
        Ok(names)
    }

    fn brightness(&mut self, monitor: &str) -> Result<f64, PluginError> {
        init();
        if monitor == BUILT_IN {
            return panel_brightness().ok_or_else(|| {
                PluginError::Failed("could not read the built-in screen's brightness".into())
            });
        }
        let all = ddc_monitors();
        let found = all
            .0
            .iter()
            .find(|candidate| candidate.name == monitor)
            .ok_or_else(|| PluginError::Failed(format!("{monitor} is not connected")))?;
        ddc_brightness(found)
    }

    fn set_brightness(&mut self, monitor: &str, percent: f64) -> Result<(), PluginError> {
        init();
        if monitor == BUILT_IN {
            return set_panel_brightness(percent);
        }
        let all = ddc_monitors();
        let found = all
            .0
            .iter()
            .find(|candidate| candidate.name == monitor)
            .ok_or_else(|| PluginError::Failed(format!("{monitor} is not connected")))?;
        set_ddc_brightness(found, percent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads the real screens; run by hand on Windows"]
    fn live_lists_the_screens_and_reads_their_brightness() {
        let mut display = WindowsDisplay::new();
        let monitors = display.monitors().expect("monitors");
        eprintln!("screens: {monitors:?}");
        for monitor in monitors {
            let level = display.brightness(&monitor).expect("brightness");
            eprintln!("{monitor}: {level}%");
            assert!((0.0..=100.0).contains(&level));
        }
    }
}
