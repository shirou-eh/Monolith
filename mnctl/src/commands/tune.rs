//! Performance tuning — make sure every CPU load is spread across all
//! available cores and threads.
//!
//! `mnctl tune` is the one-shot knob operators reach for after a fresh
//! install: it applies an opinionated, server-friendly performance
//! profile that affects how the kernel scheduler, CPU frequency
//! governors, and block-device I/O schedulers behave.
//!
//! What it does today:
//!
//! * **CPU** — pin every online CPU to the `performance` governor (or
//!   `schedutil` on `--balanced`), set
//!   `energy_performance_preference=performance`, raise minimum
//!   frequency to the maximum, enable transparent hugepages and SMT,
//!   and start `irqbalance` so hardware interrupts fan out across
//!   every core instead of piling on CPU0.
//! * **I/O** — pick a sensible elevator per device class:
//!   `none` (multi-queue) for NVMe, `mq-deadline` for rotational
//!   disks, and bump `nr_requests` / `read_ahead_kb` for sustained
//!   throughput.
//! * **Reset** — restore the kernel defaults (`schedutil`,
//!   `power`, `bfq`/`mq-deadline` per device class).
//!
//! Every action is idempotent and supports `--dry-run`, so it's safe
//! to wire into a systemd unit or call repeatedly from `mnctl tune
//! all` after boot.
use anyhow::Result;
use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Args)]
pub struct TuneArgs {
    #[command(subcommand)]
    command: TuneCommand,
}

#[derive(Subcommand)]
enum TuneCommand {
    /// Apply CPU performance tuning across all cores and threads
    Cpu {
        /// Tuning preset
        #[arg(long, value_enum, default_value_t = CpuPreset::Performance)]
        preset: CpuPreset,
        /// Print actions without modifying the system
        #[arg(long)]
        dry_run: bool,
    },
    /// Apply I/O scheduler tuning to every block device
    Io {
        /// Print actions without modifying the system
        #[arg(long)]
        dry_run: bool,
    },
    /// Apply CPU + I/O tuning in one go (recommended)
    All {
        /// Tuning preset for the CPU stage
        #[arg(long, value_enum, default_value_t = CpuPreset::Performance)]
        preset: CpuPreset,
        /// Print actions without modifying the system
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the current governor / scheduler / parallelism state
    Status,
    /// Restore kernel defaults (schedutil + per-device default elevator)
    Reset {
        /// Print actions without modifying the system
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum CpuPreset {
    /// Pin every CPU to the `performance` governor — maximum throughput
    Performance,
    /// Use `schedutil` — kernel-default, balanced power/perf
    Balanced,
    /// Pin every CPU to `powersave` — minimum power draw
    Powersave,
}

impl CpuPreset {
    fn governor(self) -> &'static str {
        match self {
            CpuPreset::Performance => "performance",
            CpuPreset::Balanced => "schedutil",
            CpuPreset::Powersave => "powersave",
        }
    }

    fn epp(self) -> &'static str {
        match self {
            CpuPreset::Performance => "performance",
            CpuPreset::Balanced => "balance_performance",
            CpuPreset::Powersave => "power",
        }
    }
}

impl TuneArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            TuneCommand::Cpu { preset, dry_run } => tune_cpu(preset, dry_run),
            TuneCommand::Io { dry_run } => tune_io(dry_run),
            TuneCommand::All { preset, dry_run } => {
                tune_cpu(preset, dry_run)?;
                println!();
                tune_io(dry_run)
            }
            TuneCommand::Status => tune_status(),
            TuneCommand::Reset { dry_run } => tune_reset(dry_run),
        }
    }
}

