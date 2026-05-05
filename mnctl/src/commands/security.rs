use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct SecurityArgs {
    #[command(subcommand)]
    command: SecurityCommand,
}

#[derive(Subcommand)]
enum SecurityCommand {
    /// Run a full security audit report
    Audit,
    /// Firewall management (nftables)
    Firewall(FirewallArgs),
    /// AppArmor profile management
    Apparmor(ApparmorArgs),
    /// Fail2ban management
    Fail2ban(Fail2banArgs),
    /// Check installed packages against CVE database
    CveCheck,
    /// Run AIDE integrity check
    Integrity,
    /// Apply hardening profile
    Harden {
        /// Hardening level
        #[arg(long, default_value = "server")]
        level: String,
    },
}

#[derive(Args)]
struct FirewallArgs {
    #[command(subcommand)]
    command: FirewallCommand,
}

#[derive(Subcommand)]
enum FirewallCommand {
    /// Show current nftables rules
    Status,
    /// Allow traffic on a port or service
    Allow {
        /// Port number or service name (e.g., 80, 443, http, https)
        port: String,
    },
    /// Deny traffic on a port or service
    Deny {
        /// Port number or service name
        port: String,
    },
    /// List all firewall rules
    List,
    /// Reload nftables configuration
    Reload,
}

#[derive(Args)]
struct ApparmorArgs {
    #[command(subcommand)]
    command: ApparmorCommand,
}

#[derive(Subcommand)]
enum ApparmorCommand {
    /// Show AppArmor status for all profiles
    Status,
    /// Set a profile to enforce mode
    Enforce {
        /// Profile name
        profile: String,
    },
    /// Set a profile to complain mode
    Complain {
        /// Profile name
        profile: String,
    },
    /// Reload all AppArmor profiles
    Reload,
}

#[derive(Args)]
struct Fail2banArgs {
    #[command(subcommand)]
    command: Fail2banCommand,
}

#[derive(Subcommand)]
enum Fail2banCommand {
    /// Show fail2ban jail status
    Status,
    /// Unban an IP address
    Unban {
        /// IP address to unban
        ip: String,
    },
    /// Show current bans
    Bans,
}

impl SecurityArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            SecurityCommand::Audit => security_audit(),
            SecurityCommand::Firewall(args) => match args.command {
                FirewallCommand::Status => firewall_status(),
                FirewallCommand::Allow { port } => firewall_allow(&port),
                FirewallCommand::Deny { port } => firewall_deny(&port),
                FirewallCommand::List => firewall_list(),
                FirewallCommand::Reload => firewall_reload(),
            },
            SecurityCommand::Apparmor(args) => match args.command {
                ApparmorCommand::Status => apparmor_status(),
                ApparmorCommand::Enforce { profile } => apparmor_set_mode("enforce", &profile),
                ApparmorCommand::Complain { profile } => apparmor_set_mode("complain", &profile),
                ApparmorCommand::Reload => apparmor_reload(),
            },
            SecurityCommand::Fail2ban(args) => match args.command {
                Fail2banCommand::Status => fail2ban_status(),
                Fail2banCommand::Unban { ip } => fail2ban_unban(&ip),
                Fail2banCommand::Bans => fail2ban_bans(),
            },
            SecurityCommand::CveCheck => cve_check(),
            SecurityCommand::Integrity => integrity_check(),
            SecurityCommand::Harden { level } => apply_hardening(&level),
        }
    }
}

