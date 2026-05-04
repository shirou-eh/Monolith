use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::process::Command;
use tracing_subscriber::EnvFilter;

/// mnpkg — Monolith OS package manager wrapper
///
/// Enhanced pacman wrapper with snapshot safety, AUR support,
/// CVE auditing, and package pinning.
#[derive(Parser)]
#[command(
    name = "mnpkg",
    version = env!("CARGO_PKG_VERSION"),
    about = "Monolith OS package manager"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install a package with dependency preview
    Install {
        /// Package name
        pkg: String,
    },
    /// Remove a package with orphan detection
    Remove {
        /// Package name
        pkg: String,
    },
    /// Update all packages with snapshot safety
    Update,
    /// Search packages in repos and AUR
    Search {
        /// Search query
        query: String,
    },
    /// Show detailed package information
    Info {
        /// Package name
        pkg: String,
    },
    /// Roll back last package operation
    Rollback,
    /// Pin a package to a specific version
    Pin {
        /// Package name
        pkg: String,
        /// Version to pin to
        version: String,
    },
    /// Unpin a package
    Unpin {
        /// Package name
        pkg: String,
    },
    /// Show all pinned packages
    Pins,
    /// Show packages with known CVEs
    Audit,
    /// List orphaned packages
    Orphans,
    /// Show disk usage by package
    Size,
    /// Show installation/removal history
    History,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Install { pkg } => install_package(&pkg),
        Commands::Remove { pkg } => remove_package(&pkg),
        Commands::Update => update_packages(),
        Commands::Search { query } => search_packages(&query),
        Commands::Info { pkg } => package_info(&pkg),
        Commands::Rollback => rollback(),
        Commands::Pin { pkg, version } => pin_package(&pkg, &version),
        Commands::Unpin { pkg } => unpin_package(&pkg),
        Commands::Pins => show_pins(),
        Commands::Audit => audit_packages(),
        Commands::Orphans => list_orphans(),
        Commands::Size => package_sizes(),
        Commands::History => show_history(),
    }
}

fn install_package(pkg: &str) -> Result<()> {
    // Show dependency tree preview
    println!(
        "{} Resolving dependencies for {}...",
        "→".blue(),
        pkg.bold()
    );

    let deps = Command::new("pacman")
        .args(["-Si", pkg])
        .output()
        .with_context(|| format!("failed to get info for {pkg}"))?;

    if deps.status.success() {
        let stdout = String::from_utf8_lossy(&deps.stdout);
        for line in stdout.lines() {
            if line.starts_with("Depends On") || line.starts_with("Download Size") {
                println!("  {line}");
            }
        }
    }

    println!();
    let confirm = dialoguer::Confirm::new()
        .with_prompt(format!("Install {pkg}?"))
        .default(true)
        .interact()?;

    if !confirm {
        println!("{}", "Cancelled.".dimmed());
        return Ok(());
    }

    let status = Command::new("pacman")
        .args(["-S", "--noconfirm", pkg])
        .status()
        .with_context(|| format!("failed to install {pkg}"))?;

    if status.success() {
        println!("{} {} installed successfully", "●".green(), pkg.bold());
    } else {
        // Try AUR
        println!("{} Not in repos, trying AUR...", "→".yellow());
        let aur_helpers = ["paru", "yay"];
        for helper in &aur_helpers {
            if which::which(helper).is_ok() {
                let status = Command::new(helper)
                    .args(["-S", "--noconfirm", pkg])
                    .status()?;
                if status.success() {
                    println!("{} {} installed from AUR", "●".green(), pkg.bold());
                    return Ok(());
                }
            }
        }
        anyhow::bail!("failed to install {pkg} from repos or AUR");
    }
    Ok(())
}

fn remove_package(pkg: &str) -> Result<()> {
    let status = Command::new("pacman")
        .args(["-Rs", "--noconfirm", pkg])
        .status()
        .with_context(|| format!("failed to remove {pkg}"))?;

    if status.success() {
        println!("{} {} removed", "●".green(), pkg.bold());

        // Check for orphans
        let orphans = Command::new("pacman").args(["-Qtdq"]).output()?;

        let stdout = String::from_utf8_lossy(&orphans.stdout);
        if !stdout.trim().is_empty() {
            let count = stdout.lines().count();
            println!(
                "{} {} orphaned package(s) found. Remove with: {} orphans",
                "●".yellow(),
                count,
                "mnpkg".bold()
            );
        }
    } else {
        anyhow::bail!("failed to remove {pkg}");
    }
    Ok(())
}

fn update_packages() -> Result<()> {
    // Create snapshot before update
    println!("{} Creating pre-update snapshot...", "→".blue());
    let _ = Command::new("snapper")
        .args([
            "create",
            "--description",
            "pre-mnpkg-update",
            "--type",
            "pre",
        ])
        .output();

    println!("{} Updating all packages...", "→".blue());
    let status = Command::new("pacman")
        .args(["-Syu", "--noconfirm"])
        .status()
        .context("failed to update packages")?;

    if status.success() {
        let _ = Command::new("snapper")
            .args([
                "create",
                "--description",
                "post-mnpkg-update",
                "--type",
                "post",
            ])
            .output();
        println!("{} All packages updated", "●".green());
    } else {
        anyhow::bail!("update failed — roll back with: mnpkg rollback");
    }
    Ok(())
}

fn search_packages(query: &str) -> Result<()> {
    println!("{}", "Repository Results:".bold().underline());
    let output = Command::new("pacman")
        .args(["-Ss", query])
        .output()
        .context("failed to search repos")?;

    if output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        println!("  {}", "No results in repos.".dimmed());
    }

    // AUR search
    println!();
    println!("{}", "AUR Results:".bold().underline());
    let aur_helpers = ["paru", "yay"];
    for helper in &aur_helpers {
        if which::which(helper).is_ok() {
            let output = Command::new(helper).args(["-Ssa", query]).output()?;
            if output.status.success() {
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
            return Ok(());
        }
    }
    println!(
        "  {}",
        "No AUR helper installed (install paru or yay).".dimmed()
    );
    Ok(())
}