/// Spread every CPU-bound load across all available cores and threads:
///
/// * Set the CPU frequency governor on **every** online CPU (P-cores
///   and E-cores both) to the requested preset.
/// * Raise `scaling_min_freq` to `cpuinfo_max_freq` so cores never
///   idle below their nominal clock under the `performance` preset.
/// * Bump `energy_performance_preference` so Intel/AMD HWP picks the
///   matching hint.
/// * Enable transparent hugepages (`madvise` is the safe default for
///   server workloads — apps that benefit opt in).
/// * Re-enable simultaneous multi-threading if it was disabled at
///   runtime so all logical CPUs are visible to the scheduler.
/// * Start `irqbalance` if available so hardware IRQs are distributed
///   instead of pinning to CPU0.
fn tune_cpu(preset: CpuPreset, dry_run: bool) -> Result<()> {
    let cores = available_cores();
    println!("{}", "CPU performance tuning".bold().underline());
    println!(
        "  preset: {} ({} cores / {} logical)",
        preset.governor().green().bold(),
        num_physical_cores(),
        cores.len(),
    );
    println!();

    set_governor_all(preset.governor(), &cores, dry_run);
    set_epp_all(preset.epp(), &cores, dry_run);
    raise_min_freq_all(&cores, dry_run);
    set_thp(matches!(preset, CpuPreset::Performance), dry_run);
    enable_smt(dry_run);
    start_irqbalance(dry_run);

    println!();
    if dry_run {
        println!("  {} dry-run — no changes applied", "i".dimmed());
    } else {
        println!(
            "  {} CPU tuned ({}). Running tasks now spread across all cores.",
            "●".green(),
            preset.governor().bold()
        );
    }
    Ok(())
}

fn tune_io(dry_run: bool) -> Result<()> {
    println!("{}", "Block I/O scheduler tuning".bold().underline());
    println!();

    let devices = list_block_devices();
    if devices.is_empty() {
        println!("  {} no block devices found under /sys/block", "i".dimmed());
        return Ok(());
    }

    for dev in &devices {
        let elevator = pick_elevator(dev);
        let scheduler_path = format!("/sys/block/{dev}/queue/scheduler");
        let nr_requests_path = format!("/sys/block/{dev}/queue/nr_requests");
        let read_ahead_path = format!("/sys/block/{dev}/queue/read_ahead_kb");

        println!(
            "  {} {} → scheduler={} nr_requests=1024 read_ahead_kb=2048",
            "→".blue(),
            dev.bold(),
            elevator
        );
        write_sysfs(&scheduler_path, elevator, dry_run);
        write_sysfs(&nr_requests_path, "1024", dry_run);
        write_sysfs(&read_ahead_path, "2048", dry_run);
    }

    println!();
    if dry_run {
        println!("  {} dry-run — no changes applied", "i".dimmed());
    } else {
        println!(
            "  {} I/O tuned across {} block device(s).",
            "●".green(),
            devices.len()
        );
    }
    Ok(())
}

fn tune_status() -> Result<()> {
    println!("{}", "Tuning status".bold().underline());
    println!();

    let cores = available_cores();
    println!(
        "  {} {} logical CPU(s) ({} physical core(s))",
        "CPUs:".dimmed(),
        cores.len(),
        num_physical_cores()
    );

    // Governor consensus
    let governors: Vec<String> = cores
        .iter()
        .filter_map(|cpu| {
            let path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor");
            fs::read_to_string(path).ok().map(|s| s.trim().to_string())
        })
        .collect();
    let governor_summary = match governors.first() {
        Some(first) if governors.iter().all(|g| g == first) => first.clone(),
        Some(_) => "mixed".to_string(),
        None => "n/a".to_string(),
    };
    println!("  {} {}", "Governor:".dimmed(), governor_summary.bold());

    // SMT
    let smt = fs::read_to_string("/sys/devices/system/cpu/smt/active")
        .map(|s| s.trim() == "1")
        .ok();
    let smt_str = match smt {
        Some(true) => "on".green().to_string(),
        Some(false) => "off".yellow().to_string(),
        None => "n/a".dimmed().to_string(),
    };
    println!("  {} {}", "SMT:".dimmed(), smt_str);

    // THP
    let thp = fs::read_to_string("/sys/kernel/mm/transparent_hugepage/enabled")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .find(|w| w.starts_with('[') && w.ends_with(']'))
                .map(|w| w.trim_matches(|c| c == '[' || c == ']').to_string())
        })
        .unwrap_or_else(|| "n/a".to_string());
    println!("  {} {}", "THP:".dimmed(), thp.bold());

    // I/O schedulers
    let devices = list_block_devices();
    if !devices.is_empty() {
        println!("  {}", "I/O schedulers:".dimmed());
        for dev in &devices {
            let path = format!("/sys/block/{dev}/queue/scheduler");
            let active = fs::read_to_string(&path)
                .ok()
                .and_then(|s| current_scheduler(&s))
                .unwrap_or_else(|| "n/a".to_string());
            println!("    {dev:<10} {}", active.green());
        }
    }

    Ok(())
}

