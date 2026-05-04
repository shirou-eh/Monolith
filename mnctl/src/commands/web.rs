//! Web management UI launcher.
//!
//! Looks for the `mnweb` binary on PATH (or in well-known install locations)
//! and either runs it in the foreground or registers a systemd unit to keep
//! it running.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::PathBuf;
use std::process::Command;

#[derive(Args)]
pub struct WebArgs {
    #[command(subcommand)]
    command: WebCommand,
}

#[derive(Subcommand)]
enum WebCommand {
    /// Run the web UI in the foreground
    Run {
        /// Bind address (default: 127.0.0.1:9911)
        #[arg(long, default_value = "127.0.0.1:9911")]
        bind: String,
    },
    /// Install a systemd unit and start the web UI as a service
    Enable {
        /// Bind address (default: 127.0.0.1:9911)
        #[arg(long, default_value = "127.0.0.1:9911")]
        bind: String,
    },
    /// Stop and disable the systemd unit
    Disable,
    /// Show systemd service status
    Status,
    /// Print URL the web UI is reachable at
    Url,
}

const SERVICE_PATH: &str = "/etc/systemd/system/monolith-mnweb.service";

impl WebArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            WebCommand::Run { bind } => run_foreground(&bind),
            WebCommand::Enable { bind } => enable(&bind),
            WebCommand::Disable => disable(),
            WebCommand::Status => status(),
            WebCommand::Url => url(),
        }
    }
}

fn locate_mnweb() -> Result<PathBuf> {
    let candidate = if let Ok(p) = which::which("mnweb") {
        p
    } else {
        let mut found = None;
        for c in [
            "/usr/local/bin/mnweb",
            "/usr/bin/mnweb",
            "./target/release/mnweb",
            "./target/debug/mnweb",
        ] {
            let p = PathBuf::from(c);
            if p.exists() {
                found = Some(p);
                break;
            }
        }
        found.ok_or_else(|| {
            anyhow::anyhow!(
                "mnweb binary not found. Build it with `cargo build --release -p mnweb` or install via `make install`."
            )
        })?
    };
    // Always return an absolute path. systemd's ExecStart= rejects
    // relative paths like ./target/release/mnweb, so we canonicalise the
    // result before handing it back.
    Ok(std::fs::canonicalize(&candidate).unwrap_or(candidate))
}

fn run_foreground(bind: &str) -> Result<()> {
    let bin = locate_mnweb()?;
    println!(
        "{} Starting mnweb on {} (Ctrl+C to stop)",
        "→".blue(),
        bind.bold()
    );
    let status = Command::new(&bin)
        .args(["--bind", bind])
        .status()
        .with_context(|| format!("failed to exec {}", bin.display()))?;
    if !status.success() {
        anyhow::bail!("mnweb exited {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

fn enable(bind: &str) -> Result<()> {
    let bin = locate_mnweb()?;
    let unit = format!(
        "[Unit]\n\
         Description=Monolith OS web management UI (mnweb)\n\
         After=network.target\n\
         \n\
         [Service]\n\
         ExecStart={bin} --bind {bind}\n\
         Restart=on-failure\n\
         RestartSec=3\n\
         User=root\n\
         AmbientCapabilities=\n\
         NoNewPrivileges=true\n\
         ProtectSystem=strict\n\
         ProtectHome=true\n\
         PrivateTmp=true\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        bin = bin.display(),
        bind = bind,
    );

    std::fs::write(SERVICE_PATH, unit).context("failed to write systemd unit (run as root?)")?;
    let _ = Command::new("systemctl").args(["daemon-reload"]).status();
    let status = Command::new("systemctl")
        .args(["enable", "--now", "monolith-mnweb.service"])
        .status()
        .context("failed to enable monolith-mnweb.service")?;
    if !status.success() {
        anyhow::bail!("systemctl enable returned {}", status.code().unwrap_or(-1));
    }
    println!(
        "{} mnweb installed and running, listening on {}",
        "●".green(),
        bind.bold()
    );
    println!("  systemctl status monolith-mnweb.service");
    Ok(())
}

fn disable() -> Result<()> {
    let _ = Command::new("systemctl")
        .args(["disable", "--now", "monolith-mnweb.service"])
        .status();
    if std::path::Path::new(SERVICE_PATH).exists() {
        std::fs::remove_file(SERVICE_PATH).context("failed to remove unit file")?;
        let _ = Command::new("systemctl").args(["daemon-reload"]).status();
    }
    println!("{} mnweb disabled", "●".green());
    Ok(())
}

fn status() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["status", "monolith-mnweb.service", "--no-pager"])
        .status()
        .context("failed to query systemctl")?;
    if !status.success() {
        return Err(anyhow::anyhow!(
            "systemctl exited {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn url() -> Result<()> {
    if let Ok(content) = std::fs::read_to_string(SERVICE_PATH) {
        for line in content.lines() {
            if let Some(rest) = line.trim().strip_prefix("ExecStart=") {
                if let Some(idx) = rest.find("--bind") {
                    let after = &rest[idx + "--bind".len()..];
                    if let Some(addr) = after.split_whitespace().next() {
                        println!("http://{addr}/");
                        return Ok(());
                    }
                }
            }
        }
    }
    println!("http://127.0.0.1:9911/");
    Ok(())
}
