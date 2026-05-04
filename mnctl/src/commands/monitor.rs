use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
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
    /// Real-time resource usage
    Top,
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
}

impl MonitorArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            MonitorCommand::Status => system_status(),
            MonitorCommand::Top => system_top(),
            MonitorCommand::Services => services_resources(),
            MonitorCommand::Network => network_stats(),
            MonitorCommand::Disk => disk_stats(),
            MonitorCommand::Logs {
                service,
                level,
                since,
                until,
            } => system_logs(
                service.as_deref(),
                level.as_deref(),
                since.as_deref(),
                until.as_deref(),
            ),
            MonitorCommand::Alerts => show_alerts().await,
            MonitorCommand::Metrics { query } => run_promql(&query).await,
            MonitorCommand::Dashboard => launch_dashboard(),
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
) -> Result<()> {
    let mut args = vec!["--no-pager", "-n", "100"];

    if let Some(s) = service {
        args.push("-u");
        args.push(s);
    }
    if let Some(p) = level {
        args.push("-p");
        args.push(p);
    }
    if let Some(s) = since {
        args.push("--since");
        args.push(s);
    }
    if let Some(u) = until {
        args.push("--until");
        args.push(u);
    }

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
