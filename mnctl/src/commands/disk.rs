use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct DiskArgs {
    #[command(subcommand)]
    command: DiskCommand,
}

#[derive(Subcommand)]
enum DiskCommand {
    /// List all block devices
    List,
    /// Show usage and free space across mounted filesystems
    Usage,
    /// Show disk I/O statistics (iostat)
    Io,
    /// Run SMART self-test or query SMART status
    Smart {
        #[command(subcommand)]
        action: SmartAction,
    },
    /// Show NVMe-specific health information
    Nvme {
        /// Device (e.g. /dev/nvme0)
        device: Option<String>,
    },
}

#[derive(Subcommand)]
enum SmartAction {
    /// Print SMART health summary for one or all disks
    Status {
        /// Device path (default: all detected disks)
        device: Option<String>,
    },
    /// Show all SMART attributes (vendor + standard) for a disk
    Attributes {
        /// Device path
        device: String,
    },
    /// Run a self-test on the disk
    Test {
        /// Device path
        device: String,
        /// Self-test type
        #[arg(long, default_value = "short")]
        kind: String,
    },
    /// Show recent SMART self-test log
    Log {
        /// Device path
        device: String,
    },
    /// Watch SMART status across all disks (one-shot summary)
    Watch,
}

impl DiskArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            DiskCommand::List => disk_list(),
            DiskCommand::Usage => disk_usage(),
            DiskCommand::Io => disk_io(),
            DiskCommand::Smart { action } => match action {
                SmartAction::Status { device } => smart_status(device.as_deref()),
                SmartAction::Attributes { device } => smart_attributes(&device),
                SmartAction::Test { device, kind } => smart_test(&device, &kind),
                SmartAction::Log { device } => smart_log(&device),
                SmartAction::Watch => smart_watch(),
            },
            DiskCommand::Nvme { device } => nvme_health(device.as_deref()),
        }
    }
}

fn require_smartctl() -> Result<()> {
    if which::which("smartctl").is_err() {
        anyhow::bail!(
            "smartctl not found. Install smartmontools: pacman -S smartmontools (Arch/Monolith)"
        );
    }
    Ok(())
}

fn detect_disks() -> Vec<String> {
    let mut disks = Vec::new();
    if let Ok(output) = Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,TYPE"])
        .output()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split_whitespace();
            let name = parts.next().unwrap_or("");
            let kind = parts.next().unwrap_or("");
            if kind == "disk" && !name.is_empty() {
                disks.push(format!("/dev/{name}"));
            }
        }
    }
    disks
}

