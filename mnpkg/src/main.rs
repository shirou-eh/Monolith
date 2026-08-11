use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use serde::Deserialize;
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
    /// Update all packages with snapshot safety, or Monolith itself with --self
    Update {
        /// Fetch the latest Monolith release from GitHub and install it,
        /// instead of updating system packages
        #[arg(long = "self")]
        self_update: bool,
        /// Specific release tag to install (implies --self), e.g. "v1.2.0"
        #[arg(long)]
        version: Option<String>,
        /// Reinstall even if already on the target version (implies --self)
        #[arg(long)]
        force: bool,
    },
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
    /// Manage a custom pacman package repository
    Repo(RepoArgs),
}

#[derive(clap::Args)]
struct RepoArgs {
    #[command(subcommand)]
    command: RepoCommand,
}

#[derive(Subcommand)]
enum RepoCommand {
    /// Create a new local repo directory (database is built on first `add`/`build`)
    Init {
        /// Directory to hold the repo
        path: String,
        /// Repo name — becomes <name>.db.tar.gz, and the [name] in pacman.conf
        #[arg(long, default_value = "monolith")]
        name: String,
    },
    /// Copy package file(s) into the repo and rebuild its database
    Add {
        /// Repo directory (from `repo init`)
        path: String,
        /// One or more built .pkg.tar.zst / .pkg.tar.xz files
        #[arg(required = true)]
        packages: Vec<String>,
        #[arg(long, default_value = "monolith")]
        name: String,
    },
    /// Rebuild the repo database from whatever package files are already in the directory
    Build {
        path: String,
        #[arg(long, default_value = "monolith")]
        name: String,
    },
    /// Serve the repo directory over plain HTTP
    Serve {
        path: String,
        #[arg(long, default_value_t = 8899)]
        port: u16,
    },
    /// Print the pacman.conf block to add this repo on another machine
    Snippet {
        /// Base URL where `repo serve` (or any web server) exposes the directory
        url: String,
        #[arg(long, default_value = "monolith")]
        name: String,
    },
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
        Commands::Update { self_update, version, force } => {
            if self_update || version.is_some() || force {
                self_update_monolith(force, version.as_deref()).await
            } else {
                update_packages()
            }
        }
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
        Commands::Repo(args) => match args.command {
            RepoCommand::Init { path, name } => repo_init(&path, &name),
            RepoCommand::Add { path, packages, name } => repo_add(&path, &packages, &name),
            RepoCommand::Build { path, name } => repo_build(&path, &name),
            RepoCommand::Serve { path, port } => repo_serve(&path, port),
            RepoCommand::Snippet { url, name } => {
                repo_snippet(&url, &name);
                Ok(())
            }
        },
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
    let mut child = Command::new("pacman")
        .args(["-Syu", "--noconfirm"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to start pacman")?;

    use std::io::{BufRead, BufReader};

    // Read stderr (pacman sends progress to stderr)
    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(l) = line {
                // Show ALPM transaction lines, download progress, warnings
                if l.contains("[ALPM]")
                    || l.contains("warning:")
                    || l.contains("error:")
                    || l.contains("Packages (")
                    || l.contains("Total Download Size")
                    || l.contains("Total Installed Size")
                {
                    // Color-code ALPM actions
                    if l.contains("] installed") {
                        println!("  {}", l.green());
                    } else if l.contains("] removed") {
                        println!("  {}", l.red());
                    } else if l.contains("] upgraded") || l.contains("] downgraded") {
                        println!("  {}", l.yellow());
                    } else {
                        println!("  {l}");
                    }
                }
            }
        }
    }

