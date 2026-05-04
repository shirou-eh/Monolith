//! Resource profile management.
//!
//! Monolith ships three opinionated profiles:
//!
//! * `lite` — designed for ≤512 MB RAM, single-app hosts (Discord bots,
//!   small Telegram bots, light web services). Disables the
//!   Prometheus/Grafana/Loki stack, the `mnweb` service, and turns off
//!   automatic SMART self-tests so the box stays idle.
//! * `full` — the default for home servers and side projects. Enables the
//!   monitoring stack and SMART checks but keeps `mnweb` opt-in.
//! * `pro` — production / multi-node. Enables everything (`mnweb`,
//!   monitoring, SMART, k3s placeholders).
//!
//! The profile is persisted in `/etc/monolith/monolith.toml` under
//! `[system].profile`. Switching profile rewrites the relevant config
//! sections and tells the user which services to (re)start.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

const CONFIG_PATH: &str = "/etc/monolith/monolith.toml";

#[derive(Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    command: ProfileCommand,
}

#[derive(Subcommand)]
enum ProfileCommand {
    /// Show the current resource profile and what each profile enables
    Show,
    /// List the available profiles with their resource targets
    List,
    /// Switch to a different resource profile
    Set {
        /// Profile name: lite, full, or pro
        name: String,
        /// Skip writing config to disk; just print what would change
        #[arg(long)]
        dry_run: bool,
        /// Override the path to monolith.toml (defaults to /etc/monolith/...)
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

impl ProfileArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ProfileCommand::Show => show(),
            ProfileCommand::List => list(),
            ProfileCommand::Set {
                name,
                dry_run,
                config,
            } => set(&name, dry_run, config),
        }
    }
}

fn list() -> Result<()> {
    println!("{}", "Available profiles".bold().underline());
    println!();
    for profile in PROFILES {
        println!(
            "  {}{}",
            profile.name.bold(),
            if profile.name == "full" {
                "  (default)".dimmed().to_string()
            } else {
                String::new()
            }
        );
        println!("    target:     {}", profile.target);
        println!("    monitoring: {}", on_off(profile.monitoring));
        println!("    mnweb:      {}", on_off(profile.mnweb));
        println!("    smart:      {}", on_off(profile.smart));
        println!("    notify:     {}", on_off(profile.notifications));
        println!();
    }
    Ok(())
}

fn show() -> Result<()> {
    let current = read_current_profile().unwrap_or_else(|_| "full".to_string());
    let profile = PROFILES
        .iter()
        .find(|p| p.name == current)
        .unwrap_or(&PROFILES[1]);
    println!(
        "{} {}",
        "Current profile:".bold(),
        profile.name.green().bold()
    );
    println!("  target:     {}", profile.target);
    println!("  monitoring: {}", on_off(profile.monitoring));
    println!("  mnweb:      {}", on_off(profile.mnweb));
    println!("  smart:      {}", on_off(profile.smart));
    println!("  notify:     {}", on_off(profile.notifications));
    Ok(())
}

fn set(name: &str, dry_run: bool, config_override: Option<PathBuf>) -> Result<()> {
    let profile = PROFILES
        .iter()
        .find(|p| p.name == name)
        .with_context(|| format!("unknown profile '{name}'. Try: lite, full, pro"))?;

    let path = config_override.unwrap_or_else(|| PathBuf::from(CONFIG_PATH));
    let original = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let updated = apply_profile(&original, profile);

    if dry_run {
        println!(
            "{}",
            format!("--- {} (dry-run, profile={}) ---", path.display(), name).bold()
        );
        println!("{updated}");
        return Ok(());
    }

    std::fs::write(&path, &updated)
        .with_context(|| format!("failed to write {}", path.display()))?;

    println!(
        "{} switched to profile {}",
        "●".green(),
        profile.name.bold()
    );
    println!();
    println!("{}", "Next steps:".bold());
    if profile.monitoring {
        println!("  • mnctl monitor enable    # Prometheus/Grafana/Loki on");
    } else {
        println!("  • mnctl monitor disable   # turn the stack off to save RAM");
    }
    if profile.mnweb {
        println!("  • mnctl web enable        # bring the web UI up");
    } else {
        println!("  • mnctl web disable       # stop the web UI if running");
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    target: &'static str,
    monitoring: bool,
    mnweb: bool,
    smart: bool,
    notifications: bool,
}

const PROFILES: &[Profile] = &[
    Profile {
        name: "lite",
        target: "≥1 vCPU / ≥512 MB RAM (Discord bots, tiny VPSes)",
        monitoring: false,
        mnweb: false,
        smart: false,
        notifications: true,
    },
    Profile {
        name: "full",
        target: "≥2 cores / ≥2 GB RAM (home servers, side projects)",
        monitoring: true,
        mnweb: false,
        smart: true,
        notifications: true,
    },
    Profile {
        name: "pro",
        target: "≥4 cores / ≥8 GB RAM (production, k3s clusters)",
        monitoring: true,
        mnweb: true,
        smart: true,
        notifications: true,
    },
];

fn on_off(b: bool) -> colored::ColoredString {
    if b {
        "on".green()
    } else {
        "off".dimmed()
    }
}

fn read_current_profile() -> Result<String> {
    let content = std::fs::read_to_string(CONFIG_PATH)
        .with_context(|| format!("failed to read {CONFIG_PATH}"))?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("profile") {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix('=') {
                return Ok(value.trim().trim_matches('"').to_string());
            }
        }
    }
    Ok("full".to_string())
}

