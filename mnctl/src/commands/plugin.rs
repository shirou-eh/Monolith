//! mnctl plugin system
//!
//! Plugins are external executables placed under one of:
//!   * `/usr/local/lib/monolith/plugins/`
//!   * `/usr/lib/monolith/plugins/`
//!   * `$XDG_CONFIG_HOME/monolith/plugins/` (defaults to `~/.config/monolith/plugins/`)
//!
//! A plugin must be an executable file named `mnctl-<name>`. When invoked via
//! `mnctl plugin run <name> [args...]` the file is exec'd directly and its
//! exit code is forwarded.
//!
//! Optional metadata can be supplied via `<name>.toml` next to the executable
//! or by emitting it on stdout when invoked with `--mnctl-metadata`. The
//! metadata is purely cosmetic for `mnctl plugin list`/`info`.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// List available plugins
    List,
    /// Show metadata for a specific plugin
    Info {
        /// Plugin name (without the mnctl- prefix)
        name: String,
    },
    /// Print plugin search paths
    Path,
    /// Install a plugin from a local file or URL
    Install {
        /// Plugin name to install (the file will be saved as mnctl-<name>)
        name: String,
        /// Local path or http(s):// URL of the plugin executable
        source: String,
        /// Install system-wide (requires root). Otherwise installs into the user dir.
        #[arg(long)]
        system: bool,
    },
    /// Remove an installed plugin
    Remove {
        /// Plugin name (without mnctl- prefix)
        name: String,
    },
    /// Run a plugin (any extra args are passed through)
    Run {
        /// Plugin name (without mnctl- prefix)
        name: String,
        /// Arguments forwarded to the plugin
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PluginMetadata {
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    homepage: String,
}

#[derive(Debug, Clone)]
struct DiscoveredPlugin {
    name: String,
    path: PathBuf,
    metadata: PluginMetadata,
}

const SYSTEM_PLUGIN_DIRS: &[&str] = &[
    "/usr/local/lib/monolith/plugins",
    "/usr/lib/monolith/plugins",
];

impl PluginArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            PluginCommand::List => list_plugins(),
            PluginCommand::Info { name } => plugin_info(&name),
            PluginCommand::Path => path_info(),
            PluginCommand::Install {
                name,
                source,
                system,
            } => install_plugin(&name, &source, system),
            PluginCommand::Remove { name } => remove_plugin(&name),
            PluginCommand::Run { name, args } => run_plugin(&name, &args),
        }
    }
}

fn user_plugin_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("monolith/plugins");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config/monolith/plugins");
    }
    PathBuf::from(".config/monolith/plugins")
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = SYSTEM_PLUGIN_DIRS.iter().map(PathBuf::from).collect();
    dirs.push(user_plugin_dir());
    dirs
}

fn discover() -> Vec<DiscoveredPlugin> {
    let mut plugins = Vec::new();
    for dir in search_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(name) = file_name.strip_prefix("mnctl-") else {
                continue;
            };
            if !is_executable(&path) {
                continue;
            }
            let metadata = load_metadata(&dir, name);
            plugins.push(DiscoveredPlugin {
                name: name.to_string(),
                path,
                metadata,
            });
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins.dedup_by(|a, b| a.name == b.name);
    plugins
}

fn load_metadata(dir: &Path, name: &str) -> PluginMetadata {
    let path = dir.join(format!("{name}.toml"));
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(meta) = toml::from_str::<PluginMetadata>(&content) {
            return meta;
        }
    }
    PluginMetadata::default()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_file()).unwrap_or(false)
}

fn list_plugins() -> Result<()> {
    let plugins = discover();
    if plugins.is_empty() {
        println!("{}", "No plugins installed.".yellow());
        println!();
        println!("Search paths:");
        for d in search_dirs() {
            println!("  {}", d.display());
        }
        println!();
        println!(
            "Install with: {} plugin install <name> <path|url>",
            "mnctl".bold()
        );
        return Ok(());
    }
    println!("{}", "Installed Plugins:".bold().underline());
    println!();
    for p in plugins {
        let desc = if p.metadata.description.is_empty() {
            "(no description)".dimmed().to_string()
        } else {
            p.metadata.description.dimmed().to_string()
        };
        let version = if p.metadata.version.is_empty() {
            String::new()
        } else {
            format!(" v{}", p.metadata.version)
        };
        println!("  {} {}{}  {}", "●".green(), p.name.bold(), version, desc);
        println!("    {}", p.path.display().to_string().dimmed());
    }
    Ok(())
}