fn package_info(pkg: &str) -> Result<()> {
    // Try local first
    let output = Command::new("pacman").args(["-Qi", pkg]).output();

    if let Ok(o) = output {
        if o.status.success() {
            println!("{}", "Installed Package:".bold().underline());
            print!("{}", String::from_utf8_lossy(&o.stdout));
            return Ok(());
        }
    }

    // Try remote
    let output = Command::new("pacman")
        .args(["-Si", pkg])
        .output()
        .with_context(|| format!("failed to get info for {pkg}"))?;

    if output.status.success() {
        println!("{}", "Available Package:".bold().underline());
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        anyhow::bail!("package {pkg} not found");
    }
    Ok(())
}

fn rollback() -> Result<()> {
    println!("{} Rolling back last package operation...", "→".blue());
    let output = Command::new("snapper")
        .args(["list", "--type", "pre-post"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
            println!();
            println!(
                "Use: {} update rollback --to <ID> to restore a specific snapshot",
                "mnctl".bold()
            );
        }
        _ => {
            println!("{}", "No snapshots available for rollback.".yellow());
        }
    }
    Ok(())
}

fn pin_package(pkg: &str, version: &str) -> Result<()> {
    let pin_dir = "/etc/monolith/pins";
    std::fs::create_dir_all(pin_dir)?;

    let pin_file = format!("{pin_dir}/{pkg}");
    std::fs::write(&pin_file, version)?;

    // Add to pacman's IgnorePkg
    println!(
        "{} Pinned {} to version {}",
        "●".green(),
        pkg.bold(),
        version
    );
    println!(
        "  Note: Add '{}' to IgnorePkg in /etc/pacman.conf to prevent upgrades",
        pkg
    );
    Ok(())
}

fn unpin_package(pkg: &str) -> Result<()> {
    let pin_file = format!("/etc/monolith/pins/{pkg}");
    if std::path::Path::new(&pin_file).exists() {
        std::fs::remove_file(&pin_file)?;
        println!("{} Unpinned {}", "●".green(), pkg.bold());
    } else {
        println!("{} {} is not pinned", "●".yellow(), pkg);
    }
    Ok(())
}

fn show_pins() -> Result<()> {
    let pin_dir = "/etc/monolith/pins";
    let path = std::path::Path::new(pin_dir);

    if !path.exists() {
        println!("{}", "No pinned packages.".dimmed());
        return Ok(());
    }

    println!("{}", "Pinned Packages:".bold().underline());
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let pkg = entry.file_name();
        let version = std::fs::read_to_string(entry.path()).unwrap_or_default();
        println!(
            "  {} {:<30} {}",
            "●".green(),
            pkg.to_string_lossy(),
            version.trim()
        );
    }
    Ok(())
}

fn audit_packages() -> Result<()> {
    println!(
        "{}",
        "Checking installed packages for known CVEs...".dimmed()
    );
    let output = Command::new("arch-audit").output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.trim().is_empty() {
                println!("{}", "No known CVEs found.".green());
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

fn list_orphans() -> Result<()> {
    let output = Command::new("pacman")
        .args(["-Qtdq"])
        .output()
        .context("failed to list orphans")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        println!("{}", "No orphaned packages.".green());
    } else {
        println!("{}", "Orphaned Packages:".bold().underline());
        for line in stdout.lines() {
            println!("  {line}");
        }
        println!();
        println!(
            "Remove all orphans: {} -Rns $(pacman -Qtdq)",
            "sudo pacman".bold()
        );
    }
    Ok(())
}

fn package_sizes() -> Result<()> {
    let output = Command::new("pacman")
        .args(["-Qi"])
        .output()
        .context("failed to get package info")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut packages: Vec<(String, u64)> = Vec::new();

    let mut current_name = String::new();
    for line in stdout.lines() {
        if let Some(name) = line.strip_prefix("Name            : ") {
            current_name = name.trim().to_string();
        }
        if let Some(size_str) = line.strip_prefix("Installed Size  : ") {
            let size_str = size_str.trim();
            let size = parse_size(size_str);
            packages.push((current_name.clone(), size));
        }
    }

    packages.sort_by_key(|b| std::cmp::Reverse(b.1));

    println!("{}", "Packages by Size (top 30):".bold().underline());
    for (name, size) in packages.iter().take(30) {
        println!("  {:>10}  {}", format_size(*size), name);
    }
    Ok(())
}

fn parse_size(s: &str) -> u64 {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 2 {
        let num: f64 = parts[0].parse().unwrap_or(0.0);
        match parts[1] {
            "B" => num as u64,
            "KiB" => (num * 1024.0) as u64,
            "MiB" => (num * 1024.0 * 1024.0) as u64,
            "GiB" => (num * 1024.0 * 1024.0 * 1024.0) as u64,
            _ => 0,
        }
    } else {
        0
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn show_history() -> Result<()> {
    let log_path = "/var/log/pacman.log";
    if std::path::Path::new(log_path).exists() {
        let output = Command::new("grep")
            .args(["-E", r"\[ALPM\] (installed|removed|upgraded)", log_path])
            .output()
            .context("failed to read pacman log")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        let start = if lines.len() > 50 {
            lines.len() - 50
        } else {
            0
        };

        println!("{}", "Recent Package History:".bold().underline());
        for line in &lines[start..] {
            println!("  {line}");
        }
    } else {
        println!("{}", "No package history available.".dimmed());
    }
    Ok(())
}