fn disk_list() -> Result<()> {
    let output = Command::new("lsblk")
        .args(["-o", "NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE,MODEL,SERIAL"])
        .output()
        .context("failed to run lsblk")?;

    println!("{}", "Block Devices:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn disk_usage() -> Result<()> {
    let output = Command::new("df")
        .args(["-h", "--output=source,fstype,size,used,avail,pcent,target"])
        .output()
        .context("failed to run df")?;

    println!("{}", "Filesystem Usage:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn disk_io() -> Result<()> {
    if which::which("iostat").is_err() {
        anyhow::bail!("iostat not found. Install sysstat: pacman -S sysstat");
    }

    let output = Command::new("iostat")
        .args(["-x", "-d", "1", "1"])
        .output()
        .context("failed to run iostat")?;

    println!("{}", "Disk I/O:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn smart_status(device: Option<&str>) -> Result<()> {
    require_smartctl()?;

    let devices: Vec<String> = match device {
        Some(d) => vec![d.to_string()],
        None => detect_disks(),
    };

    if devices.is_empty() {
        println!("{}", "No disks detected.".yellow());
        return Ok(());
    }

    println!("{}", "SMART Health Summary:".bold().underline());
    for dev in devices {
        match smart_health_one(&dev) {
            Ok(report) => {
                let symbol = match report.status.as_str() {
                    "PASSED" => "●".green(),
                    "FAILED" => "●".red(),
                    _ => "●".yellow(),
                };
                let temp_str = report
                    .temperature_celsius
                    .map(|t| format!("{t}°C"))
                    .unwrap_or_else(|| "—".to_string());
                println!(
                    "  {} {:<14} model={:<24} health={:<7} temp={:<6} hours={}",
                    symbol,
                    dev,
                    truncate(&report.model, 24),
                    report.status,
                    temp_str,
                    report.power_on_hours.unwrap_or(0)
                );
            }
            Err(err) => {
                println!("  {} {:<14} error: {}", "●".yellow(), dev, err);
            }
        }
    }
    Ok(())
}

fn smart_attributes(device: &str) -> Result<()> {
    require_smartctl()?;
    let output = Command::new("smartctl")
        .args(["-A", device])
        .output()
        .context("failed to run smartctl -A")?;
    println!(
        "{}",
        format!("SMART Attributes ({device}):").bold().underline()
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn smart_test(device: &str, kind: &str) -> Result<()> {
    require_smartctl()?;
    let kind_arg = match kind {
        "short" | "long" | "conveyance" | "offline" => kind,
        other => {
            anyhow::bail!("unknown self-test kind: {other} (valid: short|long|conveyance|offline)")
        }
    };

    println!(
        "{} Starting {} self-test on {}...",
        "→".blue(),
        kind_arg.bold(),
        device.bold()
    );
    let status = Command::new("smartctl")
        .args(["-t", kind_arg, device])
        .status()
        .context("failed to start SMART self-test")?;
    if !status.success() {
        anyhow::bail!("smartctl self-test exited with non-zero status");
    }
    println!(
        "{} Self-test scheduled. Check progress with: {} disk smart log {}",
        "●".green(),
        "mnctl".bold(),
        device
    );
    Ok(())
}

fn smart_log(device: &str) -> Result<()> {
    require_smartctl()?;
    let output = Command::new("smartctl")
        .args(["-l", "selftest", device])
        .output()
        .context("failed to read SMART selftest log")?;
    println!(
        "{}",
        format!("SMART Self-test Log ({device}):")
            .bold()
            .underline()
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn smart_watch() -> Result<()> {
    require_smartctl()?;
    let devices = detect_disks();

    println!("{}", "SMART Watch:".bold().underline());
    for dev in &devices {
        match smart_health_one(dev) {
            Ok(report) => {
                let symbol = match report.status.as_str() {
                    "PASSED" => "●".green(),
                    "FAILED" => "●".red(),
                    _ => "●".yellow(),
                };
                let temp_warn = report.temperature_celsius.is_some_and(|t| t >= 60);
                let temp_str = match report.temperature_celsius {
                    Some(t) if temp_warn => format!("{t}°C").red().to_string(),
                    Some(t) if t >= 50 => format!("{t}°C").yellow().to_string(),
                    Some(t) => format!("{t}°C").green().to_string(),
                    None => "—".to_string(),
                };

                let realloc_warn = report.reallocated_sectors.is_some_and(|n| n > 0);
                let realloc_str = match report.reallocated_sectors {
                    Some(n) if n > 0 => format!("realloc={n}").red().to_string(),
                    Some(n) => format!("realloc={n}").green().to_string(),
                    None => String::new(),
                };

                println!(
                    "  {} {:<14} {:<7} {} {} hours={}",
                    symbol,
                    dev,
                    report.status,
                    temp_str,
                    realloc_str,
                    report.power_on_hours.unwrap_or(0)
                );
                if report.status != "PASSED" || temp_warn || realloc_warn {
                    println!(
                        "    {}",
                        "⚠ requires attention — review attributes".yellow()
                    );
                }
            }
            Err(err) => {
                println!("  {} {:<14} error: {}", "●".yellow(), dev, err);
            }
        }
    }
    Ok(())
}

fn nvme_health(device: Option<&str>) -> Result<()> {
    if which::which("nvme").is_err() {
        anyhow::bail!("nvme CLI not found. Install nvme-cli: pacman -S nvme-cli");
    }

    let devices: Vec<String> = match device {
        Some(d) => vec![d.to_string()],
        None => detect_disks()
            .into_iter()
            .filter(|d| d.contains("nvme"))
            .collect(),
    };

    if devices.is_empty() {
        println!("{}", "No NVMe devices detected.".yellow());
        return Ok(());
    }

    for dev in devices {
        // detect_disks() returns whole-disk devices (lsblk -d), so for a
        // user-supplied namespace device like /dev/nvme0n1 we keep it as-is —
        // that's what `nvme smart-log` expects. Only strip trailing partition
        // suffixes like `p1` when the user passes a partition path
        // (/dev/nvme0n1p1 → /dev/nvme0n1).
        let device_root: String = if let Some(stem) = strip_nvme_partition_suffix(&dev) {
            stem
        } else {
            dev.clone()
        };
        println!(
            "{}",
            format!("NVMe Health ({device_root}):").bold().underline()
        );
        let output = Command::new("nvme")
            .args(["smart-log", device_root.as_str()])
            .output()
            .context("failed to run nvme smart-log")?;
        if output.status.success() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        } else {
            print!("{}", String::from_utf8_lossy(&output.stderr));
        }
        println!();
    }
    Ok(())
}

#[derive(Debug, Default)]
struct SmartReport {
    model: String,
    status: String,
    temperature_celsius: Option<u32>,
    power_on_hours: Option<u64>,
    reallocated_sectors: Option<u64>,
}

fn smart_health_one(device: &str) -> Result<SmartReport> {
    let mut report = SmartReport {
        model: "unknown".to_string(),
        status: "UNKNOWN".to_string(),
        ..SmartReport::default()
    };

    let output = Command::new("smartctl")
        .args(["-H", "-A", "-i", device])
        .output()
        .context("failed to run smartctl")?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Device Model:") {
            report.model = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("Model Number:") {
            report.model = rest.trim().to_string();
        } else if trimmed.starts_with("SMART overall-health self-assessment test result:") {
            if let Some(value) = trimmed.split(':').nth(1) {
                report.status = value.trim().to_string();
            }
        } else if trimmed.contains("SMART Health Status:") {
            if let Some(value) = trimmed.split(':').nth(1) {
                let raw = value.trim();
                report.status = if raw == "OK" {
                    "PASSED".to_string()
                } else {
                    raw.to_string()
                };
            }
        }

        // Standard attribute lines look like:
        //   ID# ATTRIBUTE_NAME ... RAW_VALUE
        // We parse a few well-known IDs.
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 10 {
            if let Ok(id) = parts[0].parse::<u32>() {
                let raw = parts.last().copied().unwrap_or("0");
                let raw_first = raw
                    .split_whitespace()
                    .next()
                    .unwrap_or(raw)
                    .trim_end_matches('h');
                match id {
                    9 => {
                        if let Ok(v) = raw_first.parse::<u64>() {
                            report.power_on_hours = Some(v);
                        }
                    }
                    194 | 190 => {
                        if let Ok(v) = raw_first.parse::<u32>() {
                            report.temperature_celsius = Some(v);
                        }
                    }
                    5 => {
                        if let Ok(v) = raw_first.parse::<u64>() {
                            report.reallocated_sectors = Some(v);
                        }
                    }
                    _ => {}
                }
            }
        }

        // NVMe lines have different format.
        if let Some(rest) = trimmed.strip_prefix("Temperature:") {
            if report.temperature_celsius.is_none() {
                if let Some(num) = rest.split_whitespace().find_map(|t| t.parse::<u32>().ok()) {
                    report.temperature_celsius = Some(num);
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("Power On Hours:") {
            if report.power_on_hours.is_none() {
                let cleaned: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(v) = cleaned.parse::<u64>() {
                    report.power_on_hours = Some(v);
                }
            }
        }
    }

    if report.status == "UNKNOWN" {
        // Fall back to reading exit code semantics: smartctl uses bitmask exit codes.
        // We don't fail loudly — just leave UNKNOWN for callers to surface.
    }

    Ok(report)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// If `dev` looks like an NVMe partition (e.g. `/dev/nvme0n1p2`), return the
/// namespace device path (e.g. `/dev/nvme0n1`). Otherwise return None so
/// callers can keep the original path.
fn strip_nvme_partition_suffix(dev: &str) -> Option<String> {
    if !dev.contains("nvme") {
        return None;
    }
    // Walk back from the end: digits, then the literal `p`, then a digit
    // (which marks the namespace number we want to keep).
    let bytes = dev.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_digit() {
        end -= 1;
    }
    // After stripping trailing digits, we expect a `p` immediately preceded by
    // another digit (the namespace), otherwise this isn't a partition path.
    if end >= 2 && bytes[end - 1] == b'p' && bytes[end - 2].is_ascii_digit() {
        Some(dev[..end - 1].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvme_namespace_device_kept_as_is() {
        // /dev/nvme0n1 is already a whole-disk namespace device — the smartctl
        // / nvme CLI accepts it directly. The bug we're guarding against
        // turned this into "/dev/nvme0n", which doesn't exist.
        assert_eq!(strip_nvme_partition_suffix("/dev/nvme0n1"), None);
        assert_eq!(strip_nvme_partition_suffix("/dev/nvme1n42"), None);
    }

    #[test]
    fn nvme_partition_paths_are_stripped() {
        assert_eq!(
            strip_nvme_partition_suffix("/dev/nvme0n1p1").as_deref(),
            Some("/dev/nvme0n1")
        );
        assert_eq!(
            strip_nvme_partition_suffix("/dev/nvme0n1p12").as_deref(),
            Some("/dev/nvme0n1")
        );
    }

    #[test]
    fn non_nvme_paths_returned_as_is() {
        assert_eq!(strip_nvme_partition_suffix("/dev/sda"), None);
        assert_eq!(strip_nvme_partition_suffix("/dev/sda1"), None);
    }
}