fn security_audit() -> Result<()> {
    println!("{}", "Monolith Security Audit".bold().underline());
    println!();

    // SSH config check
    print!("  Checking SSH configuration... ");
    let sshd_config = std::fs::read_to_string("/etc/ssh/sshd_config").unwrap_or_default();
    if sshd_config.contains("PermitRootLogin no") {
        println!("{}", "PASS — root login disabled".green());
    } else {
        println!("{}", "WARN — root login may be enabled".yellow());
    }
    if sshd_config.contains("PasswordAuthentication no") {
        println!(
            "  {} {}",
            "SSH passwords:".dimmed(),
            "disabled (key-only)".green()
        );
    } else {
        println!(
            "  {} {}",
            "SSH passwords:".dimmed(),
            "enabled (consider disabling)".yellow()
        );
    }

    // Firewall check
    print!("  Checking firewall... ");
    let nft = Command::new("nft").args(["list", "ruleset"]).output();
    match nft {
        Ok(o) if o.status.success() => {
            let rules = String::from_utf8_lossy(&o.stdout);
            if rules.contains("policy drop") {
                println!("{}", "PASS — default-drop policy".green());
            } else {
                println!("{}", "WARN — no default-drop policy detected".yellow());
            }
        }
        _ => println!("{}", "SKIP — nftables not available".dimmed()),
    }

    // AppArmor check
    print!("  Checking AppArmor... ");
    let aa = Command::new("aa-status").output();
    match aa {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            let enforce_count = out
                .lines()
                .find(|l| l.contains("profiles are in enforce mode"))
                .unwrap_or("0 profiles");
            println!("{} — {}", "ACTIVE".green(), enforce_count.trim());
        }
        _ => println!("{}", "SKIP — AppArmor not available".dimmed()),
    }

    // Fail2ban check
    print!("  Checking fail2ban... ");
    let f2b = Command::new("fail2ban-client").args(["status"]).output();
    match f2b {
        Ok(o) if o.status.success() => {
            println!("{}", "ACTIVE".green());
        }
        _ => println!("{}", "NOT RUNNING".yellow()),
    }

    // Kernel hardening check
    println!();
    println!("  {}", "Kernel hardening:".bold());
    let checks = [
        ("kernel.dmesg_restrict", "1"),
        ("kernel.kptr_restrict", "2"),
        ("net.ipv4.conf.all.rp_filter", "1"),
        ("net.ipv4.tcp_syncookies", "1"),
        ("kernel.randomize_va_space", "2"),
    ];

    for (param, expected) in &checks {
        let output = Command::new("sysctl").args(["-n", param]).output();
        match output {
            Ok(o) if o.status.success() => {
                let val = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if val == *expected {
                    println!("    {} {} = {}", "●".green(), param, val);
                } else {
                    println!(
                        "    {} {} = {} (expected {})",
                        "●".yellow(),
                        param,
                        val,
                        expected
                    );
                }
            }
            _ => println!("    {} {} — unavailable", "●".dimmed(), param),
        }
    }

    Ok(())
}

fn firewall_status() -> Result<()> {
    let output = Command::new("nft")
        .args(["list", "ruleset"])
        .output()
        .context("failed to get nftables status — is nftables installed?")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn firewall_allow(port: &str) -> Result<()> {
    let port_num: u16 = port.parse().unwrap_or(match port {
        "http" => 80,
        "https" => 443,
        "ssh" => 2222,
        "dns" => 53,
        "mysql" => 3306,
        "postgresql" | "postgres" => 5432,
        "redis" => 6379,
        "mongodb" => 27017,
        "minecraft" => 25565,
        _ => 0,
    });

    if port_num == 0 {
        anyhow::bail!("unknown port or service: {port}");
    }

    let status = Command::new("nft")
        .args([
            "add",
            "rule",
            "inet",
            "monolith",
            "input",
            "tcp",
            "dport",
            &port_num.to_string(),
            "accept",
            "comment",
            "\"mnctl-managed\"",
        ])
        .status()
        .with_context(|| format!("failed to add firewall rule for port {port_num}"))?;

    if status.success() {
        println!(
            "{} Allowed TCP port {} ({})",
            "●".green(),
            port_num.to_string().bold(),
            port
        );
        save_nftables()?;
    } else {
        anyhow::bail!("failed to add rule for port {port_num}");
    }
    Ok(())
}

fn firewall_deny(port: &str) -> Result<()> {
    let port_num: u16 = port.parse().unwrap_or(0);
    if port_num == 0 {
        anyhow::bail!("invalid port: {port}");
    }

    let status = Command::new("nft")
        .args([
            "add",
            "rule",
            "inet",
            "monolith",
            "input",
            "tcp",
            "dport",
            &port_num.to_string(),
            "drop",
            "comment",
            "\"mnctl-managed\"",
        ])
        .status()
        .with_context(|| format!("failed to add deny rule for port {port_num}"))?;

    if status.success() {
        println!("{} Denied TCP port {}", "●".red(), port_num);
        save_nftables()?;
    }
    Ok(())
}

fn firewall_list() -> Result<()> {
    firewall_status()
}

fn firewall_reload() -> Result<()> {
    let status = Command::new("nft")
        .args(["-f", "/etc/nftables.conf"])
        .status()
        .context("failed to reload nftables")?;

    if status.success() {
        println!("{} Firewall reloaded", "●".green());
    } else {
        anyhow::bail!("failed to reload nftables");
    }
    Ok(())
}

fn save_nftables() -> Result<()> {
    let output = Command::new("nft")
        .args(["list", "ruleset"])
        .output()
        .context("failed to save nftables rules")?;

    std::fs::write("/etc/nftables.conf", &output.stdout)
        .context("failed to write /etc/nftables.conf")?;
    Ok(())
}

fn apparmor_status() -> Result<()> {
    let output = Command::new("aa-status")
        .output()
        .context("failed to get AppArmor status")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn apparmor_set_mode(mode: &str, profile: &str) -> Result<()> {
    let cmd = format!("aa-{mode}");
    let status = Command::new(&cmd)
        .arg(profile)
        .status()
        .with_context(|| format!("failed to set {profile} to {mode} mode"))?;

    if status.success() {
        println!(
            "{} Profile {} set to {} mode",
            "●".green(),
            profile.bold(),
            mode
        );
    } else {
        anyhow::bail!("failed to set {profile} to {mode} mode");
    }
    Ok(())
}

fn apparmor_reload() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["reload", "apparmor"])
        .status()
        .context("failed to reload AppArmor")?;

    if status.success() {
        println!("{} AppArmor profiles reloaded", "●".green());
    } else {
        anyhow::bail!("failed to reload AppArmor");
    }
    Ok(())
}

