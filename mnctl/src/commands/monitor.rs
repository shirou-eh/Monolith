use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use std::os::unix::process::CommandExt;
use std::process::Command;
use sysinfo::System;

#[derive(Args)]
pub struct MonitorArgs {
    #[command(subcommand)]
    command: MonitorCommand,
}

#[derive(Subcommand)]
enum MonitorCommand {
    /// System overview (CPU, RAM, disk, network, uptime)
    Status,
    /// Show top processes by resource usage
    Top {
        /// Number of processes to show
        #[arg(short, long, default_value_t = 10)]
        count: usize,
        /// Show 24h history with ASCII sparklines
        #[arg(long)]
        history: bool,
    },
    /// All services with resource usage
    Services,
    /// Network interface stats and connections
    Network,
    /// Disk usage and I/O stats
    Disk,
    /// View system logs with filters
    Logs {
        /// Filter by service name
        #[arg(long)]
        service: Option<String>,
        /// Filter by log level
        #[arg(long)]
        level: Option<String>,
        /// Show logs since timestamp
        #[arg(long)]
        since: Option<String>,
        /// Show logs until timestamp
        #[arg(long)]
        until: Option<String>,
        /// Follow log output (live tail)
        #[arg(short, long)]
        follow: bool,
    },
    /// Show active and recent alerts
    Alerts,
    /// Run a PromQL query against local Prometheus
    Metrics {
        /// PromQL query
        query: String,
    },
    /// Launch the full TUI dashboard
    Dashboard,
    /// Export a metrics snapshot to JSON or CSV (1.0.2)
    Export {
        /// Output format: json (default) or csv
        #[arg(long, default_value = "json")]
        format: String,
        /// Output file path (default: /tmp/monolith-export.<fmt>)
        #[arg(long)]
        out: Option<String>,
    },
    /// Detect anomalies by comparing against 7-day baseline (2σ threshold)
    Anomaly {
        /// Metric to check: cpu, mem, disk, net, load
        #[arg(long, default_value = "cpu")]
        metric: String,
        /// Reset the baseline data
        #[arg(long)]
        reset: bool,
    },
}

impl MonitorArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            MonitorCommand::Status => system_status(),
            MonitorCommand::Top { count, history } => monitor_top(count, history),
            MonitorCommand::Services => services_resources(),
            MonitorCommand::Network => network_stats(),
            MonitorCommand::Disk => disk_stats(),
            MonitorCommand::Logs {
                service,
                level,
                since,
                until,
                follow,
            } => system_logs(
                service.as_deref(),
                level.as_deref(),
                since.as_deref(),
                until.as_deref(),
                follow,
            ),
            MonitorCommand::Alerts => show_alerts().await,
            MonitorCommand::Metrics { query } => run_promql(&query).await,
            MonitorCommand::Dashboard => launch_dashboard(),
            MonitorCommand::Export { format, out } => export_metrics(&format, out.as_deref()),
            MonitorCommand::Anomaly { metric, reset } => monitor_anomaly(&metric, reset),
        }
    }
}

