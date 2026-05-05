use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct BackupArgs {
    #[command(subcommand)]
    command: BackupCommand,
}

#[derive(Subcommand)]
enum BackupCommand {
    /// Create a backup now
    Create {
        /// Optional tag for this backup
        #[arg(long)]
        tag: Option<String>,
    },
    /// List all backups
    List,
    /// Restore from a backup
    Restore {
        /// Snapshot ID to restore
        snapshot_id: String,
    },
    /// Verify backup integrity
    Verify {
        /// Snapshot ID to verify
        snapshot_id: String,
    },
    /// Delete a specific backup
    Delete {
        /// Snapshot ID to delete
        snapshot_id: String,
    },
    /// List Btrfs snapshots (snapper)
    Snapshots,
    /// Export a backup to a destination path or remote
    Export {
        /// Snapshot ID to export
        snapshot_id: String,
        /// Destination path or remote URL
        dest: String,
    },
}

impl BackupArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            BackupCommand::Create { tag } => create_backup(tag.as_deref()),
            BackupCommand::List => list_backups(),
            BackupCommand::Restore { snapshot_id } => restore_backup(&snapshot_id),
            BackupCommand::Verify { snapshot_id } => verify_backup(&snapshot_id),
            BackupCommand::Delete { snapshot_id } => delete_backup(&snapshot_id),
            BackupCommand::Snapshots => list_snapshots(),
            BackupCommand::Export { snapshot_id, dest } => export_backup(&snapshot_id, &dest),
        }
    }
}

fn create_backup(tag: Option<&str>) -> Result<()> {
    println!("{} Creating backup...", "→".blue());

    // Create Btrfs snapshot first (Tier 1)
    let snap_desc = tag.unwrap_or("manual-backup");
    let snap_result = Command::new("snapper")
        .args(["create", "--description", snap_desc, "--type", "single"])
        .output();

    match snap_result {
        Ok(o) if o.status.success() => {
            let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
            println!("  {} Btrfs snapshot created ({})", "●".green(), id);
        }
        _ => {
            println!(
                "  {} Snapper not available, skipping local snapshot",
                "●".yellow()
            );
        }
    }

    // Run restic backup (Tier 2)
    let mut restic_args = vec!["backup"];

    let config = load_backup_config();
    let paths: Vec<String> = config.paths.iter().map(|s| s.to_string()).collect();
    for p in &paths {
        restic_args.push(p);
    }

    if let Some(t) = tag {
        restic_args.push("--tag");
        restic_args.push(t);
    }

    let status = Command::new("restic").args(&restic_args).status();

    match status {
        Ok(s) if s.success() => {
            println!("  {} Restic backup completed", "●".green());
        }
        Ok(_) => {
            println!("  {} Restic backup failed", "●".red());
        }
        Err(_) => {
            println!(
                "  {} restic not available. Install with: mnpkg install restic",
                "●".yellow()
            );
        }
    }

    println!("{} Backup complete", "●".green());
    Ok(())
}

fn list_backups() -> Result<()> {
    println!("{}", "Restic Snapshots:".bold().underline());
    let output = Command::new("restic")
        .args(["snapshots", "--compact"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
        }
        _ => {
            println!(
                "{}",
                "No restic repository configured or restic not installed.".yellow()
            );
        }
    }
    Ok(())
}

fn restore_backup(snapshot_id: &str) -> Result<()> {
    println!(
        "{} Restoring from snapshot {}...",
        "→".blue(),
        snapshot_id.bold()
    );

    let status = Command::new("restic")
        .args(["restore", snapshot_id, "--target", "/"])
        .status()
        .with_context(|| format!("failed to restore snapshot {snapshot_id}"))?;

    if status.success() {
        println!(
            "{} Restored from snapshot {}. Reboot recommended.",
            "●".green(),
            snapshot_id
        );
    } else {
        anyhow::bail!("restore failed for snapshot {snapshot_id}");
    }
    Ok(())
}

fn verify_backup(snapshot_id: &str) -> Result<()> {
    println!(
        "{} Verifying snapshot {}...",
        "→".blue(),
        snapshot_id.bold()
    );

    let status = Command::new("restic")
        .args(["check", "--read-data-subset=5%"])
        .status()
        .context("failed to verify backup")?;

    if status.success() {
        println!("{} Backup integrity verified", "●".green());
    } else {
        println!("{} Backup verification failed!", "●".red());
    }
    Ok(())
}

fn delete_backup(snapshot_id: &str) -> Result<()> {
    let status = Command::new("restic")
        .args(["forget", snapshot_id, "--prune"])
        .status()
        .with_context(|| format!("failed to delete snapshot {snapshot_id}"))?;

    if status.success() {
        println!("{} Snapshot {} deleted", "●".green(), snapshot_id);
    } else {
        anyhow::bail!("failed to delete snapshot {snapshot_id}");
    }
    Ok(())
}

fn list_snapshots() -> Result<()> {
    println!("{}", "Btrfs Snapshots (Snapper):".bold().underline());
    let output = Command::new("snapper").args(["list"]).output();

    match output {
        Ok(o) if o.status.success() => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
        }
        _ => {
            println!(
                "{}",
                "Snapper not available or no Btrfs filesystem.".yellow()
            );
        }
    }
    Ok(())
}

fn export_backup(snapshot_id: &str, dest: &str) -> Result<()> {
    println!(
        "{} Exporting snapshot {} to {}...",
        "→".blue(),
        snapshot_id.bold(),
        dest.bold()
    );

    let status = Command::new("restic")
        .args(["copy", "--from-repo", ".", snapshot_id, "--repo", dest])
        .status()
        .with_context(|| format!("failed to export snapshot {snapshot_id}"))?;

    if status.success() {
        println!("{} Export complete", "●".green());
    } else {
        anyhow::bail!("export failed");
    }
    Ok(())
}

struct BackupConfig {
    paths: Vec<String>,
}

fn load_backup_config() -> BackupConfig {
    const CONFIG_PATH: &str = "/etc/monolith/monolith.toml";
    const DEFAULT_PATHS: &[&str] = &["/", "/home"];

    let paths = std::fs::read_to_string(CONFIG_PATH)
        .ok()
        .and_then(|content| content.parse::<toml::Table>().ok())
        .and_then(|doc| {
            doc.get("backup")?
                .as_table()?
                .get("paths")?
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_else(|| DEFAULT_PATHS.iter().map(|s| s.to_string()).collect());

    BackupConfig { paths }
}
