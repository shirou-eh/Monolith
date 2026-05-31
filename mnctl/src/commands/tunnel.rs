use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct TunnelArgs {
    #[command(subcommand)]
    command: TunnelCommand,
}

#[derive(Subcommand)]
enum TunnelCommand {
    /// playit.gg tunnel — expose local ports through playit.gg
    Playit {
        /// Local port to expose
        port: Option<u16>,
        /// Authentication secret (from https://playit.gg/account)
        #[arg(long)]
        secret: Option<String>,
    },
    /// Cloudflare Tunnel (cloudflared) — expose services via Cloudflare
    Cloudflare {
        /// Local service address (e.g. "localhost:8080")
        address: Option<String>,
        /// Cloudflare tunnel token
        #[arg(long)]
        token: Option<String>,
    },
    /// Show active tunnel status
    Status,
    /// Stop a running tunnel
    Stop,
    /// Show tunnel logs
    Logs,
}

impl TunnelArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            TunnelCommand::Playit { port, secret } => setup_playit(port, secret).await,
            TunnelCommand::Cloudflare { address, token } => setup_cloudflare(address, token).await,
            TunnelCommand::Status => tunnel_status(),
            TunnelCommand::Stop => tunnel_stop(),
            TunnelCommand::Logs => tunnel_logs(),
        }
    }
}

/// Install playit.gg agent if missing, then configure & start.
async fn setup_playit(port: Option<u16>, secret: Option<String>) -> Result<()> {
    // Check / install playit binary
    if !has_binary("playit") {
        println!("{} playit.gg agent not found — installing...", "→".blue());
        let arch = std::env::consts::ARCH;
        let url = match arch {
            "x86_64" => "https://github.com/playit-cloud/playit-agent/releases/latest/download/playit-linux-amd64",
            "aarch64" => "https://github.com/playit-cloud/playit-agent/releases/latest/download/playit-linux-aarch64",
            _ => anyhow::bail!("unsupported architecture: {arch}"),
        };

        let status = Command::new("curl")
            .args(["-fsSL", "-o", "/usr/local/bin/playit", url])
            .status()
            .context("failed to download playit agent")?;

        if !status.success() {
            anyhow::bail!("failed to download playit agent");
        }

        Command::new("chmod")
            .args(["+x", "/usr/local/bin/playit"])
            .status()
            .context("failed to chmod playit")?;

        println!("  {} playit.gg agent installed", "✓".green());
    }

    // Write secret if provided
    if let Some(s) = secret {
        std::fs::create_dir_all("/etc/monolith")?;
        std::fs::write("/etc/monolith/playit-secret", &s)?;
        println!("  {} playit.gg secret saved", "✓".green());
    }

    // Create systemd service
    let secret_arg = if std::path::Path::new("/etc/monolith/playit-secret").exists() {
        " --secret-file /etc/monolith/playit-secret"
    } else {
        ""
    };

    let port_override = match port {
        Some(p) => format!(" --port {p}"),
        None => String::new(),
    };

    let service = format!(
        "[Unit]\n\
         Description=playit.gg Tunnel\n\
         After=network-online.target\n\
         Wants=network-online.target\n\n\
         [Service]\n\
         Type=simple\n\
         ExecStart=/usr/local/bin/playit{secret_arg}{port_override}\n\
         Restart=always\n\
         RestartSec=5\n\
         User=root\n\n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    );

    std::fs::write("/etc/systemd/system/playit-tunnel.service", &service)?;
    std::process::Command::new("systemctl")
        .args(["daemon-reload"])
        .status()?;
    std::process::Command::new("systemctl")
        .args(["enable", "--now", "playit-tunnel.service"])
        .status()?;

    println!();
    println!(
        "{} playit.gg tunnel started",
        "✓".green()
    );
    println!("  {} Status:  mnctl tunnel status", "●".cyan());
    println!("  {} Logs:    mnctl tunnel logs", "●".cyan());
    println!("  {} Web:     https://playit.gg/account/tunnels", "●".cyan());
    Ok(())
}

/// Install cloudflared if missing, then configure & start.
async fn setup_cloudflare(address: Option<String>, token: Option<String>) -> Result<()> {
    if !has_binary("cloudflared") {
        println!("{} cloudflared not found — installing...", "→".blue());
        let arch = std::env::consts::ARCH;

        // Use the official cloudflared install script
        let status = Command::new("curl")
            .args(["-fsSL", "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64"])
            .status()
            .context("failed to download cloudflared")?;

        let url = match arch {
            "x86_64" => "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64",
            "aarch64" => "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-arm64",
            _ => anyhow::bail!("unsupported architecture: {arch}"),
        };

        let status = Command::new("curl")
            .args(["-fsSL", "-o", "/usr/local/bin/cloudflared", url])
            .status()
            .context("failed to download cloudflared")?;

        if !status.success() {
            anyhow::bail!("failed to download cloudflared");
        }

        Command::new("chmod")
            .args(["+x", "/usr/local/bin/cloudflared"])
            .status()
            .context("failed to chmod cloudflared")?;

        println!("  {} cloudflared installed", "✓".green());
    }

    // If token is given, use cloudflared tunnel --no-tls-verify or standard
    if let Some(t) = token {
        std::fs::create_dir_all("/etc/monolith")?;
        std::fs::write("/etc/monolith/cloudflare-token", &t)?;
        println!("  {} Cloudflare tunnel token saved", "✓".green());
    }

    let addr = address.unwrap_or_else(|| "localhost:80".to_string());

    // Quick run to authenticate if token available
    if std::path::Path::new("/etc/monolith/cloudflare-token").exists() {
        let token_val = std::fs::read_to_string("/etc/monolith/cloudflare-token")?;
        let token_trimmed = token_val.trim().to_string();

        let service = format!(
            "[Unit]\n\
             Description=Cloudflare Tunnel\n\
             After=network-online.target\n\
             Wants=network-online.target\n\n\
             [Service]\n\
             Type=simple\n\
             ExecStart=/usr/local/bin/cloudflared tunnel --no-autoupdate run --token {token_trimmed}\n\
             Restart=always\n\
             RestartSec=5\n\
             User=root\n\n\
             [Install]\n\
             WantedBy=multi-user.target\n"
        );

        std::fs::write("/etc/systemd/system/cloudflare-tunnel.service", &service)?;
        std::process::Command::new("systemctl")
            .args(["daemon-reload"])
            .status()?;
        std::process::Command::new("systemctl")
            .args(["enable", "--now", "cloudflare-tunnel.service"])
            .status()?;

        println!();
        println!(
            "{} Cloudflare Tunnel started → {addr}",
            "✓".green()
        );
        println!("  {} Status:  mnctl tunnel status", "●".cyan());
        println!("  {} Logs:    mnctl tunnel logs", "●".cyan());
    } else {
        // No token — show instructions
        println!();
        println!("{} Cloudflare Tunnel setup required", "→".yellow());
        println!("  1. Go to https://dash.cloudflare.com/ -> Zero Trust -> Networks -> Tunnels");
        println!("  2. Create a tunnel and copy the token");
        println!("  3. Run: {} --token <your-token>", "mnctl tunnel cloudflare".bold());
        println!();
        println!("  Quick test (10 min): cloudflared tunnel --url http://{addr}");
    }

    Ok(())
}

fn tunnel_status() -> Result<()> {
    let tunnels: Vec<(&str, &str)> = vec![
        ("playit-tunnel", "playit.gg"),
        ("cloudflare-tunnel", "Cloudflare"),
    ];

    println!("{}", " Tunnel Status ".bold().underline());
    println!();

    for (name, label) in &tunnels {
        let output = Command::new("systemctl")
            .args(["is-active", name])
            .output();

        let status = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "not found".to_string(),
        };

        let colored = match status.as_str() {
            "active" => status.green(),
            "inactive" => status.yellow(),
            "failed" => status.red(),
            _ => status.dimmed(),
        };

        // Check if the service file exists
        let exists = std::path::Path::new(&format!("/etc/systemd/system/{name}.service")).exists();
        if !exists {
            println!("  {:<20} {} — not configured", label, "─".dimmed());
            continue;
        }

        println!("  {:<20} {}", label, colored);
    }

    Ok(())
}