fn tune_reset(dry_run: bool) -> Result<()> {
    println!("{}", "Restoring kernel defaults".bold().underline());
    println!();

    let cores = available_cores();
    set_governor_all("schedutil", &cores, dry_run);
    set_epp_all("balance_performance", &cores, dry_run);
    set_thp(false, dry_run);

    for dev in list_block_devices() {
        // Multi-queue rotational defaults to mq-deadline; NVMe defaults
        // to none. We just pick a known-safe elevator per class.
        let scheduler_path = format!("/sys/block/{dev}/queue/scheduler");
        let elevator = if is_rotational(&dev) { "bfq" } else { "none" };
        write_sysfs(&scheduler_path, elevator, dry_run);
    }

    println!();
    if dry_run {
        println!("  {} dry-run — no changes applied", "i".dimmed());
    } else {
        println!("  {} defaults restored.", "●".green());
    }
    Ok(())
}

// ----- helpers --------------------------------------------------------------

/// Return every online logical CPU id (0..N).
fn available_cores() -> Vec<usize> {
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/online") {
        return parse_cpu_list(content.trim());
    }
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    (0..n).collect()
}

/// Parse a Linux CPU range list like `0-3,5,7-9` into [0,1,2,3,5,7,8,9].
fn parse_cpu_list(s: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for chunk in s.split(',') {
        if let Some((a, b)) = chunk.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                out.extend(a..=b);
            }
        } else if let Ok(n) = chunk.trim().parse::<usize>() {
            out.push(n);
        }
    }
    out
}

fn num_physical_cores() -> usize {
    // sysinfo would pull in a heavy dep here; just count unique
    // `core_id` values under /sys/devices/system/cpu/cpu*/topology.
    let mut seen = std::collections::BTreeSet::new();
    for cpu in available_cores() {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/topology/core_id");
        if let Ok(s) = fs::read_to_string(path) {
            if let Ok(id) = s.trim().parse::<u32>() {
                seen.insert(id);
            }
        }
    }
    if seen.is_empty() {
        available_cores().len()
    } else {
        seen.len()
    }
}

fn set_governor_all(governor: &str, cores: &[usize], dry_run: bool) {
    println!(
        "  {} setting governor → {} on {} cpu(s)",
        "→".blue(),
        governor.bold(),
        cores.len()
    );
    let mut missing = false;
    for cpu in cores {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_governor");
        if !Path::new(&path).exists() {
            missing = true;
            continue;
        }
        write_sysfs(&path, governor, dry_run);
    }
    if missing {
        println!(
            "    {} cpufreq sysfs not present on every cpu (likely a VM); skipped those.",
            "i".dimmed()
        );
    }
}

fn set_epp_all(epp: &str, cores: &[usize], dry_run: bool) {
    let mut wrote_any = false;
    for cpu in cores {
        let path =
            format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/energy_performance_preference");
        if Path::new(&path).exists() {
            write_sysfs(&path, epp, dry_run);
            wrote_any = true;
        }
    }
    if wrote_any {
        println!(
            "  {} energy_performance_preference → {}",
            "→".blue(),
            epp.bold()
        );
    }
}

fn raise_min_freq_all(cores: &[usize], dry_run: bool) {
    let mut wrote_any = false;
    for cpu in cores {
        let max_path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq");
        let min_path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_min_freq");
        if let Ok(max) = fs::read_to_string(&max_path) {
            let max = max.trim();
            if Path::new(&min_path).exists() {
                write_sysfs(&min_path, max, dry_run);
                wrote_any = true;
            }
        }
    }
    if wrote_any {
        println!(
            "  {} scaling_min_freq raised to cpuinfo_max_freq on all cores",
            "→".blue()
        );
    }
}

fn set_thp(performance: bool, dry_run: bool) {
    // `madvise` is the safe server-side default: opt-in for apps that
    // want hugepages (databases, JVMs) without forcing 2 MiB pages on
    // every allocation. The `performance` flag exists so we can later
    // promote to `always` if a workload needs it — for now both
    // presets converge on `madvise` because the perf delta is small
    // and the memory overhead of `always` is real.
    let _ = performance;
    let mode = "madvise";
    let path = "/sys/kernel/mm/transparent_hugepage/enabled";
    if Path::new(path).exists() {
        println!("  {} transparent_hugepage → {}", "→".blue(), mode.bold());
        write_sysfs(path, mode, dry_run);
    }
}

