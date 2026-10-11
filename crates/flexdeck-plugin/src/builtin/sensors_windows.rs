//! Windows sensors: memory, disk, network, cores, GPU, battery, uptime.
//!
//! Cores and the GPU come from performance counters, which need two
//! collections a moment apart before they mean anything; the query is kept
//! open between readings so every reading after the first is a rate over the
//! time since the last. Network rates are worked out the same way from the
//! interfaces' byte counts.

use std::time::Instant;

use windows::core::PCWSTR;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX};

use super::sensors::{Readings, SensorBackend};
use crate::PluginError;

const GB: f64 = 1024.0 * 1024.0 * 1024.0;
/// IF_TYPE_SOFTWARE_LOOPBACK: not traffic anyone sees.
const LOOPBACK: u32 = 24;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug)]
pub struct WindowsSensors {
    query: Option<PDH_HQUERY>,
    cores: Option<PDH_HCOUNTER>,
    gpu: Option<PDH_HCOUNTER>,
    /// Bytes in and out, and when, at the last reading.
    network: Option<(u64, u64, Instant)>,
}

// The handles are only used from the thread holding the plugin's lock.
unsafe impl Send for WindowsSensors {}

impl WindowsSensors {
    pub fn new() -> Self {
        let mut sensors = Self { query: None, cores: None, gpu: None, network: None };
        sensors.open();
        sensors
    }

    /// Open the counter query; on a machine without some counter (no GPU
    /// counters on older Windows), that counter is simply absent.
    fn open(&mut self) {
        let mut query = PDH_HQUERY::default();
        // SAFETY: plain FFI with valid out-pointers.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return;
        }
        let add = |path: &str| {
            let path = wide(path);
            let mut counter = PDH_HCOUNTER::default();
            // SAFETY: `path` outlives the call; `counter` is a valid out-pointer.
            (unsafe { PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) } == 0).then_some(counter)
        };
        self.cores = add("\\Processor(*)\\% Processor Time");
        self.gpu = add("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
        // SAFETY: the query was opened above. The first collection only primes it.
        unsafe { PdhCollectQueryData(query) };
        self.query = Some(query);
    }

    /// Every instance of `counter` as (name, value).
    fn instances(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        let mut size = 0u32;
        let mut count = 0u32;
        // SAFETY: the first call asks only for the size.
        let first = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) };
        if first != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }
        let items = (size as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
        let mut buffer = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); items + 1];
        // SAFETY: the buffer is at least `size` bytes.
        let status = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buffer.as_mut_ptr())) };
        if status != 0 {
            return Vec::new();
        }
        buffer
            .iter()
            .take(count as usize)
            .map(|item| {
                // SAFETY: the name points into `buffer`, filled by PDH.
                let name = unsafe { item.szName.to_string() }.unwrap_or_default();
                // SAFETY: PDH_FMT_DOUBLE was asked for, so the union holds a double.
                let value = unsafe { item.FmtValue.Anonymous.doubleValue };
                (name, value)
            })
            .collect()
    }
}

impl Default for WindowsSensors {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorBackend for WindowsSensors {
    fn read(&mut self) -> Result<Readings, PluginError> {
        let mut readings = Readings::default();

        let mut memory = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        // SAFETY: `dwLength` is set as the call requires.
        if unsafe { GlobalMemoryStatusEx(&mut memory) }.is_ok() {
            let total = memory.ullTotalPhys as f64;
            let used = total - memory.ullAvailPhys as f64;
            readings.memory_total_gb = Some(total / GB);
            readings.memory_used_gb = Some(used / GB);
            readings.memory_used_percent = Some(f64::from(memory.dwMemoryLoad));
        }

        let drive = wide("C:\\");
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: valid out-pointers; the path outlives the call.
        if unsafe { GetDiskFreeSpaceExW(PCWSTR(drive.as_ptr()), None, Some(&mut total), Some(&mut free)) }.is_ok() && total > 0 {
            readings.disk_free_gb = Some(free as f64 / GB);
            readings.disk_used_percent = Some((total - free) as f64 / total as f64 * 100.0);
        }

        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        // SAFETY: GetIfTable2 allocates the table; FreeMibTable releases it below.
        if unsafe { GetIfTable2(&mut table) }.is_ok() && !table.is_null() {
            let (mut down, mut up) = (0u64, 0u64);
            // SAFETY: the table holds `NumEntries` rows.
            let rows = unsafe { std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize) };
            for row in rows {
                // Real interfaces only: not the loopback, not filters, and up.
                let hardware = row.InterfaceAndOperStatusFlags._bitfield & 1 == 1;
                if row.Type != LOOPBACK && hardware && row.OperStatus == IfOperStatusUp {
                    down += row.InOctets;
                    up += row.OutOctets;
                }
            }
            // SAFETY: the table came from GetIfTable2.
            unsafe { FreeMibTable(table as *const _) };
            let now = Instant::now();
            if let Some((was_down, was_up, then)) = self.network {
                let seconds = now.duration_since(then).as_secs_f64();
                if seconds > 0.0 {
                    readings.network_down = Some(down.saturating_sub(was_down) as f64 / seconds);
                    readings.network_up = Some(up.saturating_sub(was_up) as f64 / seconds);
                }
            }
            self.network = Some((down, up, now));
        }

        if let Some(query) = self.query {
            // SAFETY: the query is open.
            if unsafe { PdhCollectQueryData(query) } == 0 {
                if let Some(cores) = self.cores {
                    let mut per_core: Vec<(usize, f64)> = Self::instances(cores)
                        .into_iter()
                        .filter_map(|(name, value)| name.parse::<usize>().ok().map(|n| (n, value.clamp(0.0, 100.0))))
                        .collect();
                    per_core.sort_by_key(|(n, _)| *n);
                    readings.cores = per_core.into_iter().map(|(_, value)| value).collect();
                }
                if let Some(gpu) = self.gpu {
                    let total: f64 = Self::instances(gpu).into_iter().map(|(_, value)| value).sum();
                    readings.gpu_percent = Some(total.clamp(0.0, 100.0));
                }
            }
        }

        let mut power = SYSTEM_POWER_STATUS::default();
        // SAFETY: a valid out-pointer.
        if unsafe { GetSystemPowerStatus(&mut power) }.is_ok() {
            // 128: no battery at all; 255: unknown.
            if power.BatteryFlag & 128 == 0 && power.BatteryLifePercent <= 100 {
                readings.battery_percent = Some(f64::from(power.BatteryLifePercent));
                readings.battery_charging = Some(power.ACLineStatus == 1);
            }
        }

        // SAFETY: no arguments.
        readings.uptime_seconds = Some(unsafe { GetTickCount64() } as f64 / 1000.0);
        Ok(readings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads this machine. Ignored by default: run with `--ignored` on Windows.
    #[test]
    #[ignore]
    fn live_readings_are_plausible() {
        let mut sensors = WindowsSensors::new();
        let _ = sensors.read().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let readings = sensors.read().unwrap();
        eprintln!("{readings:#?}");
        let memory = readings.memory_used_percent.expect("memory");
        assert!((0.0..=100.0).contains(&memory));
        assert!(readings.memory_total_gb.unwrap() > 0.5);
        assert!(!readings.cores.is_empty(), "per-core counters");
        assert!(readings.cores.iter().all(|c| (0.0..=100.0).contains(c)));
        assert!(readings.network_down.is_some(), "a rate after two readings");
        assert!(readings.uptime_seconds.unwrap() > 0.0);
    }
}