fn system_status() -> Result<()> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let os = System::long_os_version().unwrap_or_else(|| "unknown".to_string());
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());

    println!("{}", "Monolith System Status".bold().underline());
    println!();
    println!("  {} {}", "Hostname:".dimmed(), hostname.bold());
    println!("  {} {}", "OS:".dimmed(), os);
    println!("  {} {}", "Kernel:".dimmed(), kernel);
    println!();

    let cpu_count = sys.cpus().len();
    let cpu_usage: f32 = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / cpu_count as f32;
    let total_mem = sys.total_memory();
    let used_mem = sys.used_memory();
    let mem_pct = (used_mem as f64 / total_mem as f64) * 100.0;

    println!(
        "  {} {:.1}% ({} cores)",
        "CPU:".dimmed(),
        cpu_usage,
        cpu_count
    );
    println!(
        "  {} {:.1}% ({} / {} MB)",
        "RAM:".dimmed(),
        mem_pct,
        used_mem / 1024 / 1024,
        total_mem / 1024 / 1024
    );

    let total_swap = sys.total_swap();
    let used_swap = sys.used_swap();
    if total_swap > 0 {
        let swap_pct = (used_swap as f64 / total_swap as f64) * 100.0;
        println!(
            "  {} {:.1}% ({} / {} MB)",
            "Swap:".dimmed(),
            swap_pct,
            used_swap / 1024 / 1024,
            total_swap / 1024 / 1024
        );
    }
    println!();

    let load_avg = System::load_average();
    println!(
        "  {} {:.2}  {:.2}  {:.2}",
        "Load:".dimmed(),
        load_avg.one,
        load_avg.five,
        load_avg.fifteen
    );

    let uptime = System::uptime();
    let days = uptime / 86400;
    let hours = (uptime % 86400) / 3600;
    let mins = (uptime % 3600) / 60;
    println!("  {} {}d {}h {}m", "Uptime:".dimmed(), days, hours, mins);

    println!();

    for disk in sysinfo::Disks::new_with_refreshed_list().list() {
        let mount = disk.mount_point().to_string_lossy();
        let total = disk.total_space();
        let avail = disk.available_space();
        let used = total.saturating_sub(avail);
        let pct = if total > 0 {
            (used as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        let color = if pct > 90.0 {
            "red"
        } else if pct > 80.0 {
            "yellow"
        } else {
            "green"
        };
        let pct_str = format!("{pct:.1}%");
        let colored_pct = match color {
            "red" => pct_str.red(),
            "yellow" => pct_str.yellow(),
            _ => pct_str.green(),
        };
        println!(
            "  {} {:<20} {} ({} / {} GB)",
            "Disk:".dimmed(),
            mount,
            colored_pct,
            used / 1024 / 1024 / 1024,
            total / 1024 / 1024 / 1024
        );
    }

    Ok(())
}

fn system_top() -> Result<()> {
    let status = Command::new("top")
        .args(["-b", "-n", "1", "-o", "%CPU"])
        .output()
        .context("failed to run top")?;

    print!("{}", String::from_utf8_lossy(&status.stdout));
    Ok(())
}

fn services_resources() -> Result<()> {
    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--state=running",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .output()
        .context("failed to list services")?;

    println!("{}", "Running Services:".bold().underline());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(name) = parts.first() {
            println!("  {} {}", "●".green(), name);
        }
    }
    Ok(())
}

