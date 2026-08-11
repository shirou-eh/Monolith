//! mnctl-ups — UPS monitoring plugin for Monolith OS.
//!
//! Drop the built binary at `~/.local/share/mnctl/plugins/mnctl-ups` (or
//! `/usr/local/lib/mnctl/plugins/` system-wide) and it shows up as
//! `mnctl ups`, per the plugin system documented in the main README.
//!
//! Talks to a UPS through NUT (`upsc`/`upscmd`, package `nut`) — the
//! widest-supported option, works with basically every UPS vendor, not
//! just one brand. Posts rich, colour-coded embeds to a Discord webhook
//! on every state change (on battery / back online / low battery), and
//! on a genuinely critical battery triggers a safe, ordered shutdown
//! instead of just watching the box die mid-write.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "mnctl-ups", about = "UPS monitoring with Discord alerts and graceful shutdown")]
struct Cli {
    /// NUT UPS name (as `upsc -l` lists it). Auto-detects the first one
    /// found if omitted — fine for the common case of exactly one UPS.
    #[arg(long, global = true)]
    ups: Option<String>,
    /// Discord webhook URL. Falls back to $MNCTL_UPS_DISCORD_WEBHOOK,
    /// then /etc/monolith/plugins/ups.toml's `discord_webhook` key.
    #[arg(long, global = true)]
    webhook: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print current UPS status once and exit
    Status,
    /// Watch continuously; alert on state changes, shut down safely on
    /// critical battery
    Watch {
        /// Seconds between polls
        #[arg(long, default_value_t = 15)]
        interval: u64,
        /// Battery charge percent at/below which a shutdown starts
        #[arg(long, default_value_t = 20)]
        shutdown_at: u32,
        /// Report status to Discord even when nothing changed, every
        /// this many polls (0 = only on state transitions)
        #[arg(long, default_value_t = 0)]
        heartbeat_every: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum UpsState {
    OnLine,
    OnBattery,
    LowBattery,
    Unknown(String),
}

impl UpsState {
    fn from_status_field(s: &str) -> Self {
        // ups.status can list multiple space-separated flags, e.g. "OB LB".
        // LB (low battery) takes priority over OB (on battery) which takes
        // priority over OL (on line/mains).
        if s.split_whitespace().any(|f| f == "LB") {
            UpsState::LowBattery
        } else if s.split_whitespace().any(|f| f == "OB") {
            UpsState::OnBattery
        } else if s.split_whitespace().any(|f| f == "OL") {
            UpsState::OnLine
        } else {
            UpsState::Unknown(s.to_string())
        }
    }

    fn label(&self) -> &'static str {
        match self {
            UpsState::OnLine => "On mains power",
            UpsState::OnBattery => "Running on battery",
            UpsState::LowBattery => "Battery critically low",
            UpsState::Unknown(_) => "Unknown state",
        }
    }

    fn discord_color(&self) -> u32 {
        match self {
            UpsState::OnLine => 0x2ecc71,     // green
            UpsState::OnBattery => 0xf1c40f,  // yellow
            UpsState::LowBattery => 0xe74c3c, // red
            UpsState::Unknown(_) => 0x95a5a6, // grey
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let webhook = resolve_webhook(cli.webhook.clone());

    match cli.command {
        Commands::Status => cmd_status(cli.ups.as_deref()).await,
        Commands::Watch { interval, shutdown_at, heartbeat_every } => {
            cmd_watch(cli.ups.as_deref(), webhook.as_deref(), interval, shutdown_at, heartbeat_every).await
        }
    }
}

fn resolve_webhook(flag: Option<String>) -> Option<String> {
    flag.or_else(|| std::env::var("MNCTL_UPS_DISCORD_WEBHOOK").ok())
        .or_else(|| {
            let content = std::fs::read_to_string("/etc/monolith/plugins/ups.toml").ok()?;
            content
                .lines()
                .find(|l| l.trim_start().starts_with("discord_webhook"))
                .and_then(|l| l.split('=').nth(1))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
}

/// List UPS names NUT knows about (`upsc -l`), or bail with a clear
/// error if `upsd`/NUT isn't reachable at all — the plugin is useless
/// without it and should say so plainly rather than fail mysteriously
/// three calls deeper.
fn detect_ups_name() -> Result<String> {
    let output = Command::new("upsc")
        .arg("-l")
        .output()
        .context("failed to run `upsc` — is the `nut` package installed and `upsd` running?")?;

    if !output.status.success() {
        anyhow::bail!("`upsc -l` failed — check `upsd` is running (systemctl status nut-server)");
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .context("no UPS registered with NUT — configure /etc/nut/ups.conf first")
}

/// Query every field NUT exposes for one UPS into a flat map.
fn query_ups(name: &str) -> Result<HashMap<String, String>> {
    let output = Command::new("upsc")
        .arg(name)
        .output()
        .with_context(|| format!("failed to query UPS '{name}'"))?;

    if !output.status.success() {
        anyhow::bail!("upsc reported an error for UPS '{name}' — is it still connected?");
    }

    let mut map = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((key, val)) = line.split_once(':') {
            map.insert(key.trim().to_string(), val.trim().to_string());
        }
    }
    Ok(map)
}

fn print_status(name: &str, fields: &HashMap<String, String>) {
    let status = fields.get("ups.status").cloned().unwrap_or_default();
    let state = UpsState::from_status_field(&status);
    let charge = fields.get("battery.charge").cloned().unwrap_or_else(|| "?".into());
    let runtime = fields
        .get("battery.runtime")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| format!("{}m {}s", s / 60, s % 60))
        .unwrap_or_else(|| "?".into());
    let load = fields.get("ups.load").cloned().unwrap_or_else(|| "?".into());

    println!("{} {}", "UPS:".bold(), name);
    println!("  status:  {}", match state {
        UpsState::OnLine => state.label().green(),
        UpsState::OnBattery => state.label().yellow(),
        UpsState::LowBattery => state.label().red().bold(),
        UpsState::Unknown(_) => state.label().dimmed(),
    });
    println!("  charge:  {charge}%");
    println!("  runtime: {runtime} remaining");
    println!("  load:    {load}%");
}

async fn cmd_status(ups_arg: Option<&str>) -> Result<()> {
    let name = match ups_arg {
        Some(n) => n.to_string(),
        None => detect_ups_name()?,
    };
    let fields = query_ups(&name)?;
    print_status(&name, &fields);
    Ok(())
}

async fn cmd_watch(
    ups_arg: Option<&str>,
    webhook: Option<&str>,
    interval: u64,
    shutdown_at: u32,
    heartbeat_every: u32,
) -> Result<()> {
    let name = match ups_arg {
        Some(n) => n.to_string(),
        None => detect_ups_name()?,
    };
    println!("{} Watching UPS '{}' every {interval}s", "→".blue(), name.bold());
    if webhook.is_none() {
        println!(
            "  {} no Discord webhook configured — alerts will only print to the journal",
            "⚠".yellow()
        );
    }

    let mut last_state: Option<UpsState> = None;
    let mut shutdown_triggered = false;
    let mut poll_count: u32 = 0;

    loop {
        poll_count += 1;
        match query_ups(&name) {
            Ok(fields) => {
                let status = fields.get("ups.status").cloned().unwrap_or_default();
                let state = UpsState::from_status_field(&status);
                let charge: u32 = fields
                    .get("battery.charge")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(100);

                let changed = last_state.as_ref() != Some(&state);
                let heartbeat_due = heartbeat_every > 0 && poll_count % heartbeat_every == 0;

                if changed || heartbeat_due {
                    print_status(&name, &fields);
                    if let Some(url) = webhook {
                        let _ = send_discord_alert(url, &name, &state, &fields).await;
                    }
                }

                if state == UpsState::LowBattery || (charge > 0 && charge <= shutdown_at) {
                    if !shutdown_triggered {
                        shutdown_triggered = true;
                        println!("{} Battery at {charge}% — starting safe shutdown", "✖".red().bold());
                        if let Some(url) = webhook {
                            let _ = send_shutdown_alert(url, &name, charge).await;
                        }
                        run_safe_shutdown();
                    }
                } else {
                    shutdown_triggered = false;
                }

                last_state = Some(state);
            }
            Err(e) => {
                eprintln!("{} {e}", "⚠".yellow());
            }
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

/// Snapshot state before power actually runs out, then power off —
/// preferred over letting the box brown out mid-write. `mnctl backup
/// create` and `poweroff` both fail loudly into the log if something's
/// wrong rather than silently; this is deliberately the last thing that
/// runs, so a failure here shouldn't block the actual shutdown.
fn run_safe_shutdown() {
    println!("  {} mnctl backup create --tag pre-power-loss", "→".blue());
    let _ = Command::new("mnctl")
        .args(["backup", "create", "--tag", "pre-power-loss"])
        .status();

    println!("  {} systemctl poweroff", "→".blue());
    let _ = Command::new("systemctl").arg("poweroff").status();
}

/// Discord message component type IDs (Components V2). No `content`, no
/// `embeds` — the whole message body is built out of these instead.
const COMPONENT_CONTAINER: u8 = 17;
const COMPONENT_TEXT_DISPLAY: u8 = 10;
/// Message flag that opts a message into Components V2 rendering.
const FLAG_IS_COMPONENTS_V2: u32 = 1 << 15;

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "monolith".to_string())
        .trim()
        .to_string()
}

/// Wrap `text` in a single-container Components V2 payload with `color`
/// as the container's accent bar. This is the entire message shape the
/// bot ever sends — no embeds, no emoji, just a container + text.
fn container_message(text: String, color: u32) -> serde_json::Value {
    serde_json::json!({
        "flags": FLAG_IS_COMPONENTS_V2,
        "components": [{
            "type": COMPONENT_CONTAINER,
            "accent_color": color,
            "components": [{
                "type": COMPONENT_TEXT_DISPLAY,
                "content": text,
            }],
        }],
    })
}

async fn send_discord_alert(
    webhook: &str,
    ups_name: &str,
    state: &UpsState,
    fields: &HashMap<String, String>,
) -> Result<()> {
    let charge = fields.get("battery.charge").cloned().unwrap_or_else(|| "?".into());
    let runtime = fields
        .get("battery.runtime")
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| format!("{}m {}s", s / 60, s % 60))
        .unwrap_or_else(|| "?".into());

    let text = format!(
        "**UPS {ups_name} — {}**\nHost: {}\nCharge: {charge}%\nRuntime left: {runtime}",
        state.label(),
        hostname(),
    );

    post_discord(webhook, &container_message(text, state.discord_color())).await
}

async fn send_shutdown_alert(webhook: &str, ups_name: &str, charge: u32) -> Result<()> {
    let text = format!(
        "**{} shutting down — UPS {ups_name} at {charge}%**\nSnapshotting via `mnctl backup create`, then powering off.",
        hostname(),
    );

    post_discord(webhook, &container_message(text, 0xe74c3c)).await
}

async fn post_discord(webhook: &str, payload: &serde_json::Value) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build HTTP client")?;

    let resp = client
        .post(webhook)
        .json(payload)
        .send()
        .await
        .context("failed to reach Discord webhook")?;

    if !resp.status().is_success() {
        anyhow::bail!("Discord webhook returned HTTP {}", resp.status());
    }
    Ok(())
}