    let status = child.wait().context("failed to wait for pacman")?;

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
            // Clean up pacman lock file after rollback so subsequent
            // install/upgrade commands don't refuse to run.
            let lockfile = "/var/lib/pacman/db.lck";
            if std::path::Path::new(lockfile).exists() {
                let _ = std::fs::remove_file(lockfile);
                println!(
                    "  {} Removed stale pacman lock file",
                    "●".yellow()
                );
            }
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
                // Render CVE IDs as OSC 8 hyperlinks for modern terminals
                for line in stdout.lines() {
                    let mut line = line.to_string();
                    // Find CVE-XXXX-XXXX patterns and wrap with hyperlink
                    let mut replaced = String::new();
                    let mut rest = line.as_str();
                    while let Some(start) = rest.find("CVE-") {
                        replaced.push_str(&rest[..start]);
                        let cve_end = rest[start..]
                            .find(|c: char| !c.is_alphanumeric() && c != '-')
                            .map(|i| start + i)
                            .unwrap_or(rest.len());
                        let cve_id = &rest[start..cve_end];
                        let url = format!("https://security.archlinux.org/{cve_id}");
                        replaced.push_str(&format!(
                            "\x1b]8;;{url}\x1b\\{cve_id}\x1b]8;;\x1b\\"
                        ));
                        rest = &rest[cve_end..];
                    }
                    replaced.push_str(rest);
                    println!("  {replaced}");
                }
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

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// `mnpkg update --self` — check the upstream `shirou-eh/Monolith`
/// GitHub repo for a newer release, download the matching release
/// tarball, snapshot first (same snapshot-before-upgrade rule the
/// regular `mnpkg update` follows), and unpack it into place.
async fn self_update_monolith(force: bool, version: Option<&str>) -> Result<()> {
    let repo = "shirou-eh/Monolith";
    let client = reqwest::Client::builder()
        .user_agent("mnpkg")
        .build()
        .context("failed to build HTTP client")?;

    let tag = match version {
        Some(v) => v.trim_start_matches('v').to_string(),
        None => {
            println!("{} Checking {repo} for the latest release...", "→".blue());
            let url = format!("https://api.github.com/repos/{repo}/releases/latest");
            let resp = client
                .get(&url)
                .send()
                .await
                .context("failed to reach GitHub")?;
            if !resp.status().is_success() {
                anyhow::bail!("GitHub API returned {} — check network / rate limit", resp.status());
            }
            let release: GithubRelease = resp
                .json()
                .await
                .context("failed to parse GitHub release JSON")?;
            release.tag_name.trim_start_matches('v').to_string()
        }
    };

    let current = env!("CARGO_PKG_VERSION");
    if tag == current && !force {
        println!("{} mnpkg is already on v{}", "●".green(), current.bold());
        return Ok(());
    }

    println!(
        "{} Updating Monolith v{} → v{}",
        "→".blue(),
        current,
        tag.bold()
    );

    // Same rule as a normal `mnpkg update`: snapshot before touching anything.
    println!("{} Creating pre-update snapshot...", "→".blue());
    let _ = Command::new("snapper")
        .args(["create", "--description", &format!("pre-mnpkg-self-update-{tag}"), "--type", "pre"])
        .output();

    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let tarball_name = format!("monolith-{arch}-unknown-{os}-gnu.tar.gz");
    let url = format!("https://github.com/{repo}/releases/download/v{tag}/{tarball_name}");

    println!("  {} Downloading {tarball_name}...", "↓".cyan());
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to download {tarball_name}"))?;

    if !resp.status().is_success() {
        anyhow::bail!("download failed with HTTP {} — is v{tag} released?", resp.status());
    }

    let bytes = resp.bytes().await.context("failed to read response body")?;
    let tmp = "/tmp/mnpkg-self-update.tar.gz";
    std::fs::write(tmp, &bytes).context("failed to write tarball to /tmp")?;

    let install_dir = "/usr/local/bin";
    println!("  {} Extracting to {install_dir}...", "→".blue());
    let output = Command::new("sh")
        .args(["-c", &format!("tar -xzf {tmp} -C {install_dir}")])
        .output()
        .context("failed to extract tarball")?;

    let _ = std::fs::remove_file(tmp);

    if !output.status.success() {
        anyhow::bail!("extraction failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    println!();
    println!("{} Monolith v{} installed to {install_dir}", "✓".green(), tag.bold());
    println!("{} Post-update snapshot...", "→".blue());
    let _ = Command::new("snapper")
        .args(["create", "--description", &format!("post-mnpkg-self-update-{tag}"), "--type", "post"])
        .output();

    Ok(())
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

fn repo_init(path: &str, name: &str) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("failed to create {path}"))?;
    println!("{} Repo directory ready at {path}", "●".green());
    println!("  {} Add packages: mnpkg repo add {path} <file.pkg.tar.zst>", "→".blue());
    println!("  {} Then, on other machines, add to /etc/pacman.conf:", "→".blue());
    println!();
    repo_snippet(&format!("file://{path}"), name);
    Ok(())
}

/// Every *.pkg.tar.zst / *.pkg.tar.xz / *.pkg.tar.gz in `path`, sorted so
/// `repo-add`'s output order is stable across runs.
fn repo_package_files(path: &str) -> Result<Vec<String>> {
    let mut files: Vec<String> = std::fs::read_dir(path)
        .with_context(|| format!("failed to read repo directory {path}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".pkg.tar.zst") || name.ends_with(".pkg.tar.xz") || name.ends_with(".pkg.tar.gz")
        })
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    files.sort();
    Ok(files)
}

fn run_repo_add(path: &str, name: &str) -> Result<()> {
    if which::which("repo-add").is_err() {
        anyhow::bail!("repo-add not found — install pacman-contrib");
    }

    let files = repo_package_files(path)?;
    if files.is_empty() {
        anyhow::bail!("no .pkg.tar.* files in {path} yet");
    }

    let db_path = format!("{path}/{name}.db.tar.gz");
    let mut cmd = Command::new("repo-add");
    cmd.arg(&db_path);
    cmd.args(&files);
    let status = cmd.status().context("failed to run repo-add")?;
    if !status.success() {
        anyhow::bail!("repo-add failed");
    }

    println!("{} {name}.db.tar.gz rebuilt ({} package(s))", "●".green(), files.len());
    Ok(())
}

fn repo_add(path: &str, packages: &[String], name: &str) -> Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("failed to create {path}"))?;