fn network_stats() -> Result<()> {
    let output = Command::new("ip")
        .args(["-brief", "-color", "addr"])
        .output()
        .context("failed to get network interfaces")?;

    println!("{}", "Network Interfaces:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    println!();

    let ss_output = Command::new("ss")
        .args(["-tuln"])
        .output()
        .context("failed to get listening sockets")?;

    println!("{}", "Listening Sockets:".bold().underline());
    print!("{}", String::from_utf8_lossy(&ss_output.stdout));
    Ok(())
}

fn disk_stats() -> Result<()> {
    let output = Command::new("df")
        .args([
            "-h",
            "--type=btrfs",
            "--type=ext4",
            "--type=xfs",
            "--type=tmpfs",
        ])
        .output()
        .context("failed to get disk usage")?;

    println!("{}", "Disk Usage:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    println!();

    let iostat = Command::new("iostat").args(["-x", "1", "1"]).output();

    if let Ok(io) = iostat {
        if io.status.success() {
            println!("{}", "Disk I/O:".bold().underline());
            print!("{}", String::from_utf8_lossy(&io.stdout));
        }
    }
    Ok(())
}

fn system_logs(
    service: Option<&str>,
    level: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
    follow: bool,
) -> Result<()> {
    let mut args = vec!["-n", "100"];

    if let Some(s) = service {
        args.push("-u");
        args.push(s);
    }
    if let Some(p) = level {
        args.push("-p");
        args.push(p);
    }
    // FIX (1.0.2): journalctl --since requires absolute timestamps or
    // strings prefixed with "-" for relative offsets.  Plain "1h" was
    // passed verbatim and journalctl silently ignored it.  We normalise
    // common relative forms ("30m", "2h", "1d") by prepending "-".
    let since_owned;
    if let Some(s) = since {
        args.push("--since");
        let looks_relative = s.ends_with('m') || s.ends_with('h') || s.ends_with('d');
        if looks_relative && !s.starts_with('-') {
            since_owned = format!("-{s}");
            args.push(&since_owned);
        } else {
            args.push(s);
        }
    }
    if let Some(u) = until {
        args.push("--until");
        args.push(u);
    }

    if follow {
        // Exec into journalctl directly so terminal state is managed
        // by journalctl — avoids leaving the shell in raw mode when
        // the user presses q to quit follow mode.
        let err = Command::new("journalctl").args(&args).arg("-f").exec();
        anyhow::bail!("failed to exec journalctl: {err}");
    }

    args.push("--no-pager");
    let output = Command::new("journalctl")
        .args(&args)
        .output()
        .context("failed to read journal")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

async fn show_alerts() -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .get("http://localhost:9090/api/v1/alerts")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await?;
            if let Some(alerts) = body["data"]["alerts"].as_array() {
                if alerts.is_empty() {
                    println!("{}", "No active alerts.".green());
                } else {
                    println!("{}", "Active Alerts:".bold().underline());
                    for alert in alerts {
                        let name = alert["labels"]["alertname"].as_str().unwrap_or("unknown");
                        let severity = alert["labels"]["severity"].as_str().unwrap_or("unknown");
                        let state = alert["state"].as_str().unwrap_or("unknown");
                        let indicator = match severity {
                            "critical" => "●".red(),
                            "warning" => "●".yellow(),
                            _ => "●".blue(),
                        };
                        println!("  {indicator} [{severity}] {name} ({state})");
                    }
                }
            }
        }
        _ => {
            println!(
                "{}",
                "Prometheus not reachable at localhost:9090. Is monitoring enabled?".yellow()
            );
        }
    }
    Ok(())
}

async fn run_promql(query: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .get("http://localhost:9090/api/v1/query")
        .query(&[("query", query)])
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await?;
            println!("{}", serde_json::to_string_pretty(&body["data"]["result"])?);
        }
        Ok(r) => {
            let body = r.text().await?;
            anyhow::bail!("Prometheus query failed: {body}");
        }
        Err(e) => {
            anyhow::bail!("could not reach Prometheus: {e}");
        }
    }
    Ok(())
}

fn launch_dashboard() -> Result<()> {
    let exe = std::env::current_exe().context("failed to determine executable path")?;
    let mntui = exe
        .parent()
        .map(|p| p.join("mntui"))
        .unwrap_or_else(|| "mntui".into());

    if mntui.exists() {
        let status = Command::new(&mntui)
            .status()
            .context("failed to launch mntui")?;
        if !status.success() {
            anyhow::bail!("mntui exited with error");
        }
    } else {
        println!(
            "{}",
            "mntui binary not found. Install with: mnpkg install monolith-tui".yellow()
        );
    }
    Ok(())
}

