use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct NetworkArgs {
    #[command(subcommand)]
    command: NetworkCommand,
}

#[derive(Subcommand)]
enum NetworkCommand {
    /// Show all interfaces, IPs, and routes
    Status,
    /// List network interfaces
    Interfaces,
    /// Show or change DNS servers
    Dns {
        /// Set DNS server
        #[arg(long)]
        set: Option<String>,
    },
    /// Show routing table
    Routes,
    /// Run network connectivity test
    Test {
        /// Host to test connectivity to
        host: String,
    },
}

impl NetworkArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            NetworkCommand::Status => network_status(),
            NetworkCommand::Interfaces => list_interfaces(),
            NetworkCommand::Dns { set } => dns_config(set.as_deref()),
            NetworkCommand::Routes => show_routes(),
            NetworkCommand::Test { host } => test_connectivity(&host),
        }
    }
}

fn network_status() -> Result<()> {
    println!("{}", "Network Status".bold().underline());
    println!();

    list_interfaces()?;
    println!();
    show_routes()?;

    println!();
    println!("{}", "DNS Servers:".bold());
    let resolv = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    for line in resolv.lines() {
        if line.starts_with("nameserver") {
            println!("  {line}");
        }
    }
    Ok(())
}

fn list_interfaces() -> Result<()> {
    let output = Command::new("ip")
        .args(["-brief", "addr"])
        .output()
        .context("failed to list interfaces")?;

    println!("{}", "Interfaces:".bold());
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let name = parts[0];
            let state = parts[1];
            let addrs = parts[2..].join(", ");
            let indicator = match state {
                "UP" => "●".green(),
                "DOWN" => "●".red(),
                _ => "●".yellow(),
            };
            println!("  {indicator} {:<15} {:<8} {}", name, state, addrs);
        }
    }
    Ok(())
}

fn dns_config(set: Option<&str>) -> Result<()> {
    if let Some(server) = set {
        println!("{} Setting DNS to {}...", "→".blue(), server.bold());
        let content = format!("nameserver {server}\n");
        std::fs::write("/etc/resolv.conf", &content).context("failed to write /etc/resolv.conf")?;
        println!("{} DNS server set to {}", "●".green(), server);
    } else {
        let content = std::fs::read_to_string("/etc/resolv.conf")
            .context("failed to read /etc/resolv.conf")?;
        println!("{}", "DNS Configuration:".bold());
        print!("{content}");
    }
    Ok(())
}

fn show_routes() -> Result<()> {
    let output = Command::new("ip")
        .args(["route"])
        .output()
        .context("failed to get routing table")?;

    println!("{}", "Routes:".bold());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn test_connectivity(host: &str) -> Result<()> {
    println!("{}", format!("Testing connectivity to {host}...").bold());
    println!();

    // DNS resolution
    print!("  DNS resolution... ");
    let dig = Command::new("dig").args(["+short", host]).output();

    match dig {
        Ok(o) if o.status.success() => {
            let ips = String::from_utf8_lossy(&o.stdout);
            if ips.trim().is_empty() {
                println!("{}", "FAILED".red());
            } else {
                println!("{} ({})", "OK".green(), ips.trim());
            }
        }
        _ => println!("{}", "dig not available".dimmed()),
    }

    // Ping
    print!("  Ping... ");
    let ping = Command::new("ping")
        .args(["-c", "3", "-W", "5", host])
        .output()
        .context("failed to ping")?;

    if ping.status.success() {
        let stdout = String::from_utf8_lossy(&ping.stdout);
        let rtt = stdout
            .lines()
            .find(|l| l.contains("rtt"))
            .unwrap_or("no RTT data");
        println!("{} — {}", "OK".green(), rtt);
    } else {
        println!("{}", "FAILED".red());
    }

    // TCP connectivity
    print!("  TCP 443 (HTTPS)... ");
    let nc = Command::new("bash")
        .args([
            "-c",
            &format!("timeout 5 bash -c 'echo >/dev/tcp/{host}/443' 2>/dev/null"),
        ])
        .status();

    match nc {
        Ok(s) if s.success() => println!("{}", "OK".green()),
        _ => println!("{}", "FAILED or timeout".yellow()),
    }

    // Traceroute
    println!();
    println!("  {}:", "Traceroute".bold());
    let traceroute = Command::new("traceroute")
        .args(["-m", "15", "-w", "3", host])
        .output();

    match traceroute {
        Ok(o) => print!("  {}", String::from_utf8_lossy(&o.stdout)),
        Err(_) => println!("  {}", "traceroute not available".dimmed()),
    }

    Ok(())
}
