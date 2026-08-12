use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
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
    /// Self-update Monolith OS from GitHub releases
    SelfUpdate {
        /// Force reinstall even if same version
        #[arg(long)]
        force: bool,
        /// Specific version tag to install (e.g. "v1.2.0")
        #[arg(long)]
        version: Option<String>,
    },
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
            UpdateCommand::SelfUpdate { force, version } => {
                self_update(force, version.as_deref()).await
            }
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

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// Self-update Monolith OS from the latest GitHub release.
async fn self_update(force: bool, version: Option<&str>) -> Result<()> {
    let repo = "shirou-eh/Monolith";
    let client = reqwest::Client::builder()
        .user_agent("mnctl")
        .build()
        .context("failed to build HTTP client")?;

    // Always fetch the release object (by tag if one was given, latest
    // otherwise) rather than only doing this for "latest" and then
    // hand-guessing the asset filename for an explicit --version. The
    // release's own `assets` list is authoritative about what actually
    // got uploaded — a guessed name is one rename-on-release away from
    // a self-update that silently 404s.
    println!("{} Checking GitHub release...", "→".blue());
    let api_url = match version {
        Some(v) => format!(
            "https://api.github.com/repos/{repo}/releases/tags/v{}",
            v.trim_start_matches('v')
        ),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };
    let resp = client
        .get(&api_url)
        .send()
        .await
        .context("failed to fetch release from GitHub")?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "GitHub API returned {} for {api_url} — check your network / rate limit / that this version was released",
            resp.status()
        );
    }
    let release: GithubRelease = resp
        .json()
        .await
        .context("failed to parse GitHub release JSON")?;
    let tag = release.tag_name.trim_start_matches('v').to_string();

    let current = env!("CARGO_PKG_VERSION");

    if tag == current && !force {
        println!("{} Already up to date (v{})", "●".green(), current.bold());
        return Ok(());
    }

    if tag == current && force {
        println!("{} Force reinstall v{}", "→".blue(), current.bold());
    } else {
        println!(
            "{} Updating {} v{} → v{}",
            "→".blue(),
            "Monolith".bold(),
            current,
            tag.bold()
        );
        let summary: String = release.body.lines().take(3).collect::<Vec<_>>().join(" ");
        if !summary.trim().is_empty() {
            println!("  {} {}", "→".blue(), summary.trim().dimmed());
        }
    }

    // Match against the release's real asset list instead of guessing a
    // filename — see the comment above for why.
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let expected_infix = format!("{arch}-unknown-{os}-gnu");
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(&expected_infix) && a.name.ends_with(".tar.gz"))
        .ok_or_else(|| {
            let available: Vec<&str> = release.assets.iter().map(|a| a.name.as_str()).collect();
            anyhow::anyhow!(
                "no release asset matching '*{expected_infix}*.tar.gz' in v{tag}. Available assets: {}",
                if available.is_empty() { "(none)".to_string() } else { available.join(", ") }
            )
        })?;

    println!("  {} Downloading {}...", "↓".cyan(), asset.name);

    let resp = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .with_context(|| format!("failed to download {}", asset.name))?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "download failed with HTTP {} for {}",
            resp.status(),
            asset.browser_download_url
        );
    }

    let bytes = resp.bytes().await.context("failed to read response body")?;

    // Verify against the Monolith release signing key before this
    // touches disk anywhere real — same reasoning as mnpkg's copy of
    // this (see monolith-sign's crate docs): TLS covers the wire, this
    // covers a compromised or tampered release artifact, which matters
    // a lot more for something about to be extracted into
    // /usr/local/bin and run as root. Missing .sig (pre-signing
    // releases) warns and continues; a present-but-broken .sig always
    // aborts.
    let sig_name = format!("{}.sig", asset.name);
    match release.assets.iter().find(|a| a.name == sig_name) {
        Some(sig_asset) => {
            println!("  {} Verifying signature ({})...", "→".blue(), sig_name);
            let sig_resp = client
                .get(&sig_asset.browser_download_url)
                .send()
                .await
                .with_context(|| format!("failed to download {sig_name}"))?;
            if !sig_resp.status().is_success() {
                anyhow::bail!(
                    "signature download failed with HTTP {} for {sig_name}",
                    sig_resp.status()
                );
            }
            let sig_bytes = sig_resp
                .bytes()
                .await
                .context("failed to read signature response body")?;
            monolith_sign::verify_detached(&bytes, &sig_bytes)
                .context("refusing to install an unverified release artifact")?;
            println!("  {} Signature verified", "✓".green());
        }
        None => {
            println!(
                "  {} No signature published for {} — proceeding unverified (release predates signing)",
                "⚠".yellow(),
                asset.name
            );
        }
    }

    let tmp = "/tmp/monolith-update.tar.gz";
    std::fs::write(tmp, &bytes).context("failed to write tarball to /tmp")?;

    // Extract tarball and install binaries
    let install_dir = "/usr/local/bin";
    println!("  {} Extracting to {}...", "→".blue(), install_dir);

    // Direct argv, not `sh -c` — matches mnpkg's copy of this same
    // self-update flow. tmp/install_dir are fixed constants so there's
    // no injection risk either way, but one exec pattern to audit
    // instead of two.
    let output = Command::new("tar")
        .args(["-xzf", tmp, "-C", install_dir])
        .output()
        .context("failed to extract tarball")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("extraction failed: {stderr}");
    }

    let _ = std::fs::remove_file(tmp);

    // Update config version field
    let config_path = "/etc/monolith/monolith.toml";
    if std::path::Path::new(config_path).exists() {
        if let Ok(content) = std::fs::read_to_string(config_path) {
            if let Ok(mut doc) = content.parse::<toml::Value>() {
                if let Some(table) = doc.as_table_mut() {
                    table.insert("version".to_string(), toml::Value::String(tag.clone()));
                }
                if let Ok(serialized) = toml::to_string_pretty(&doc) {
                    let _ = std::fs::write(config_path, &serialized);
                }
            }
        }
    }

    println!();
    println!(
        "{} Monolith v{} installed to {}",
        "✓".green(),
        tag.bold(),
        install_dir
    );

    if version.is_none() && tag != current {
        println!(
            "{} Release notes: https://github.com/{repo}/releases/tag/v{tag}",
            "≡".blue()
        );
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