fn monitor_anomaly(metric: &str, reset: bool) -> Result<()> {
    let data_dir = "/var/lib/monolith/monitor/anomaly";
    std::fs::create_dir_all(data_dir)?;

    let baseline_file = format!("{data_dir}/baseline-{metric}.json");
    let history_file = format!("{data_dir}/history-{metric}.json");

    // Collect current value
    let current = match metric {
        "cpu" => {
            let stat = std::fs::read_to_string("/proc/stat")
                .context("failed to read /proc/stat")?;
            let line = stat.lines().next().unwrap_or("");
            let parts: Vec<f64> = line.split_whitespace()
                .skip(1)
                .filter_map(|s| s.parse().ok())
                .collect();
            if parts.len() >= 5 {
                let total: f64 = parts.iter().sum();
                let idle = parts[3];
                if total > 0.0 { (total - idle) / total * 100.0 } else { 0.0 }
            } else { 0.0 }
        }
        "mem" => {
            let info = std::fs::read_to_string("/proc/meminfo")
                .context("failed to read /proc/meminfo")?;
            let total_line = info.lines().find(|l| l.starts_with("MemTotal:")).unwrap_or("");
            let avail_line = info.lines().find(|l| l.starts_with("MemAvailable:")).unwrap_or("");
            let total_kb: f64 = total_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
            let avail_kb: f64 = avail_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            if total_kb > 0.0 { (1.0 - avail_kb / total_kb) * 100.0 } else { 0.0 }
        }
        "disk" => {
            let usage = std::process::Command::new("df")
                .args(["/"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            usage.lines().nth(1)
                .and_then(|l| l.split_whitespace().nth(4))
                .and_then(|s| s.trim_end_matches('%').parse().ok())
                .unwrap_or(0.0)
        }
        "net" => {
            let dev = std::fs::read_to_string("/proc/net/dev")
                .context("failed to read /proc/net/dev")?;
            // Sum bytes across all interfaces
            let mut total: f64 = 0.0;
            for line in dev.lines().skip(2) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 10 {
                    if let Ok(rx) = parts[1].parse::<f64>() {
                        total += rx;
                    }
                    if let Ok(tx) = parts[9].parse::<f64>() {
                        total += tx;
                    }
                }
            }
            total / 1_000_000.0 // MB
        }
        "load" => {
            let load = std::fs::read_to_string("/proc/loadavg")
                .context("failed to read /proc/loadavg")?;
            let parts: Vec<&str> = load.split_whitespace().collect();
            parts.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0)
        }
        _ => anyhow::bail!("unknown metric: {metric} (use: cpu, mem, disk, net, load)"),
    };

    if reset {
        std::fs::remove_file(&baseline_file).ok();
        std::fs::remove_file(&history_file).ok();
        println!("  {} Baseline for '{metric}' reset", "●".yellow());
        return Ok(());
    }

    // Append to history
    let mut history: Vec<f64> = if std::path::Path::new(&history_file).exists() {
        let content = std::fs::read_to_string(&history_file)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    };
    history.push(current);
    // Keep last 7 days of readings (sampled every 5 min = 2016 entries)
    if history.len() > 2016 {
        history.drain(0..history.len() - 2016);
    }
    std::fs::write(&history_file, serde_json::to_string(&history)?)?;

    // Build baseline from history if we have enough data
    let should_warn = if history.len() >= 20 {
        let mean: f64 = history.iter().sum::<f64>() / history.len() as f64;
        let variance: f64 = history.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / history.len() as f64;
        let std_dev = variance.sqrt();
        let threshold = mean + 2.0 * std_dev;
        current > threshold
    } else {
        false
    };

    println!("{} Anomaly detection — {metric}", "≡".blue().bold());
    println!("  Current:  {:>8.1}", current);
    if history.len() >= 20 {
        let mean: f64 = history.iter().sum::<f64>() / history.len() as f64;
        let variance: f64 = history.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / history.len() as f64;
        let std_dev = variance.sqrt();
        let threshold = mean + 2.0 * std_dev;
        println!("  Baseline: {:>8.1} (σ={:.1}, 2σ threshold={:.1})", mean, std_dev, threshold);
        if should_warn {
            println!("  {} ANOMALY — current value exceeds baseline threshold!", "⚠".red().bold());
        } else {
            println!("  {} Normal (within baseline)", "●".green());
        }
    } else {
        println!("  {} Collecting baseline... ({}/20 readings)", "●".cyan(), history.len());
    }

    // Save baseline
    #[derive(Serialize)]
    struct Baseline {
        count: usize,
        mean: f64,
        std_dev: f64,
        threshold: f64,
        last_current: f64,
        updated_at: String,
    }

    if history.len() >= 20 {
        let mean: f64 = history.iter().sum::<f64>() / history.len() as f64;
        let variance: f64 = history.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / history.len() as f64;
        let std_dev = variance.sqrt();
        let threshold = mean + 2.0 * std_dev;

        let baseline = Baseline {
            count: history.len(),
            mean,
            std_dev,
            threshold,
            last_current: current,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        std::fs::write(&baseline_file, serde_json::to_string_pretty(&baseline)?)?;
    }

    Ok(())
}

fn monitor_top(count: usize, history: bool) -> Result<()> {
    if history {
        let data_dir = "/var/lib/monolith/monitor/top-history";
        std::fs::create_dir_all(data_dir)?;

        let _all_snapshots: Vec<Vec<(String, f64)>> = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(data_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        // Keep last 24h (assuming one snapshot per minute)
        let limit = 1440;
        if entries.len() > limit {
            entries.drain(0..entries.len() - limit);
        }

        // Find top-N processes across all snapshots
        use std::collections::BTreeMap;
        let mut process_samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();

        for entry in &entries {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(snapshot) = serde_json::from_str::<Vec<(String, f64)>>(&content) {
                    for (name, cpu) in snapshot {
                        process_samples.entry(name).or_default().push(cpu);
                    }
                }
            }
        }

        if process_samples.is_empty() {
            println!("{} No history data collected yet.", "●".yellow());
            println!("  Run 'mnctl monitor top' periodically (e.g., via cron every minute)");
            return Ok(());
        }

        println!("{} Top processes — 24h history (ASCII sparklines)", "≡".blue().bold());
        println!();

        // Sort by average CPU
        let mut sorted: Vec<_> = process_samples.into_iter().collect();
        sorted.sort_by(|a, b| {
            let avg_a = a.1.iter().sum::<f64>() / a.1.len() as f64;
            let avg_b = b.1.iter().sum::<f64>() / b.1.len() as f64;
            avg_b.partial_cmp(&avg_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        for (name, samples) in sorted.iter().take(count) {
            let avg = samples.iter().sum::<f64>() / samples.len() as f64;
            let max = samples.iter().cloned().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0.0);
            let min = samples.iter().cloned().min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(0.0);

            let sparkline = sparkline(samples, 20);
            println!("  {:<20} {} {:>5.1}% avg {:>5.1}% max {:>5.1}% min",
                name.chars().take(20).collect::<String>(),
                sparkline, avg, max, min);
        }
    } else {
        let output = std::process::Command::new("ps")
            .args(["axo", "pid,%cpu,%mem,comm", "--sort=-%cpu"])
            .output()
            .context("failed to run ps")?;
        let content = String::from_utf8_lossy(&output.stdout).to_string();

        println!("{} Top {} processes", "≡".blue(), count);
        println!("  {:<8} {:<6} {:<6}  {}", "PID", "CPU%", "MEM%", "COMMAND");
        for line in content.lines().skip(1).take(count) {
            println!("  {line}");
        }
    }

    Ok(())
}

fn sparkline(values: &[f64], width: usize) -> String {
    if values.is_empty() {
        return "│ │".to_string();
    }

    let max = values.iter().cloned().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).unwrap_or(1.0);
    if max == 0.0 {
        return "│ │".to_string();
    }

    // Sparkline characters (8-height)
    let chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // Downsample to fit width
    let sampled: Vec<f64> = if values.len() > width {
        let step = values.len() / width;
        values.iter().step_by(step).copied().take(width).collect()
    } else {
        values.to_vec()
    };

    let result: String = sampled.iter().map(|v| {
        let normalized = (v / max * 7.0).floor() as usize;
        let idx = normalized.min(7);
        chars[idx]
    }).collect();

    format!("│{}│", result)
}

/// Export a snapshot of system metrics to a structured file (added 1.0.2).
///
/// Captures hostname, timestamp, per-core CPU %, RAM/Swap, disk usage,
/// load average, and active service count.  The JSON schema is stable
/// across 1.x patch releases.
fn export_metrics(format: &str, out: Option<&str>) -> Result<()> {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname = System::host_name().unwrap_or_else(|| "unknown".into());
    let timestamp = chrono::Utc::now().to_rfc3339();

    let cpu_per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    let cpu_avg = if cpu_per_core.is_empty() {
        0.0f32
    } else {
        cpu_per_core.iter().sum::<f32>() / cpu_per_core.len() as f32
    };

    let ram_used = sys.used_memory();
    let ram_total = sys.total_memory();
    let swap_used = sys.used_swap();
    let swap_total = sys.total_swap();

    let load = System::load_average();

    struct DiskEntry {
        mount: String,
        used: u64,
        total: u64,
        pct: f64,
    }
    let disks: Vec<DiskEntry> = sysinfo::Disks::new().list()
        .iter()
        .map(|d| {
            let total = d.total_space();
            let avail = d.available_space();
            let used = total.saturating_sub(avail);
            let pct = if total > 0 {
                used as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            DiskEntry {
                mount: d.mount_point().to_string_lossy().into_owned(),
                used,
                total,
                pct,
            }
        })
        .collect();

    // Count active systemd services.
    let active_services = Command::new("systemctl")
        .args(["--no-pager", "--no-legend", "-t", "service", "--state=active"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count())
        .unwrap_or(0);

    let default_path = format!("/tmp/monolith-export.{format}");
    let out_path = out.unwrap_or(&default_path);

    match format {
        "json" => {
            let disks_json: Vec<serde_json::Value> = disks
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "mount": d.mount,
                        "used_bytes": d.used,
                        "total_bytes": d.total,
                        "used_pct": (d.pct * 10.0).round() / 10.0,
                    })
                })
                .collect();

            let doc = serde_json::json!({
                "schema": "monolith-export/1",
                "hostname": hostname,
                "timestamp": timestamp,
                "cpu": {
                    "avg_pct": (cpu_avg * 10.0).round() / 10.0,
                    "per_core_pct": cpu_per_core,
                },
                "ram": {
                    "used_bytes": ram_used,
                    "total_bytes": ram_total,
                    "used_pct": (ram_used as f64 / ram_total.max(1) as f64 * 1000.0).round() / 10.0,
                },
                "swap": {
                    "used_bytes": swap_used,
                    "total_bytes": swap_total,
                },
                "load": {
                    "1min": load.one,
                    "5min": load.five,
                    "15min": load.fifteen,
                },
                "disks": disks_json,
                "active_services": active_services,
            });

            let content = serde_json::to_string_pretty(&doc)?;
            std::fs::write(out_path, &content)
                .with_context(|| format!("failed to write {out_path}"))?;
        }
        "csv" => {
            use std::fmt::Write as FmtWrite;
            let mut csv = String::new();
            writeln!(
                &mut csv,
                "hostname,timestamp,cpu_avg_pct,ram_used_bytes,ram_total_bytes,\
                 swap_used_bytes,swap_total_bytes,load_1m,load_5m,load_15m,active_services"
            )?;
            writeln!(
                &mut csv,
                "{hostname},{timestamp},{:.1},{ram_used},{ram_total},{swap_used},{swap_total},{:.2},{:.2},{:.2},{active_services}",
                cpu_avg, load.one, load.five, load.fifteen
            )?;
            writeln!(&mut csv)?;
            writeln!(&mut csv, "mount,used_bytes,total_bytes,used_pct")?;
            for d in &disks {
                writeln!(&mut csv, "{},{},{},{:.1}", d.mount, d.used, d.total, d.pct)?;
            }
            std::fs::write(out_path, &csv)
                .with_context(|| format!("failed to write {out_path}"))?;
        }
        other => anyhow::bail!("unknown format '{other}'. Use 'json' or 'csv'"),
    }

    println!("{} metrics exported to {out_path}", "●".green());
    Ok(())
}