/// Rewrite the config text so the relevant sections match the requested
/// profile. We do a textual line-based pass rather than a full TOML
/// round-trip so we preserve user comments and formatting.
fn apply_profile(original: &str, profile: &Profile) -> String {
    let mut out = String::with_capacity(original.len());
    let mut current_section: Option<String> = None;
    for line in original.lines() {
        let trimmed = line.trim();
        if let Some(section) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current_section = Some(section.to_string());
        }

        let rewritten = if let Some(section) = current_section.as_deref() {
            rewrite_line(section, line, trimmed, profile)
        } else {
            line.to_string()
        };

        out.push_str(&rewritten);
        out.push('\n');
    }
    out
}

fn rewrite_line(section: &str, line: &str, trimmed: &str, profile: &Profile) -> String {
    // [system].profile is the source of truth; always update it.
    if section == "system" && trimmed.starts_with("profile") {
        return format!("profile = \"{}\"", profile.name);
    }
    let (key, value) = match section {
        "monitoring" if trimmed.starts_with("enabled") => ("enabled", profile.monitoring),
        "webui" if trimmed.starts_with("enabled") => ("enabled", profile.mnweb),
        "disks" if trimmed.starts_with("smart_check_enabled") => {
            ("smart_check_enabled", profile.smart)
        }
        "notifications" if trimmed.starts_with("enabled") && !trimmed.contains("smtp") => {
            ("enabled", profile.notifications)
        }
        _ => return line.to_string(),
    };
    format!("{key} = {value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[system]
profile = "full"

[monitoring]
enabled = true
prometheus_port = 9090

[webui]
enabled = false

[disks]
smart_check_enabled = true

[notifications]
enabled = false

[notifications.smtp]
enabled = false
"#;

    #[test]
    fn lite_disables_heavy_stack() {
        let lite = &PROFILES[0];
        assert_eq!(lite.name, "lite");
        let updated = apply_profile(SAMPLE, lite);
        assert!(updated.contains("profile = \"lite\""));
        assert!(updated.contains("[monitoring]\nenabled = false"));
        assert!(updated.contains("[webui]\nenabled = false"));
        assert!(updated.contains("[disks]\nsmart_check_enabled = false"));
        // notifications stays enabled in lite — the bot likely wants to
        // ping the operator if it crashes.
        assert!(updated.contains("[notifications]\nenabled = true"));
        // The SMTP sub-section's `enabled` must NOT be touched.
        assert!(updated.contains("[notifications.smtp]\nenabled = false"));
    }

    #[test]
    fn pro_turns_everything_on() {
        let pro = &PROFILES[2];
        let updated = apply_profile(SAMPLE, pro);
        assert!(updated.contains("profile = \"pro\""));
        assert!(updated.contains("[monitoring]\nenabled = true"));
        assert!(updated.contains("[webui]\nenabled = true"));
        assert!(updated.contains("[disks]\nsmart_check_enabled = true"));
    }

    #[test]
    fn unknown_sections_are_left_alone() {
        let lite = &PROFILES[0];
        let custom = "[custom]\nenabled = true\n";
        let updated = apply_profile(custom, lite);
        assert!(updated.contains("[custom]\nenabled = true"));
    }
}