fn plugin_info(name: &str) -> Result<()> {
    let plugins = discover();
    let Some(plugin) = plugins.into_iter().find(|p| p.name == name) else {
        anyhow::bail!("plugin '{name}' not found");
    };
    println!("{}", plugin.name.bold().underline());
    println!("  Path:        {}", plugin.path.display());
    if !plugin.metadata.description.is_empty() {
        println!("  Description: {}", plugin.metadata.description);
    }
    if !plugin.metadata.version.is_empty() {
        println!("  Version:     {}", plugin.metadata.version);
    }
    if !plugin.metadata.author.is_empty() {
        println!("  Author:      {}", plugin.metadata.author);
    }
    if !plugin.metadata.homepage.is_empty() {
        println!("  Homepage:    {}", plugin.metadata.homepage);
    }
    Ok(())
}

fn path_info() -> Result<()> {
    println!("{}", "Plugin Search Paths:".bold().underline());
    for dir in search_dirs() {
        let exists = dir.exists();
        let marker = if exists {
            "●".green()
        } else {
            "○".dimmed()
        };
        println!("  {} {}", marker, dir.display());
    }
    Ok(())
}

fn install_plugin(name: &str, source: &str, system: bool) -> Result<()> {
    let target_dir = if system {
        PathBuf::from(SYSTEM_PLUGIN_DIRS[0])
    } else {
        user_plugin_dir()
    };
    std::fs::create_dir_all(&target_dir)
        .with_context(|| format!("failed to create plugin dir: {}", target_dir.display()))?;
    let target = target_dir.join(format!("mnctl-{name}"));

    if source.starts_with("http://") || source.starts_with("https://") {
        download_to(source, &target)?;
    } else {
        std::fs::copy(source, &target)
            .with_context(|| format!("failed to copy {source} -> {}", target.display()))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&target)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&target, perms)?;
    }

    println!(
        "{} Installed plugin {} to {}",
        "●".green(),
        name.bold(),
        target.display()
    );
    Ok(())
}

fn download_to(url: &str, target: &Path) -> Result<()> {
    // Try curl, fall back to wget. We avoid pulling additional Rust deps to
    // keep the binary lean — these tools ship with Monolith OS's base image.
    let curl = which::which("curl");
    let wget = which::which("wget");

    if curl.is_ok() {
        let status = Command::new("curl")
            .args(["-fsSL", "-o", target.to_string_lossy().as_ref(), url])
            .status()
            .context("failed to run curl")?;
        if !status.success() {
            anyhow::bail!("curl failed to download {url}");
        }
        return Ok(());
    }

    if wget.is_ok() {
        let status = Command::new("wget")
            .args(["-qO", target.to_string_lossy().as_ref(), url])
            .status()
            .context("failed to run wget")?;
        if !status.success() {
            anyhow::bail!("wget failed to download {url}");
        }
        return Ok(());
    }

    anyhow::bail!("neither curl nor wget available for downloading plugins");
}

fn remove_plugin(name: &str) -> Result<()> {
    let mut removed_any = false;
    for dir in search_dirs() {
        let path = dir.join(format!("mnctl-{name}"));
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            println!("{} Removed {}", "●".green(), path.display());
            removed_any = true;
        }
        let meta = dir.join(format!("{name}.toml"));
        if meta.exists() {
            let _ = std::fs::remove_file(&meta);
        }
    }
    if !removed_any {
        anyhow::bail!("plugin '{name}' not installed");
    }
    Ok(())
}

fn run_plugin(name: &str, args: &[String]) -> Result<()> {
    let plugins = discover();
    let Some(plugin) = plugins.into_iter().find(|p| p.name == name) else {
        anyhow::bail!(
            "plugin '{name}' not found. Run `mnctl plugin list` to see available plugins."
        );
    };
    let status = Command::new(&plugin.path)
        .args(args)
        .status()
        .with_context(|| format!("failed to exec plugin {}", plugin.path.display()))?;
    if !status.success() {
        anyhow::bail!(
            "plugin {} exited with status {}",
            name,
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}