    for pkg in packages {
        let src = std::path::Path::new(pkg);
        if !src.exists() {
            anyhow::bail!("package file not found: {pkg}");
        }
        let filename = src.file_name().ok_or_else(|| anyhow::anyhow!("bad package path: {pkg}"))?;
        let dest = std::path::Path::new(path).join(filename);
        if src.canonicalize().ok() != dest.canonicalize().ok() {
            std::fs::copy(src, &dest).with_context(|| format!("failed to copy {pkg} into {path}"))?;
        }
        println!("  {} {}", "●".green(), filename.to_string_lossy());
    }

    run_repo_add(path, name)
}

fn repo_build(path: &str, name: &str) -> Result<()> {
    run_repo_add(path, name)
}

/// Minimal read-only static file server — enough for `pacman -Sy` to pull
/// the db file and package archives over HTTP from another Monolith node
/// on the LAN. No directory listing beyond what's needed to fetch a
/// named file; paths are resolved and checked to stay inside `path` so a
/// request can't walk out of the repo directory with `../`.
fn repo_serve(path: &str, port: u16) -> Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let root = std::fs::canonicalize(path).with_context(|| format!("repo directory not found: {path}"))?;
    let listener = TcpListener::bind(("0.0.0.0", port)).with_context(|| format!("failed to bind port {port}"))?;
    println!("{} Serving {} on http://0.0.0.0:{port}/", "●".green(), path);
    println!("  {} Ctrl+C to stop", "→".blue());

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mut reader = BufReader::new(stream.try_clone().expect("clone tcp stream"));
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            continue;
        }

        // "GET /path HTTP/1.1"
        let requested = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .trim_start_matches('/');
        let requested = urlencoding_decode(requested);

        let candidate = root.join(&requested);
        let resolved = std::fs::canonicalize(&candidate).ok().filter(|p| p.starts_with(&root) && p.is_file());

        match resolved {
            Some(file_path) => {
                if let Ok(mut file) = std::fs::File::open(&file_path) {
                    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = std::io::copy(&mut file, &mut stream);
                } else {
                    let _ = stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nConnection: close\r\n\r\n");
                }
            }
            None => {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\nnot found");
            }
        }
    }
    Ok(())
}

/// `%XX`-decode a request path. Just enough for filenames pacman itself
/// generates (letters, digits, `-`, `_`, `.`, `+`, `~`) — not a full URL
/// decoder, but this only ever needs to round-trip what `repo-add`
/// produced in the same directory.
fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                out.push(byte as char);
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn repo_snippet(url: &str, name: &str) {
    println!("[{name}]");
    println!("SigLevel = Optional TrustAll");
    println!("Server = {url}");
}
