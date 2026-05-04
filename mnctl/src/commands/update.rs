use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct UpdateArgs {
    #[command(subcommand)]
    command: UpdateCommand,
}

#[derive(Subcommand)]
enum UpdateCommand {
    /// Check for available updates
    Check,
    /// Apply available updates
    Apply {
        /// Only apply security updates
        #[arg(long)]
        security_only: bool,
        /// Perform a dry run without making changes
        #[arg(long)]
        dry_run: bool,
    },
    /// Update or rebuild the Monolith kernel
    Kernel {
        /// Specific kernel version to install
        #[arg(long)]
        version: Option<String>,
    },
    /// Roll back to a previous system state
    Rollback {
        /// Snapshot ID to roll back to
        #[arg(long)]
        to: Option<String>,
    },
    /// Show update history
    History,
    /// Show or edit the update schedule
    Schedule,
}

impl UpdateArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            UpdateCommand::Check => check_updates(),
            UpdateCommand::Apply {
                security_only,
                dry_run,
            } => apply_updates(security_only, dry_run),
            UpdateCommand::Kernel { version } => update_kernel(version.as_deref()),
            UpdateCommand::Rollback { to } => rollback(to.as_deref()),
            UpdateCommand::History => update_history(),
            UpdateCommand::Schedule => update_schedule(),
        }
    }
}

fn check_updates() -> Result<()> {
    println!("{}", "Checking for updates...".dimmed());
    let output = Command::new("pacman")
        .args(["-Sy", "--noconfirm"])
        .output()
        .context("failed to sync package databases")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{} Sync warning: {}", "●".yellow(), stderr.trim());
    }

    let check = Command::new("pacman")
        .args(["-Qu"])
        .output()
        .context("failed to check for updates")?;

    let stdout = String::from_utf8_lossy(&check.stdout);
    if stdout.trim().is_empty() {
        println!("{} System is up to date.", "●".green());
    } else {
        let count = stdout.lines().count();
        println!(
            "{} {} update(s) available:",
            "●".yellow(),
            count.to_string().bold()
        );
        println!();
        for line in stdout.lines() {
            println!("  {line}");
        }
    }
    Ok(())
}

fn apply_updates(security_only: bool, dry_run: bool) -> Result<()> {
    if security_only {
        println!(
            "{}",
            "Security-only updates not yet implemented for Arch. Running full update.".yellow()
        );
    }

    // Create snapshot before update
    println!("{} Creating pre-update snapshot...", "→".blue());
    let snap = Command::new("snapper")
        .args(["create", "--description", "pre-update", "--type", "pre"])
        .output();

    match snap {
        Ok(o) if o.status.success() => {
            println!("  {} Snapshot created", "●".green());
        }
        _ => {
            println!(
                "  {} Snapper not available, skipping snapshot",
                "●".yellow()
            );
        }
    }

    if dry_run {
        println!("{}", "Dry run — no changes will be made.".dimmed());
        let output = Command::new("pacman")
            .args(["-Syu", "--noconfirm", "--print-only"])
            .output()
            .context("failed to dry-run update")?;

        print!("{}", String::from_utf8_lossy(&output.stdout));
        return Ok(());
    }

    println!("{} Applying updates...", "→".blue());
    let status = Command::new("pacman")
        .args(["-Syu", "--noconfirm"])
        .status()
        .context("failed to apply updates")?;

    if status.success() {
        println!("{} System updated successfully", "●".green());

        // Create post-update snapshot
        let _ = Command::new("snapper")
            .args(["create", "--description", "post-update", "--type", "post"])
            .output();
    } else {
        anyhow::bail!("update failed — consider rolling back with: mnctl update rollback");
    }
    Ok(())
}

fn update_kernel(version: Option<&str>) -> Result<()> {
    let build_script = "/usr/share/monolith/kernel/build.sh";

    if !std::path::Path::new(build_script).exists() {
        println!(
            "{}",
            "Kernel build script not found. Using packaged kernel update.".yellow()
        );
        let status = Command::new("pacman")
            .args(["-S", "--noconfirm", "monolith-kernel"])
            .status()
            .context("failed to update kernel package")?;

        if status.success() {
            println!("{} Kernel updated. Reboot required.", "●".green());
        }
        return Ok(());
    }

    let mut args = vec![build_script.to_string()];
    if let Some(v) = version {
        args.push(format!("--version={v}"));
    }

    println!("{} Building kernel...", "→".blue());
    let status = Command::new("bash")
        .args(&args)
        .status()
        .context("failed to build kernel")?;

    if status.success() {
        println!(
            "{} Kernel built and installed. Reboot required.",
            "●".green()
        );
    } else {
        anyhow::bail!("kernel build failed — check /var/log/monolith-kernel-build.log");
    }
    Ok(())
}

fn rollback(snapshot_id: Option<&str>) -> Result<()> {
    match snapshot_id {
        Some(id) => {
            println!("{} Rolling back to snapshot {}...", "→".blue(), id.bold());
            let status = Command::new("snapper")
                .args(["undochange", id])
                .status()
                .with_context(|| format!("failed to roll back to snapshot {id}"))?;

            if status.success() {
                println!(
                    "{} Rolled back to snapshot {}. Reboot recommended.",
                    "●".green(),
                    id
                );
            } else {
                anyhow::bail!("rollback to snapshot {id} failed");
            }
        }
        None => {
            println!("{}", "Available snapshots:".bold().underline());
            let output = Command::new("snapper")
                .args(["list"])
                .output()
                .context("failed to list snapshots")?;

            print!("{}", String::from_utf8_lossy(&output.stdout));
            println!();
            println!("Use: {} update rollback --to <ID>", "mnctl".bold());
        }
    }
    Ok(())
}

fn update_history() -> Result<()> {
    let log_path = "/var/log/pacman.log";
    if std::path::Path::new(log_path).exists() {
        let output = Command::new("tail")
            .args(["-n", "50", log_path])
            .output()
            .context("failed to read pacman log")?;

        println!("{}", "Recent Package Operations:".bold().underline());
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        println!("{}", "No update history available.".dimmed());
    }
    Ok(())
}

fn update_schedule() -> Result<()> {
    let config_path = "/etc/monolith/update.toml";
    if std::path::Path::new(config_path).exists() {
        let content =
            std::fs::read_to_string(config_path).context("failed to read update config")?;
        println!("{}", "Update Schedule:".bold().underline());
        println!("{content}");
    } else {
        println!("{}", "No update schedule configured.".dimmed());
        println!("Create one at: {}", config_path.bold());
    }
    Ok(())
}