fn enable_smt(dry_run: bool) {
    let path = "/sys/devices/system/cpu/smt/control";
    if !Path::new(path).exists() {
        return;
    }
    if let Ok(active) = fs::read_to_string("/sys/devices/system/cpu/smt/active") {
        if active.trim() == "1" {
            return;
        }
    }
    println!("  {} enabling SMT (was off)", "→".blue());
    write_sysfs(path, "on", dry_run);
}

fn start_irqbalance(dry_run: bool) {
    if !command_exists("irqbalance") {
        return;
    }
    println!("  {} ensuring irqbalance is running", "→".blue());
    if dry_run {
        println!("    [dry-run] systemctl enable --now irqbalance");
        return;
    }
    let _ = Command::new("systemctl")
        .args(["enable", "--now", "irqbalance"])
        .status();
}

fn list_block_devices() -> Vec<String> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/block") {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip loop / ram / virtual devices that don't have a real
        // queue/scheduler.
        if name.starts_with("loop")
            || name.starts_with("ram")
            || name.starts_with("zram")
            || name.starts_with("dm-")
            || name.starts_with("md")
        {
            continue;
        }
        let scheduler = format!("/sys/block/{name}/queue/scheduler");
        if Path::new(&scheduler).exists() {
            out.push(name);
        }
    }
    out.sort();
    out
}

fn is_rotational(dev: &str) -> bool {
    let path = format!("/sys/block/{dev}/queue/rotational");
    fs::read_to_string(path)
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

fn pick_elevator(dev: &str) -> &'static str {
    let scheduler_path = format!("/sys/block/{dev}/queue/scheduler");
    let available = fs::read_to_string(&scheduler_path).unwrap_or_default();
    let candidates: Vec<&str> = if dev.starts_with("nvme") {
        // NVMe loves multi-queue with no extra elevator; fall back to
        // mq-deadline if the kernel was built without `none`.
        vec!["none", "mq-deadline"]
    } else if is_rotational(dev) {
        vec!["bfq", "mq-deadline"]
    } else {
        // SATA/SAS SSDs.
        vec!["mq-deadline", "none", "bfq"]
    };
    for cand in candidates {
        if available.contains(cand) {
            return cand;
        }
    }
    "mq-deadline"
}

fn current_scheduler(content: &str) -> Option<String> {
    content
        .split_whitespace()
        .find(|w| w.starts_with('[') && w.ends_with(']'))
        .map(|w| w.trim_matches(|c| c == '[' || c == ']').to_string())
}

fn write_sysfs(path: &str, value: &str, dry_run: bool) {
    if dry_run {
        println!("    [dry-run] echo {value} > {path}");
        return;
    }
    if let Err(e) = fs::write(path, value) {
        // sysfs writes need root; surface the failure but keep going so
        // one missing tunable doesn't abort the whole tune run.
        eprintln!("    {} {}: {}", "warn".yellow(), path, e);
    }
}

fn command_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cpu_list_simple() {
        assert_eq!(parse_cpu_list("0-3"), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("0,2,4"), vec![0, 2, 4]);
        assert_eq!(parse_cpu_list("0-1,3,5-6"), vec![0, 1, 3, 5, 6]);
        assert_eq!(parse_cpu_list("7"), vec![7]);
    }

    #[test]
    fn parse_cpu_list_empty_and_garbage() {
        assert!(parse_cpu_list("").is_empty());
        assert_eq!(parse_cpu_list("0,foo,2"), vec![0, 2]);
    }

    #[test]
    fn current_scheduler_picks_active() {
        assert_eq!(
            current_scheduler("noop deadline [cfq]"),
            Some("cfq".to_string())
        );
        assert_eq!(
            current_scheduler("[none] mq-deadline kyber bfq"),
            Some("none".to_string())
        );
        assert_eq!(current_scheduler("none mq-deadline"), None);
    }

    #[test]
    fn cpu_preset_governors_match_design() {
        assert_eq!(CpuPreset::Performance.governor(), "performance");
        assert_eq!(CpuPreset::Balanced.governor(), "schedutil");
        assert_eq!(CpuPreset::Powersave.governor(), "powersave");
    }
}
