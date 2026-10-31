# silicon-throttle-rs

[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Prometheus](https://img.shields.io/badge/Prometheus-Port%209112-green.svg)](http://localhost:9112/metrics)
[![Platform](https://img.shields.io/badge/platform-Linux%20x86__64-lightgrey.svg)]()

A zero-allocation Linux CPU PROCHOT clock-stretching, silicon thermal asphyxiation, and PCIe link downclock sentinel written in pure Rust.

Interrogates CPU frequency scaling nodes (`/sys/devices/system/cpu/cpu*/cpufreq/`), hardware monitoring sensors (`/sys/class/hwmon/`), and ACPI cooling devices (`/sys/class/thermal/cooling_device*`) to expose the **"Phantom Slowdown"**—where overheated silicon slashes physical compute throughput while masking as 100% busy in standard APM tools. Exposes Prometheus metrics on port `:9112`.

```
===============================================================================
   SILICON-THROTTLE-SENTINEL: CPU Clock-Stretching & Hardware Sentinel
===============================================================================
 Effective Frequency Ratio:   92.2%  | Peak Temp:  80.3°C
 PROCHOT Clamped Cores:        0 / 16 | PCIe Throttled Ports:  2
-------------------------------------------------------------------------------
 Silicon Health Status:      [OPTIMAL: SILICON OPERATING AT NOMINAL PERFORMANCE]
-------------------------------------------------------------------------------
 [PER-CORE FREQUENCY & COMPUTE PENALTY]
 CORE   EFFECTIVE RATIO          FREQUENCY          PENALTY    GOVERNOR  
 -----------------------------------------------------------------------------
 CPU 0  [##################--]  96.7%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 1  [##################--]  96.6%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 2  [##################--]  96.8%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 3  [##################--]  96.7%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 4  [##################--]  96.9%    3.20 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 5  [##################--]  96.8%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 6  [##################--]  96.8%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 7  [##################--]  96.7%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 8  [##################--]  96.8%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 9  [##############------]  72.7%    2.40 GHz / 3.30 GHz   1.38x [WARN] performance
 CPU 10 [##################--]  96.8%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
 CPU 11 [##################--]  96.7%    3.19 GHz / 3.30 GHz   1.03x [OK] performance
-------------------------------------------------------------------------------
 [ACTIVE ACPI THERMAL COOLING THROTTLE STATES]
  cooling_device1    -> State 3/3 (PCIe_Port_Link_Speed_0000:00:02.1)
  cooling_device2    -> State 2/3 (PCIe_Port_Link_Speed_0000:00:02.2)
-------------------------------------------------------------------------------
 [PRIMARY THERMAL SENSORS]
  k10temp: 74.5°C     amdgpu: 55.0°C     nvme: 36.9°C   
===============================================================================
 Prometheus Exporter: http://0.0.0.0:9112/metrics
 Polling: 1s | PID: 80644 | Press Ctrl+C to shutdown
```

---

## The "Phantom Slowdown" of Enterprise Computing

Modern x86_64 and ARM server CPUs (AMD EPYC, Intel Xeon, Ryzen) do not fail with a crash when cooling degrades. Instead:

1. **PROCHOT (Processor Hot) Hardware Clamping**:
   When thermal paste degrades, dust clogs server chassis fans, or VRMs overheat, the bidirectional `PROCHOT#` hardware signal asserts.
   The CPU's internal power control microcontroller downclocks the chip from **3.3+ GHz down to 400 MHz–800 MHz**.
2. **The Monitoring Blindspot**:
   Traditional tools (`top`, `htop`, standard Prometheus `node_exporter`) compute CPU load by sampling tick slices from `/proc/stat`.
   **If a core is clamped at 400 MHz and processing work, `/proc/stat` reports 100% CPU utilization.**
   Operators and SREs see normal "high utilization," completely unaware that the physical core has **lost 85% of its compute bandwidth**.
3. **Cascading Cluster Failure**:
   Database queries that normally complete in 5 ms now take 60 ms. Microservice connection pools saturate, load balancer health checks timeout, and cascading failovers bring down healthy nodes.

---

## Architectural Mechanics

```
Thermal Saturation / VRM Overheat
             │
             ▼
     PROCHOT# Hardware Trip
             │
             ├─► Silicon Power Control Downclocks Core Multiplier (3.3 GHz ──► 400 MHz)
             │
             ├─► ACPI Cooling Devices Restrict PCIe Link Speeds (Gen 4 x16 ──► Gen 1 x4)
             │
             ▼
[ The Phantom Slowdown ]
   • User space sees: 100% CPU Utilization (Normal)
   • Physical Reality: Compute Penalty Factor = 8.25x SLOWER
```

`silicon-throttle-rs` directly computes:
- **Effective Frequency Ratio**: $R_{\text{eff}} = \frac{f_{\text{cur}}}{f_{\text{max}}}$
- **Compute Penalty Factor**: $\text{Penalty} = \frac{f_{\text{max}}}{f_{\text{cur}}}$

---

## Prometheus Metrics Reference (`:9112/metrics`)

| Metric Name | Type | Description |
|---|---|---|
| `silicon_cpu_avg_effective_ratio` | Gauge | Average ratio of actual frequency over nominal max across all cores (`0.0` to `1.0+`) |
| `silicon_prochot_throttled_cores_total` | Gauge | Number of CPU cores clamped down near hardware thermal minimum floor |
| `silicon_pcie_throttled_ports_total` | Gauge | Number of PCIe ports running at reduced link speed due to ACPI cooling states |
| `silicon_phantom_slowdown_active` | Gauge | `1` if CPU is experiencing severe clock stretching while masking as 100% busy |
| `silicon_max_temp_celsius` | Gauge | Peak temperature across all audited thermal silicon zones |
| `silicon_cpu_core_cur_freq_khz` | Gauge | Real-time core operating frequency in kHz |
| `silicon_cpu_core_max_freq_khz` | Gauge | Nominal maximum hardware frequency in kHz |
| `silicon_cpu_core_min_freq_khz` | Gauge | Hardware minimum thermal downclock floor in kHz |
| `silicon_cpu_core_effective_ratio` | Gauge | Core frequency divided by max nominal frequency |
| `silicon_compute_penalty_factor` | Gauge | Multiplier slowdown factor caused by thermal downclocking (e.g. `1.38x`) |
| `silicon_thermal_temp_celsius` | Gauge | Physical temperatures across package, GPU, and NVMe silicon |
| `silicon_cooling_device_cur_state` | Gauge | Current ACPI thermal cooling state (`0` = unthrottled) |

---

## Quickstart

### 1. Build from Source

Requires Rust 1.70+:

```bash
git clone https://github.com/patrick-isles/silicon-throttle-rs.git
cd silicon-throttle-rs
cargo build --release
```

The optimized binary is output to `target/release/silicon-throttle-rs` (~1.2 MB stripped).

### 2. Interactive Terminal Dashboard

```bash
./target/release/silicon-throttle-rs
```

### 3. Headless Prometheus Exporter Daemon

```bash
./target/release/silicon-throttle-rs --daemon --port 9112 --interval 0.5
```

Scrape metrics directly:
```bash
curl -s http://127.0.0.1:9112/metrics | grep silicon_
```

---

## Production Deployment (systemd)

Install the hardened systemd unit:

```bash
sudo cp target/release/silicon-throttle-rs /usr/local/bin/
sudo cp systemd/silicon-throttle.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now silicon-throttle.service
```

Inspect service health:
```bash
systemctl status silicon-throttle.service
journalctl -u silicon-throttle.service -f
```

---

## Engineering Design Principles

- **Zero Runtime Allocations in Polling Loop**: Parses raw sysfs files directly into pre-allocated memory buffers without heap thrash.
- **Pure Rust**: Direct VFS and sysfs interrogation without external C shared libraries.
- **Resilient Signal Handling**: Clean shutdown on `SIGINT` and `SIGTERM`.

---

## Author

**Patrick Isles**  
Portland, Oregon  
GitHub: [@patrick-isles](https://github.com/patrick-isles)  
Email: patrick.isles@pcc.edu
