//! Declarative system configuration (NixOS-inspired, deliberately much
//! smaller in scope): describe the profile, hardening level, service set,
//! and firewall rules a host *should* have in one TOML file, then
//! reconcile the running system to match with one command.
//!
//! This is not a from-scratch config manager — it's a thin orchestration
//! layer over commands mnctl already has (`profile set`, `security
//! harden`, `security firewall allow/deny`, `systemctl enable/disable`),
//! applied in a fixed order and reported per-step instead of atomically.
//! A step failing doesn't roll back earlier steps; it's reported and the
//! rest of the spec still gets a chance to apply.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use std::process::Command;

#[derive(Args)]
pub struct DeclareArgs {
    #[command(subcommand)]
    command: DeclareCommand,
}

#[derive(Subcommand)]
enum DeclareCommand {
    /// Reconcile this host to match a spec file
    Apply {
        /// Path to the spec TOML
        #[arg(short, long, default_value = "/etc/monolith/declare.toml")]
        file: String,
        /// Print the steps that would run without applying them
        #[arg(long)]
        dry_run: bool,
    },
    /// Write a commented example spec to start from
    Init {
        #[arg(short, long, default_value = "/etc/monolith/declare.toml")]
        out: String,
    },
}

impl DeclareArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            DeclareCommand::Apply { file, dry_run } => declare_apply(&file, dry_run),
            DeclareCommand::Init { out } => declare_init(&out),
        }
    }
}

#[derive(Deserialize, Default)]
struct Spec {
    system: Option<SystemSpec>,
    services: Option<ServicesSpec>,
    firewall: Option<FirewallSpec>,
}

#[derive(Deserialize, Default)]
struct SystemSpec {
    /// One of: lite, full, pro, desktop — see `mnctl profile list`
    profile: Option<String>,
    /// One of: server, desktop, paranoid — see `mnctl security harden --help`
    hardening: Option<String>,
}

#[derive(Deserialize, Default)]
struct ServicesSpec {
    #[serde(default)]
    enabled: Vec<String>,
    #[serde(default)]
    disabled: Vec<String>,
}

#[derive(Deserialize, Default)]
struct FirewallSpec {
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
}

/// Run `mnctl <args...>` (or, for firewall/profile/harden, the specific
/// subcommand it needs), reporting the outcome instead of assuming
/// success. Steps talk to the already-installed `mnctl`/`systemctl`
/// binaries rather than calling internal functions directly, matching
/// how `cluster_rolling_update` reaches other subsystems — one seam,
/// not two divergent code paths for "call it locally" vs "call it
/// as a subcommand".
fn run_step(label: &str, dry_run: bool, cmd: &str, args: &[&str]) -> bool {
    if dry_run {
        println!("  {} {cmd} {}", "would run:".dimmed(), args.join(" "));
        return true;
    }
    let ok = Command::new(cmd).args(args).status().map(|s| s.success()).unwrap_or(false);
    println!("  {} {label}", if ok { "●".green() } else { "●".red() });
    ok
}

fn declare_apply(file: &str, dry_run: bool) -> Result<()> {
    let content = std::fs::read_to_string(file).with_context(|| format!("failed to read spec {file}"))?;
    let spec: Spec = toml::from_str(&content).with_context(|| format!("failed to parse spec {file}"))?;

    println!("{} Reconciling against {file}{}", "→".blue().bold(), if dry_run { " (dry run)".dimmed().to_string() } else { String::new() });

    let mut failures = 0u32;

    if let Some(system) = &spec.system {
        if let Some(profile) = &system.profile {
            println!("{}", "Profile:".bold());
            if !run_step(&format!("profile set {profile}"), dry_run, "mnctl", &["profile", "set", profile]) {
                failures += 1;
            }
        }
        if let Some(level) = &system.hardening {
            println!("{}", "Hardening:".bold());
            if !run_step(&format!("harden --level {level}"), dry_run, "mnctl", &["security", "harden", "--level", level]) {
                failures += 1;
            }
        }
    }

    if let Some(services) = &spec.services {
        if !services.enabled.is_empty() {
            println!("{}", "Services (enabled):".bold());
            for svc in &services.enabled {
                if !run_step(svc, dry_run, "systemctl", &["enable", "--now", svc]) {
                    failures += 1;
                }
            }
        }
        if !services.disabled.is_empty() {
            println!("{}", "Services (disabled):".bold());
            for svc in &services.disabled {
                if !run_step(svc, dry_run, "systemctl", &["disable", "--now", svc]) {
                    failures += 1;
                }
            }
        }
    }

    if let Some(firewall) = &spec.firewall {
        if !firewall.allow.is_empty() {
            println!("{}", "Firewall (allow):".bold());
            for port in &firewall.allow {
                let (p, udp) = split_port_proto(port);
                let mut args = vec!["security", "firewall", "allow", p];
                if udp {
                    args.push("--udp");
                }
                if !run_step(port, dry_run, "mnctl", &args) {
                    failures += 1;
                }
            }
        }
        if !firewall.deny.is_empty() {
            println!("{}", "Firewall (deny):".bold());
            for port in &firewall.deny {
                let (p, udp) = split_port_proto(port);
                let mut args = vec!["security", "firewall", "deny", p];
                if udp {
                    args.push("--udp");
                }
                if !run_step(port, dry_run, "mnctl", &args) {
                    failures += 1;
                }
            }
        }
    }

    println!();
    if failures == 0 {
        println!("{} Host matches {file}", "●".green().bold());
        Ok(())
    } else {
        anyhow::bail!("{failures} step(s) failed — host does not fully match {file}");
    }
}

/// "80/udp" -> ("80", true); "80" -> ("80", false)
fn split_port_proto(spec: &str) -> (&str, bool) {
    match spec.split_once('/') {
        Some((port, proto)) if proto.eq_ignore_ascii_case("udp") => (port, true),
        _ => (spec, false),
    }
}

fn declare_init(out: &str) -> Result<()> {
    if let Some(parent) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("failed to create {parent:?}"))?;
    }
    if std::path::Path::new(out).exists() {
        anyhow::bail!("{out} already exists — remove it first or pass a different --out");
    }

    let example = r#"# Monolith declarative spec — reconcile with: mnctl declare apply -f <this file>
# Every section is optional; omit what you don't want mnctl to touch.

[system]
profile = "full"       # lite | full | pro | desktop — see `mnctl profile list`
hardening = "server"   # server | desktop | paranoid — see `mnctl security harden --help`

[services]
enabled = ["monolith-cluster"]
disabled = []

[firewall]
allow = ["ssh", "https"]
deny = []
"#;

    std::fs::write(out, example).with_context(|| format!("failed to write {out}"))?;
    println!("{} Example spec written to {out}", "●".green());
    println!("  {} Edit it, then: mnctl declare apply -f {out}", "→".blue());
    Ok(())
}
