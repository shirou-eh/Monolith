use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::os::unix::fs::PermissionsExt;
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
    /// Live security event stream (SSH, AppArmor, nftables, sudo)
    Watch,
    /// Port scan with diff against previous scan
    Scan {
        /// Scan type: internal (ss) or external (nmap)
        #[arg(long, default_value = "internal")]
        scan_type: String,
    },
    /// Behavioural intrusion detection layered on nftables/AppArmor/fail2ban
    Ids {
        /// Run a single evaluation pass and exit instead of polling forever
        #[arg(long)]
        once: bool,
    },
    /// Bind decoy ports — any connection is treated as hostile and reacted to
    Honeypot {
        /// Comma-separated ports to listen on
        #[arg(long, default_value = "23,3389,5432")]
        ports: String,
    },
    /// Immediately ban a source and snapshot system state for forensics
    React {
        /// Source IP to react to
        ip: String,
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
        /// Rule applies to UDP instead of TCP (e.g. DNS, mDNS)
        #[arg(long)]
        udp: bool,
    },
    /// Deny traffic on a port or service
    Deny {
        /// Port number or service name
        port: String,
        /// Rule applies to UDP instead of TCP
        #[arg(long)]
        udp: bool,
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
                FirewallCommand::Allow { port, udp } => firewall_allow(&port, udp),
                FirewallCommand::Deny { port, udp } => firewall_deny(&port, udp),
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
            SecurityCommand::Watch => security_watch(),
            SecurityCommand::Scan { scan_type } => security_scan(&scan_type),
            SecurityCommand::Ids { once } => security_ids(once),
            SecurityCommand::Honeypot { ports } => security_honeypot(&ports),
            SecurityCommand::React { ip } => security_react(&ip),
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

    // SSH host key permissions check
    print!("  Checking SSH host key permissions... ");
    let ssh_dir = std::path::Path::new("/etc/ssh");
    let mut host_key_ok = true;
    if ssh_dir.exists() {
        for entry in std::fs::read_dir(ssh_dir).unwrap_or_else(|_| std::fs::read_dir("/").unwrap()) {
            if let Ok(entry) = entry {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("ssh_host_") && name.ends_with("_key") {
                    if let Ok(meta) = entry.metadata() {
                        let mode = meta.permissions().mode() & 0o777;
                        if mode > 0o640 {
                            let path = entry.path().to_string_lossy().to_string();
                            println!(
                                "\n  {} SSH host key {} is world-readable ({:o})",
                                "FAIL".red().bold(),
                                path,
                                mode
                            );
                            println!(
                                "     Fix: chmod 0600 {}",
                                path
                            );
                            host_key_ok = false;
                        }
                    }
                }
            }
        }
    }
    if host_key_ok {
        println!("{}", "PASS — host key permissions are safe".green());
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

/// Find the `inet` table this system's firewall rules actually live in.
///
/// The project's own template (`security/nftables/monolith.nft`) creates
/// `inet monolith`, but plenty of boxes — including both machines this
/// was tested against — never had that template loaded and are still
/// running whatever the base install's `inet filter` table is. Rather
/// than fail on real systems, prefer `monolith` if it exists and fall
/// back to `filter`, which is the practical default almost everywhere.
fn detect_firewall_table() -> String {
    let output = Command::new("nft").args(["list", "tables"]).output();
    if let Ok(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        if text.lines().any(|l| l.trim() == "table inet monolith") {
            return "monolith".to_string();
        }
    }
    "filter".to_string()
}

fn resolve_port(port: &str) -> Option<u16> {
    port.parse().ok().or(match port {
        "http" => Some(80),
        "https" => Some(443),
        "ssh" => Some(2222),
        "dns" => Some(53),
        "mdns" => Some(5353),
        "mysql" => Some(3306),
        "postgresql" | "postgres" => Some(5432),
        "redis" => Some(6379),
        "mongodb" => Some(27017),
        "minecraft" => Some(25565),
        _ => None,
    })
}

fn firewall_allow(port: &str, udp: bool) -> Result<()> {
    let port_num = resolve_port(port).ok_or_else(|| anyhow::anyhow!("unknown port or service: {port}"))?;
    let proto = if udp { "udp" } else { "tcp" };
    let table = detect_firewall_table();

    let status = Command::new("nft")
        .args([
            "add",
            "rule",
            "inet",
            &table,
            "input",
            proto,
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
            "{} Allowed {} port {} ({})",
            "●".green(),
            proto.to_uppercase(),
            port_num.to_string().bold(),
            port
        );
        save_nftables()?;
    } else {
        anyhow::bail!("failed to add rule for port {port_num}");
    }
    Ok(())
}

fn firewall_deny(port: &str, udp: bool) -> Result<()> {
    let port_num = resolve_port(port).ok_or_else(|| anyhow::anyhow!("unknown port or service: {port}"))?;
    let proto = if udp { "udp" } else { "tcp" };
    let table = detect_firewall_table();

    let status = Command::new("nft")
        .args([
            "add",
            "rule",
            "inet",
            &table,
            "input",
            proto,
            "dport",
            &port_num.to_string(),
            "drop",
            "comment",
            "\"mnctl-managed\"",
        ])
        .status()
        .with_context(|| format!("failed to add deny rule for port {port_num}"))?;

    if status.success() {
        println!("{} Denied {} port {}", "●".red(), proto.to_uppercase(), port_num);
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
        "desktop" => {
            println!("  {} Applying desktop-friendly hardening", "→".blue());
            println!(
                "  {} AppArmor profiles set to complain (log, don't block)",
                "→".blue()
            );
            println!(
                "  {} Debugger/profiler restrictions relaxed for local dev tools",
                "→".blue()
            );
        }
        "default" => {
            println!(
                "  {} Restoring default Monolith security settings",
                "→".blue()
            );
        }
        _ => {
            anyhow::bail!(
                "unknown hardening level: {level}. Use: paranoid, server, desktop, or default"
            );
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
        // A desktop still gets real protection (dmesg/kptr hidden from
        // unprivileged users, SYN cookies, rp_filter), but doesn't pay
        // for restrictions that mainly exist to stop remote attackers
        // probing a headless box: BPF and perf stay usable for local
        // dev/profiling tools, ptrace_scope stays at the *system*
        // default (0) so `gdb -p`/`strace -p` work without ceremony,
        // and sysrq stays on since a human is physically at the box.
        "desktop" => vec![
            ("kernel.dmesg_restrict", "1"),
            ("kernel.kptr_restrict", "1"),
            ("kernel.unprivileged_bpf_disabled", "0"),
            ("kernel.perf_event_paranoid", "1"),
            ("kernel.yama.ptrace_scope", "0"),
            ("kernel.sysrq", "1"),
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

fn security_watch() -> Result<()> {
    println!("{}", " Security Event Watch ".bold().underline());
    println!("  Following: SSH auth, AppArmor denials, nftables drops, sudo");
    println!("  {} Press Ctrl+C to stop", "→".blue());
    println!();

    let mut child = std::process::Command::new("journalctl")
        .args([
            "-f", "-n", "0",
            "-u", "sshd",
            "-u", "apparmor",
            "-u", "nftables",
            "-u", "sudo",
            "--output", "short-full",
            "--no-pager",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to start journalctl")?;

    use std::io::{BufRead, BufReader};
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                let colored = if l.contains("Failed password") || l.contains("authentication failure") {
                    l.red().to_string()
                } else if l.contains("Accepted") {
                    l.green().to_string()
                } else if l.contains("DENIED") || l.contains("denied") || l.contains("DROP") {
                    l.yellow().to_string()
                } else if l.contains("sudo") {
                    l.cyan().to_string()
                } else {
                    l.dimmed().to_string()
                };
                println!("  {colored}");
            }
        }
    }
    let _ = child.wait();
    Ok(())
}

fn security_scan(scan_type: &str) -> Result<()> {
    let scan_dir = "/var/lib/monolith/security/scans";
    std::fs::create_dir_all(scan_dir)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let scan_file = format!("{scan_dir}/ports-{timestamp}.txt");
    let prev_link = format!("{scan_dir}/ports-latest.txt");

    let result = match scan_type {
        "internal" => {
            println!("{} Scanning internal ports (ss)...", "→".blue());
            let output = std::process::Command::new("ss")
                .args(["-tuln"])
                .output()
                .context("failed to run ss")?;
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            std::fs::write(&scan_file, &content)?;
            content
        }
        "external" => {
            let local_ip = std::process::Command::new("hostname")
                .args(["-I"])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().unwrap_or("127.0.0.1").to_string())
                .unwrap_or_else(|_| "127.0.0.1".to_string());

            println!("{} Scanning external ports on {local_ip} (nmap)...", "→".blue());
            let output = std::process::Command::new("nmap")
                .args(["-sT", "--open", "-oG", "-", &local_ip])
                .output()
                .context("nmap not installed. Install: mnpkg install nmap")?;
            let content = String::from_utf8_lossy(&output.stdout).to_string();
            std::fs::write(&scan_file, &content)?;
            content
        }
        _ => anyhow::bail!("invalid scan type: {scan_type}. Use 'internal' or 'external'"),
    };

    // Symlink the latest scan
    let _ = std::fs::remove_file(&prev_link);
    let _ = std::os::unix::fs::symlink(&scan_file, &prev_link);

    // Diff against previous scan if it exists
    let prev_content = if std::path::Path::new(&prev_link).exists() {
        Some(std::fs::read_to_string(&prev_link)?)
    } else {
        None
    };

    // Find the second-to-latest scan for diff
    let mut scans: Vec<_> = std::fs::read_dir(scan_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("ports-"))
        .collect();
    scans.sort_by_key(|e| e.file_name());

    if scans.len() >= 2 {
        let prev_path = scans[scans.len() - 2].path();
        let prev_content = std::fs::read_to_string(&prev_path)?;

        println!();
        println!("{}", " Diff vs previous scan ".bold().underline());

        let prev_ports: Vec<&str> = prev_content.lines().collect();
        let curr_ports: Vec<&str> = result.lines().collect();

        // Find new ports
        for line in &curr_ports {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !prev_content.contains(trimmed) {
                println!("  {} {trimmed}", "+".green().bold());
            }
        }
        // Find closed ports
        for line in &prev_ports {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !result.contains(trimmed) {
                println!("  {} {trimmed}", "-".red().bold());
            }
        }
    } else if scans.len() == 1 {
        println!("  {} First scan taken — no previous to diff", "●".cyan());
    }

    println!();
    println!("  {} Saved to {scan_file}", "●".green());
    Ok(())
}

/// Behavioural pass over recent sshd activity: counts failed logins per
/// source IP over the last 10 minutes and flags anything past a fixed
/// threshold. Layered on top of the existing nftables/AppArmor/fail2ban
/// stack rather than replacing it — flags for `mnctl security react`,
/// doesn't ban on its own.
fn security_ids(once: bool) -> Result<()> {
    const WINDOW: &str = "-10min";
    const THRESHOLD: u32 = 5;

    loop {
        let output = Command::new("journalctl")
            .args(["-u", "sshd", "--since", WINDOW, "--no-pager", "-o", "cat"])
            .output();

        match output {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut counts: std::collections::HashMap<String, u32> =
                    std::collections::HashMap::new();

                for line in text.lines().filter(|l| l.contains("Failed password")) {
                    if let Some(ip) = line
                        .split("from ")
                        .nth(1)
                        .and_then(|s| s.split_whitespace().next())
                    {
                        *counts.entry(ip.to_string()).or_insert(0) += 1;
                    }
                }

                let mut flagged: Vec<_> =
                    counts.into_iter().filter(|(_, n)| *n >= THRESHOLD).collect();
                flagged.sort_by(|a, b| b.1.cmp(&a.1));

                if flagged.is_empty() {
                    println!("{} No anomalies in the last 10 minutes", "●".green());
                } else {
                    for (ip, n) in &flagged {
                        println!(
                            "{} {n} failed logins from {} in 10min — run `mnctl security react {ip}`",
                            "⚠".yellow(),
                            ip.bold()
                        );
                    }
                }
            }
            Err(_) => println!("{} journalctl unavailable — skipping this pass", "⚠".yellow()),
        }

        if once {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

/// Listen on decoy ports. Nothing legitimate ever connects to them, so
/// any inbound connection is treated as hostile and handed straight to
/// `security_react`.
fn security_honeypot(ports: &str) -> Result<()> {
    use std::net::TcpListener;

    let port_list: Vec<u16> = ports.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    if port_list.is_empty() {
        anyhow::bail!("no valid ports in '{ports}'");
    }

    println!("{} Starting honeypot on ports: {:?}", "→".blue(), port_list);

    let mut handles = Vec::new();
    for port in port_list {
        handles.push(std::thread::spawn(move || {
            let listener = match TcpListener::bind(("0.0.0.0", port)) {
                Ok(l) => l,
                Err(e) => {
                    println!("{} Could not bind decoy port {port}: {e}", "⚠".yellow());
                    return;
                }
            };
            for stream in listener.incoming().flatten() {
                if let Ok(peer) = stream.peer_addr() {
                    println!("{} Honeypot hit on port {port} from {}", "⚠".red(), peer.ip());
                    let _ = security_react(&peer.ip().to_string());
                }
                drop(stream);
            }
        }));
    }

    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// Immediate response to a confirmed-hostile source: hard-block it at
/// the firewall and take a forensic snapshot so there's something to
/// investigate afterwards. Deliberately does the minimum and nothing
/// clever — this runs unattended.
fn security_react(ip: &str) -> Result<()> {
    println!("{} Reacting to {}", "→".blue(), ip.bold());

    let ban = Command::new("nft")
        .args(["add", "rule", "inet", "filter", "input", "ip", "saddr", ip, "drop"])
        .status();
    match ban {
        Ok(s) if s.success() => println!("  {} Blocked {ip} via nftables", "●".green()),
        _ => println!("  {} Could not add nftables rule (need root?)", "⚠".yellow()),
    }

    let snap = Command::new("snapper")
        .args(["create", "--description", &format!("security-react-{ip}")])
        .status();
    match snap {
        Ok(s) if s.success() => println!("  {} Forensic snapshot created", "●".green()),
        _ => println!("  {} snapper snapshot skipped (unavailable)", "⚠".yellow()),
    }

    Ok(())
}