fn tunnel_stop() -> Result<()> {
    for name in &["playit-tunnel", "cloudflare-tunnel"] {
        if std::path::Path::new(&format!("/etc/systemd/system/{name}.service")).exists() {
            let status = Command::new("systemctl")
                .args(["disable", "--now", name])
                .status();

            match status {
                Ok(s) if s.success() => {
                    println!("  {} {} stopped", "●".green(), name);
                }
                _ => {
                    println!("  {} {} not running", "●".yellow(), name);
                }
            }
        }
    }
    println!("{} All tunnels stopped", "●".green());
    Ok(())
}

fn tunnel_logs() -> Result<()> {
    // Try to show logs from whichever tunnel is active
    let tunnel_names = ["playit-tunnel", "cloudflare-tunnel"];
    let mut found = false;

    for name in &tunnel_names {
        let active = Command::new("systemctl")
            .args(["is-active", name])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
            .unwrap_or(false);

        if active {
            found = true;
            println!("{} Showing logs for {}:", "→".blue(), name);
            let status = std::process::Command::new("journalctl")
                .args(["-u", name, "-n", "50", "--no-pager", "-f"])
                .status()
                .context("failed to show logs")?;

            if !status.success() {
                println!("  {} journalctl failed for {name}", "●".red());
            }
            break;
        }
    }

    if !found {
        println!("{} No active tunnels found", "●".yellow());
        println!("  Start one with: mnctl tunnel playit <port>");
        println!("  Or:            mnctl tunnel cloudflare <address>");
    }

    Ok(())
}

fn has_binary(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
