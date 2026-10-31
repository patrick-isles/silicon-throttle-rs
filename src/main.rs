//! Linux CPU PROCHOT Clock-Stretching & Silicon Thermal Sentinel
//!
//! Interrogates CPU frequency scaling nodes, thermal zones, and ACPI cooling devices
//! to detect silent hardware clock stretching and compute throughput penalties.

mod exporter;
mod silicon_scanner;

use exporter::SiliconExporter;
use silicon_scanner::SiliconScanner;

use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

fn format_freq(khz: u64) -> String {
    if khz >= 1_000_000 {
        format!("{:.2} GHz", khz as f64 / 1_000_000.0)
    } else {
        format!("{:.0} MHz", khz as f64 / 1_000.0)
    }
}

fn render_bar(ratio: f64, width: usize) -> String {
    let clamped = ratio.clamp(0.0, 1.0);
    let filled = (clamped * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let mut bar = String::with_capacity(width + 2);
    bar.push('[');
    for _ in 0..filled {
        bar.push('#');
    }
    for _ in 0..empty {
        bar.push('-');
    }
    bar.push(']');
    bar
}

fn print_usage(prog: &str) {
    println!(
        r#"{} - Linux Silicon Clock-Stretching & PROCHOT Thermal Sentinel

USAGE:
    {} [OPTIONS]

OPTIONS:
    --port <PORT>        Prometheus HTTP exporter port (default: 9112)
    --interval <SECS>    Metrics refresh interval in seconds (default: 1.0)
    --daemon             Run in headless daemon mode (no terminal TUI)
    -h, --help           Show this help message

PROMETHEUS ENDPOINT:
    http://0.0.0.0:9112/metrics
"#,
        prog, prog
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut port: u16 = 9112;
    let mut interval_secs: f64 = 1.0;
    let mut daemon_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                if i + 1 < args.len() {
                    port = args[i + 1].parse().unwrap_or(9112);
                    i += 1;
                }
            }
            "--interval" => {
                if i + 1 < args.len() {
                    interval_secs = args[i + 1].parse().unwrap_or(1.0);
                    i += 1;
                }
            }
            "--daemon" => {
                daemon_mode = true;
            }
            "-h" | "--help" => {
                print_usage(&args[0]);
                return;
            }
            _ => {}
        }
        i += 1;
    }

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    let initial_snapshot = SiliconScanner::scan();
    let snapshot = Arc::new(RwLock::new(initial_snapshot));

    // Launch Prometheus HTTP server on configured port
    SiliconExporter::start_exporter(snapshot.clone(), port);

    println!(
        "[silicon-throttle-rs] Initialized on 0.0.0.0:{}/metrics (PID: {})",
        port,
        std::process::id()
    );

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs_f64(interval_secs));

        let current = SiliconScanner::scan();
        {
            let mut snap = snapshot.write().unwrap();
            *snap = current.clone();
        }

        if !daemon_mode {
            // ANSI terminal refresh
            print!("\x1B[2J\x1B[1;1H");
            println!("===============================================================================");
            println!("   SILICON-THROTTLE-SENTINEL: CPU Clock-Stretching & Hardware Sentinel");
            println!("===============================================================================");
            println!(
                " Effective Frequency Ratio:  {:>5.1}%  | Peak Temp: {:>5.1}°C",
                current.avg_effective_ratio * 100.0,
                current.max_temp_celsius
            );
            println!(
                " PROCHOT Clamped Cores:      {:>2} / {:>2} | PCIe Throttled Ports: {:>2}",
                current.throttled_cores_count,
                current.cores.len(),
                current.pcie_throttled_ports_count
            );
            println!("-------------------------------------------------------------------------------");

            let status_badge = if current.is_phantom_slowdown {
                "[CRITICAL: PHANTOM SLOWDOWN - HARDWARE CLOCK-STRETCHING ACTIVE]"
            } else {
                "[OPTIMAL: SILICON OPERATING AT NOMINAL PERFORMANCE]"
            };
            println!(" Silicon Health Status:     {}", status_badge);
            println!("-------------------------------------------------------------------------------");
            println!(" [PER-CORE FREQUENCY & COMPUTE PENALTY]");
            println!(
                " {:<6} {:<24} {:<18} {:<10} {:<10}",
                "CORE", "EFFECTIVE RATIO", "FREQUENCY", "PENALTY", "GOVERNOR"
            );
            println!(" -----------------------------------------------------------------------------");

            for c in current.cores.iter().take(12) {
                let bar = render_bar(c.effective_ratio, 20);
                let penalty_str = if c.penalty_factor > 1.25 {
                    format!("{:.2}x [WARN]", c.penalty_factor)
                } else {
                    format!("{:.2}x [OK]", c.penalty_factor)
                };

                println!(
                    " CPU {:<2} {} {:>5.1}%   {:>9} / {:<9} {:<10} {:<10}",
                    c.cpu_id,
                    bar,
                    c.effective_ratio * 100.0,
                    format_freq(c.cur_freq_khz),
                    format_freq(c.max_freq_khz),
                    penalty_str,
                    c.governor
                );
            }

            println!("-------------------------------------------------------------------------------");
            println!(" [ACTIVE ACPI THERMAL COOLING THROTTLE STATES]");
            let throttled_devs: Vec<_> = current.cooling_devices.iter().filter(|d| d.is_throttled).collect();
            if throttled_devs.is_empty() {
                println!("  (No active PCIe or Processor ACPI cooling state throttles)");
            } else {
                for d in throttled_devs.iter().take(5) {
                    println!(
                        "  {:<18} -> State {}/{} ({})",
                        d.dev_name, d.cur_state, d.max_state, d.dev_type
                    );
                }
            }

            println!("-------------------------------------------------------------------------------");
            println!(" [PRIMARY THERMAL SENSORS]");
            for s in current.sensors.iter().take(4) {
                print!("  {}: {:.1}°C   ", s.name, s.temp_celsius);
            }
            println!();

            println!("===============================================================================");
            println!(" Prometheus Exporter: http://0.0.0.0:{}/metrics", port);
            println!(
                " Polling: {}s | PID: {} | Press Ctrl+C to shutdown",
                interval_secs,
                std::process::id()
            );
        } else if current.is_phantom_slowdown {
            eprintln!(
                "[CRITICAL ALERT] Silicon Phantom Slowdown active! Clamped cores: {}, Avg ratio: {:.1}%",
                current.throttled_cores_count,
                current.avg_effective_ratio * 100.0
            );
        }
    }

    println!("\n[silicon-throttle-rs] Received interrupt signal. Shutting down cleanly.");
}
