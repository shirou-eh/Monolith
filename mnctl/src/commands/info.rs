use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;
use sysinfo::System;

#[derive(Args)]
pub struct InfoArgs {
    #[command(subcommand)]
    command: InfoCommand,
}

#[derive(Subcommand)]
enum InfoCommand {
    /// OS version, kernel, uptime, hardware summary
    System,
    /// Detailed hardware information
    Hardware,
    /// Monolith version info
    Version,
}

impl InfoArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            InfoCommand::System => system_info(),
            InfoCommand::Hardware => hardware_info(),
            InfoCommand::Version => version_info(),
        }
    }
}

fn system_info() -> Result<()> {
    let mut sys = System::new_all();
    sys.refresh_all();

    println!("{}", "System Information".bold().underline());
    println!();
    println!(
        "  {} {}",
        "OS:".dimmed(),
        System::long_os_version().unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "  {} {}",
        "Kernel:".dimmed(),
        System::kernel_version().unwrap_or_else(|| "unknown".to_string())
    );
    println!(
        "  {} {}",
        "Hostname:".dimmed(),
        System::host_name().unwrap_or_else(|| "unknown".to_string())
    );

    let uptime = System::uptime();
    let days = uptime / 86400;
    let hours = (uptime % 86400) / 3600;
    let mins = (uptime % 3600) / 60;
    println!("  {} {}d {}h {}m", "Uptime:".dimmed(), days, hours, mins);

    println!();
    println!("  {} {} cores", "CPU:".dimmed(), sys.cpus().len());
    if let Some(cpu) = sys.cpus().first() {
        println!("  {} {}", "CPU Model:".dimmed(), cpu.brand());
    }
    println!(
        "  {} {} MB",
        "Total RAM:".dimmed(),
        sys.total_memory() / 1024 / 1024
    );
    println!(
        "  {} {} MB",
        "Total Swap:".dimmed(),
        sys.total_swap() / 1024 / 1024
    );

    let arch = std::env::consts::ARCH;
    println!("  {} {}", "Architecture:".dimmed(), arch);

    Ok(())
}

fn hardware_info() -> Result<()> {
    let mut sys = System::new_all();
    sys.refresh_all();

    println!("{}", "Hardware Information".bold().underline());
    println!();

    // CPU details
    println!("  {}", "CPU:".bold());
    if let Some(cpu) = sys.cpus().first() {
        println!("    Model:     {}", cpu.brand());
        println!("    Cores:     {}", sys.cpus().len());
        println!("    Frequency: {} MHz", cpu.frequency());
    }

    println!();

    // Memory
    println!("  {}", "Memory:".bold());
    println!("    Total:     {} MB", sys.total_memory() / 1024 / 1024);
    println!("    Used:      {} MB", sys.used_memory() / 1024 / 1024);
    println!(
        "    Available: {} MB",
        (sys.total_memory() - sys.used_memory()) / 1024 / 1024
    );

    println!();

    // Disks
    println!("  {}", "Disks:".bold());
    for disk in sysinfo::Disks::new_with_refreshed_list().list() {
        println!(
            "    {:<20} {:>10} GB total, {:>10} GB free  ({})",
            disk.mount_point().to_string_lossy(),
            disk.total_space() / 1024 / 1024 / 1024,
            disk.available_space() / 1024 / 1024 / 1024,
            disk.file_system().to_string_lossy(),
        );
    }

    println!();

    // Network interfaces
    println!("  {}", "Network:".bold());
    let output = std::process::Command::new("ip")
        .args(["-brief", "link"])
        .output();

    if let Ok(o) = output {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            println!("    {line}");
        }
    }

    Ok(())
}

fn version_info() -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    let codename = "Obsidian";
    let title = format!("MONOLITH OS  v{version}  ·  {codename}");
    let bar = "─".repeat(title.chars().count() + 4);
    println!();
    println!("  {}", bar.truecolor(53, 224, 161));
    println!(
        "  {} {} {}",
        "│".truecolor(53, 224, 161),
        title.bold().truecolor(232, 236, 241),
        "│".truecolor(53, 224, 161)
    );
    println!("  {}", bar.truecolor(53, 224, 161));
    println!();
    println!(
        "  {}",
        "Built for the ones who mean it.".truecolor(125, 132, 143)
    );
    println!();
    println!("  {}", "Components".bold());
    println!(
        "    {}  v{version}",
        "mnctl             ".truecolor(125, 132, 143)
    );
    println!(
        "    {}  v{version}",
        "mnpkg             ".truecolor(125, 132, 143)
    );
    println!(
        "    {}  v{version}",
        "mntui             ".truecolor(125, 132, 143)
    );
    println!(
        "    {}  v{version}",
        "mnweb             ".truecolor(125, 132, 143)
    );
    println!(
        "    {}  v{version}",
        "monolith-installer".truecolor(125, 132, 143)
    );
    println!();
    println!(
        "  {} https://github.com/shirou-eh/Monolith",
        "↗".truecolor(92, 201, 255)
    );
    println!();
    Ok(())
}
