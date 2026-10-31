//! Prometheus Metrics Exporter on :9112/metrics
//!
//! Exposes CPU core clock-stretching ratios, compute penalty factors,
//! PROCHOT thermal downclocking, and PCIe link speed throttling states.

use crate::silicon_scanner::SiliconSnapshot;
use std::sync::{Arc, RwLock};
use std::thread;
use tiny_http::{Response, Server};

pub struct SiliconExporter;

impl SiliconExporter {
    pub fn start_exporter(snapshot: Arc<RwLock<SiliconSnapshot>>, port: u16) {
        thread::spawn(move || {
            let addr = format!("0.0.0.0:{}", port);
            let server = match Server::http(&addr) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Prometheus HTTP server failed to bind to {}: {}", addr, e);
                    return;
                }
            };

            for request in server.incoming_requests() {
                if request.url() == "/metrics" || request.url() == "/" {
                    let snap = snapshot.read().unwrap().clone();

                    let mut body = String::with_capacity(8192);

                    body.push_str("# HELP silicon_cpu_avg_effective_ratio Average ratio of current clock frequency over nominal maximum across all cores\n");
                    body.push_str("# TYPE silicon_cpu_avg_effective_ratio gauge\n");
                    body.push_str(&format!("silicon_cpu_avg_effective_ratio {:.4}\n", snap.avg_effective_ratio));

                    body.push_str("# HELP silicon_prochot_throttled_cores_total Number of CPU cores clamped down near hardware thermal floor\n");
                    body.push_str("# TYPE silicon_prochot_throttled_cores_total gauge\n");
                    body.push_str(&format!("silicon_prochot_throttled_cores_total {}\n", snap.throttled_cores_count));

                    body.push_str("# HELP silicon_pcie_throttled_ports_total Number of PCIe ports running at reduced link speed due to ACPI thermal states\n");
                    body.push_str("# TYPE silicon_pcie_throttled_ports_total gauge\n");
                    body.push_str(&format!("silicon_pcie_throttled_ports_total {}\n", snap.pcie_throttled_ports_count));

                    body.push_str("# HELP silicon_phantom_slowdown_active 1 if CPU is experiencing severe clock stretching while masking as 100% busy\n");
                    body.push_str("# TYPE silicon_phantom_slowdown_active gauge\n");
                    body.push_str(&format!("silicon_phantom_slowdown_active {}\n", if snap.is_phantom_slowdown { 1 } else { 0 }));

                    body.push_str("# HELP silicon_max_temp_celsius Peak temperature across all audited thermal silicon zones\n");
                    body.push_str("# TYPE silicon_max_temp_celsius gauge\n");
                    body.push_str(&format!("silicon_max_temp_celsius {:.2}\n", snap.max_temp_celsius));

                    // Per-core frequency metrics
                    body.push_str("# HELP silicon_cpu_core_cur_freq_khz Current real-time operating frequency in kHz\n");
                    body.push_str("# TYPE silicon_cpu_core_cur_freq_khz gauge\n");
                    for c in &snap.cores {
                        body.push_str(&format!(
                            "silicon_cpu_core_cur_freq_khz{{cpu=\"{}\",governor=\"{}\"}} {}\n",
                            c.cpu_id, c.governor, c.cur_freq_khz
                        ));
                        body.push_str(&format!(
                            "silicon_cpu_core_max_freq_khz{{cpu=\"{}\"}} {}\n",
                            c.cpu_id, c.max_freq_khz
                        ));
                        body.push_str(&format!(
                            "silicon_cpu_core_min_freq_khz{{cpu=\"{}\"}} {}\n",
                            c.cpu_id, c.min_freq_khz
                        ));
                    }

                    body.push_str("# HELP silicon_cpu_core_effective_ratio Current frequency divided by max nominal frequency (0.0 to 1.0+)\n");
                    body.push_str("# TYPE silicon_cpu_core_effective_ratio gauge\n");
                    for c in &snap.cores {
                        body.push_str(&format!(
                            "silicon_cpu_core_effective_ratio{{cpu=\"{}\"}} {:.4}\n",
                            c.cpu_id, c.effective_ratio
                        ));
                    }

                    body.push_str("# HELP silicon_compute_penalty_factor Multiplier slowdown penalty factor due to thermal downclocking\n");
                    body.push_str("# TYPE silicon_compute_penalty_factor gauge\n");
                    for c in &snap.cores {
                        body.push_str(&format!(
                            "silicon_compute_penalty_factor{{cpu=\"{}\"}} {:.2}\n",
                            c.cpu_id, c.penalty_factor
                        ));
                    }

                    // Thermal Sensors
                    body.push_str("# HELP silicon_thermal_temp_celsius Physical temperature in degrees Celsius\n");
                    body.push_str("# TYPE silicon_thermal_temp_celsius gauge\n");
                    for s in &snap.sensors {
                        let clean_name = s.name.replace('"', "\\\"");
                        let clean_label = s.label.replace('"', "\\\"");
                        body.push_str(&format!(
                            "silicon_thermal_temp_celsius{{sensor=\"{}\",label=\"{}\"}} {:.2}\n",
                            clean_name, clean_label, s.temp_celsius
                        ));
                    }

                    // ACPI Cooling Devices
                    body.push_str("# HELP silicon_cooling_device_cur_state Current thermal cooling throttle state (0 = unthrottled)\n");
                    body.push_str("# TYPE silicon_cooling_device_cur_state gauge\n");
                    for cd in &snap.cooling_devices {
                        let clean_type = cd.dev_type.replace('"', "\\\"");
                        body.push_str(&format!(
                            "silicon_cooling_device_cur_state{{device=\"{}\",type=\"{}\"}} {}\n",
                            cd.dev_name, clean_type, cd.cur_state
                        ));
                    }

                    let response = Response::from_string(body)
                        .with_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/plain; version=0.0.4"[..]).unwrap());
                    let _ = request.respond(response);
                } else {
                    let response = Response::from_string("Not Found").with_status_code(404);
                    let _ = request.respond(response);
                }
            }
        });
    }
}
