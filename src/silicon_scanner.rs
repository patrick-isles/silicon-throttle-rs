//! Linux Silicon Frequency, PROCHOT Clock-Stretching & Thermal Cooling Scanner
//!
//! Interrogates `/sys/devices/system/cpu/cpu*/cpufreq/`, `/sys/class/hwmon/`, and
//! `/sys/class/thermal/cooling_device*` to detect silent CPU thermal throttling and PCIe downclocking.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CpuCoreFreq {
    pub cpu_id: u32,
    pub cur_freq_khz: u64,
    pub max_freq_khz: u64,
    pub min_freq_khz: u64,
    pub governor: String,
    pub effective_ratio: f64,
    pub penalty_factor: f64,
    pub is_prochot_throttled: bool,
}

#[derive(Debug, Clone)]
pub struct ThermalSensor {
    pub name: String,
    pub label: String,
    pub temp_celsius: f64,
}

#[derive(Debug, Clone)]
pub struct CoolingDevice {
    pub dev_name: String,
    pub dev_type: String,
    pub cur_state: u32,
    pub max_state: u32,
    pub is_throttled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SiliconSnapshot {
    pub cores: Vec<CpuCoreFreq>,
    pub sensors: Vec<ThermalSensor>,
    pub cooling_devices: Vec<CoolingDevice>,
    pub avg_effective_ratio: f64,
    pub throttled_cores_count: usize,
    pub pcie_throttled_ports_count: usize,
    pub max_temp_celsius: f64,
    pub is_phantom_slowdown: bool,
}

pub struct SiliconScanner;

impl SiliconScanner {
    /// Scans CPU frequency sysfs nodes, thermal sensors, and ACPI cooling devices.
    pub fn scan() -> SiliconSnapshot {
        let mut snapshot = SiliconSnapshot::default();

        // 1. Scan CPU cores under /sys/devices/system/cpu/cpu[0-9]*
        let mut cpu_id = 0u32;
        let mut total_ratio = 0.0;

        while let Some(core) = Self::read_cpu_core(cpu_id) {
            if core.is_prochot_throttled {
                snapshot.throttled_cores_count += 1;
            }
            total_ratio += core.effective_ratio;
            snapshot.cores.push(core);
            cpu_id += 1;
        }

        if !snapshot.cores.is_empty() {
            snapshot.avg_effective_ratio = total_ratio / snapshot.cores.len() as f64;
        }

        // 2. Scan hwmon thermal sensors (/sys/class/hwmon/hwmon*)
        snapshot.sensors = Self::read_thermal_sensors();
        for s in &snapshot.sensors {
            if s.temp_celsius > snapshot.max_temp_celsius {
                snapshot.max_temp_celsius = s.temp_celsius;
            }
        }

        // 3. Scan ACPI cooling devices (/sys/class/thermal/cooling_device*)
        snapshot.cooling_devices = Self::read_cooling_devices();
        for cd in &snapshot.cooling_devices {
            if cd.is_throttled && cd.dev_type.contains("PCIe") {
                snapshot.pcie_throttled_ports_count += 1;
            }
        }

        // A Phantom Slowdown occurs when CPU is clamped below 50% capacity or cores hit PROCHOT floor
        snapshot.is_phantom_slowdown = snapshot.throttled_cores_count > 0 || snapshot.avg_effective_ratio < 0.50;

        snapshot
    }

    fn read_cpu_core(cpu_id: u32) -> Option<CpuCoreFreq> {
        let cpufreq_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq", cpu_id);
        let path = Path::new(&cpufreq_path);
        if !path.exists() {
            return None;
        }

        let cur_freq_khz = Self::read_u64(&path.join("scaling_cur_freq")).unwrap_or(0);
        let max_freq_khz = Self::read_u64(&path.join("cpuinfo_max_freq")).unwrap_or(1);
        let min_freq_khz = Self::read_u64(&path.join("cpuinfo_min_freq")).unwrap_or(0);
        let governor = fs::read_to_string(path.join("scaling_governor"))
            .unwrap_or_else(|_| "unknown".to_string())
            .trim()
            .to_string();

        let effective_ratio = if max_freq_khz > 0 {
            cur_freq_khz as f64 / max_freq_khz as f64
        } else {
            1.0
        };

        let penalty_factor = if cur_freq_khz > 0 {
            max_freq_khz as f64 / cur_freq_khz as f64
        } else {
            1.0
        };

        // If core is clamped near minimum hardware floor (within 15% of min_freq), it is PROCHOT throttled
        let is_prochot_throttled = min_freq_khz > 0 && cur_freq_khz <= (min_freq_khz * 115 / 100);

        Some(CpuCoreFreq {
            cpu_id,
            cur_freq_khz,
            max_freq_khz,
            min_freq_khz,
            governor,
            effective_ratio,
            penalty_factor,
            is_prochot_throttled,
        })
    }

    fn read_thermal_sensors() -> Vec<ThermalSensor> {
        let mut sensors = Vec::new();
        let hwmon_dir = match fs::read_dir("/sys/class/hwmon") {
            Ok(d) => d,
            Err(_) => return sensors,
        };

        for entry in hwmon_dir.flatten() {
            let path = entry.path();
            let name = fs::read_to_string(path.join("name"))
                .unwrap_or_else(|_| "unknown".to_string())
                .trim()
                .to_string();

            // Look for temp1_input, temp2_input, etc.
            if let Ok(entries) = fs::read_dir(&path) {
                for file_entry in entries.flatten() {
                    let file_name = file_entry.file_name();
                    let file_str = file_name.to_string_lossy();
                    if file_str.starts_with("temp") && file_str.ends_with("_input") {
                        if let Some(milli_c) = Self::read_u64(&file_entry.path()) {
                            let temp_celsius = milli_c as f64 / 1000.0;
                            let label_file = file_str.replace("_input", "_label");
                            let label = fs::read_to_string(path.join(label_file))
                                .unwrap_or_else(|_| file_str.replace("_input", ""))
                                .trim()
                                .to_string();

                            sensors.push(ThermalSensor {
                                name: name.clone(),
                                label,
                                temp_celsius,
                            });
                        }
                    }
                }
            }
        }

        sensors
    }

    fn read_cooling_devices() -> Vec<CoolingDevice> {
        let mut devices = Vec::new();
        let thermal_dir = match fs::read_dir("/sys/class/thermal") {
            Ok(d) => d,
            Err(_) => return devices,
        };

        for entry in thermal_dir.flatten() {
            let name_str = entry.file_name().to_string_lossy().to_string();
            if name_str.starts_with("cooling_device") {
                let path = entry.path();
                let dev_type = fs::read_to_string(path.join("type"))
                    .unwrap_or_else(|_| "unknown".to_string())
                    .trim()
                    .to_string();

                let cur_state = Self::read_u32(&path.join("cur_state")).unwrap_or(0);
                let max_state = Self::read_u32(&path.join("max_state")).unwrap_or(0);
                let is_throttled = cur_state > 0;

                devices.push(CoolingDevice {
                    dev_name: name_str,
                    dev_type,
                    cur_state,
                    max_state,
                    is_throttled,
                });
            }
        }

        devices.sort_by(|a, b| a.dev_name.cmp(&b.dev_name));
        devices
    }

    fn read_u64(path: &Path) -> Option<u64> {
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn read_u32(path: &Path) -> Option<u32> {
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }
}