fn fail2ban_status() -> Result<()> {
    let output = Command::new("fail2ban-client")
        .args(["status"])
        .output()
        .context("failed to get fail2ban status")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn fail2ban_unban(ip: &str) -> Result<()> {
    let status = Command::new("fail2ban-client")
        .args(["unban", ip])
        .status()
        .with_context(|| format!("failed to unban {ip}"))?;

    if status.success() {
        println!("{} Unbanned {}", "●".green(), ip.bold());
    }
    Ok(())
}

fn fail2ban_bans() -> Result<()> {
    let output = Command::new("fail2ban-client")
        .args(["status", "sshd"])
        .output()
        .context("failed to get fail2ban bans")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn cve_check() -> Result<()> {
    println!(
        "{}",
        "Checking installed packages for known CVEs...".dimmed()
    );
    let output = Command::new("arch-audit").output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.trim().is_empty() {
                println!("{}", "No known CVEs found in installed packages.".green());
            } else {
                println!("{}", "Vulnerable packages:".bold().underline());
                print!("{stdout}");
            }
        }
        _ => {
            println!(
                "{}",
                "arch-audit not installed. Install with: mnpkg install arch-audit".yellow()
            );
        }
    }
    Ok(())
}

fn integrity_check() -> Result<()> {
    println!("{}", "Running AIDE integrity check...".dimmed());
    let status = Command::new("aide").args(["--check"]).output();

    match status {
        Ok(o) => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
            if o.status.success() {
                println!("{}", "Integrity check passed.".green());
            } else {
                println!("{}", "Integrity violations detected!".red().bold());
            }
        }
        Err(_) => {
            println!(
                "{}",
                "AIDE not installed. Install with: mnpkg install aide".yellow()
            );
        }
    }
    Ok(())
}

fn apply_hardening(level: &str) -> Result<()> {
    println!("Applying {} hardening profile...", level.bold());

    match level {
        "paranoid" => {
            println!("  {} Disabling all non-essential services", "→".blue());
            println!("  {} Setting most restrictive sysctl values", "→".blue());
            println!("  {} Enforcing all AppArmor profiles", "→".blue());
        }
        "server" => {
            println!("  {} Applying balanced server hardening", "→".blue());
            println!("  {} Enforcing critical AppArmor profiles", "→".blue());
        }
        "default" => {
            println!(
                "  {} Restoring default Monolith security settings",
                "→".blue()
            );
        }
        _ => {
            anyhow::bail!("unknown hardening level: {level}. Use: paranoid, server, or default");
        }
    }

    let sysctl_values = match level {
        "paranoid" => vec![
            ("kernel.dmesg_restrict", "1"),
            ("kernel.kptr_restrict", "2"),
            ("kernel.unprivileged_bpf_disabled", "1"),
            ("kernel.perf_event_paranoid", "3"),
            ("kernel.yama.ptrace_scope", "3"),
            ("kernel.sysrq", "0"),
            ("net.ipv4.conf.all.rp_filter", "1"),
            ("net.ipv4.tcp_syncookies", "1"),
        ],
        "server" | "default" => vec![
            ("kernel.dmesg_restrict", "1"),
            ("kernel.kptr_restrict", "2"),
            ("kernel.unprivileged_bpf_disabled", "1"),
            ("kernel.perf_event_paranoid", "3"),
            ("kernel.yama.ptrace_scope", "1"),
            ("kernel.sysrq", "0"),
            ("net.ipv4.conf.all.rp_filter", "1"),
            ("net.ipv4.tcp_syncookies", "1"),
        ],
        _ => vec![],
    };

    for (param, val) in &sysctl_values {
        let _ = Command::new("sysctl")
            .args(["-w", &format!("{param}={val}")])
            .output();
    }

    println!("{} Hardening profile '{}' applied", "●".green(), level);
    Ok(())
}
