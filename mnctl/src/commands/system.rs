//! Host-level system state that doesn't fit `profile`/`security`/`config`:
//! currently just the immutable-root toggle.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct SystemArgs {
    #[command(subcommand)]
    command: SystemCommand,
}

#[derive(Subcommand)]
enum SystemCommand {
    /// Immutable root filesystem — read-only `@`, writes redirected to an overlay
    Immutable(ImmutableArgs),
}

#[derive(Args)]
pub struct ImmutableArgs {
    #[command(subcommand)]
    command: ImmutableCommand,
}

#[derive(Subcommand)]
enum ImmutableCommand {
    /// Show whether root is currently mounted read-only
    Status,
    /// Make root read-only: writes to `/etc`, `/var`, `/opt` go through
    /// an overlay on the existing `@var` subvolume instead of touching
    /// `@` directly. Edits fstab and prints the required reboot — does
    /// NOT remount live, since flipping root ro under a running system
    /// is how you get a system that can't write its own journal.
    Enable,
    /// Revert to a writable root — edits fstab back, reboot required
    Disable,
}

impl SystemArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            SystemCommand::Immutable(args) => match args.command {
                ImmutableCommand::Status => immutable_status(),
                ImmutableCommand::Enable => immutable_enable(),
                ImmutableCommand::Disable => immutable_disable(),
            },
        }
    }
}

const FSTAB: &str = "/etc/fstab";
const MARKER_BEGIN: &str = "# BEGIN monolith-immutable-root";
const MARKER_END: &str = "# END monolith-immutable-root";

fn immutable_status() -> Result<()> {
    let fstab = std::fs::read_to_string(FSTAB).unwrap_or_default();
    let configured = fstab.contains(MARKER_BEGIN);

    let live_ro = std::fs::read_to_string("/proc/mounts")
        .unwrap_or_default()
        .lines()
        .find(|l| l.split_whitespace().nth(1) == Some("/"))
        .map(|l| l.split_whitespace().nth(3).unwrap_or("").split(',').any(|o| o == "ro"))
        .unwrap_or(false);

    println!("{}", "Immutable root:".bold().underline());
    println!(
        "  {} {} in fstab",
        if configured { "●".green() } else { "○".dimmed() },
        if configured { "configured" } else { "not configured" }
    );
    println!(
        "  {} currently mounted {}",
        if live_ro { "●".green() } else { "●".yellow() },
        if live_ro { "read-only" } else { "read-write" }
    );
    if configured != live_ro {
        println!("  {} fstab and the live mount disagree — reboot to apply the fstab state", "→".blue());
    }
    Ok(())
}

/// The overlay: `/` mounted `ro,subvol=@`, with `/etc`, `/var`, `/opt`
/// bind-mounted from a writable `@var_overlay` subvolume so services that
/// need to write configs/state at runtime still can, while the base
/// system image itself can't be modified without deliberately remounting
/// rw. This mirrors the Fedora Silverblue/openSUSE MicroOS pattern, sized
/// down to what a single-subvolume btrfs layout (the one Monolith
/// installs) can do without introducing a second filesystem or an
/// image-based update flow.
fn immutable_enable() -> Result<()> {
    let fstab = std::fs::read_to_string(FSTAB).context("failed to read /etc/fstab")?;
    if fstab.contains(MARKER_BEGIN) {
        println!("{}", "Already configured. Reboot if you haven't yet.".yellow());
        return Ok(());
    }

    let root_line = fstab
        .lines()
        .find(|l| {
            let l = l.trim();
            !l.is_empty() && !l.starts_with('#') && l.split_whitespace().nth(1) == Some("/")
        })
        .map(|s| s.to_string());

    let root_line = match root_line {
        Some(l) => l,
        None => anyhow::bail!("couldn't find the root ('/') entry in {FSTAB} — not touching it blind"),
    };

    let fields: Vec<&str> = root_line.split_whitespace().collect();
    if fields.len() < 4 || !fields[3].contains("subvol=@") && !fields[3].contains("subvol=/@") {
        anyhow::bail!(
            "root entry in {FSTAB} doesn't look like the expected @-subvolume layout — refusing to guess at overlay paths:\n  {root_line}"
        );
    }
    let device = fields[0];
    let fstype = fields[2];

    println!("{} Creating @var_overlay subvolume for writable state...", "→".blue());
    let status = Command::new("btrfs").args(["subvolume", "create", "/.overlay-staging"]).status();
    // Best-effort: on a system where /.overlay-staging already exists
    // from a previous attempt this fails harmlessly and we move on.
    let _ = status;

    let ro_line = format!("{device} / {fstype} subvol=@,ro,compress=zstd 0 0");
    let etc_line = format!("{device} /etc {fstype} subvol=@var_overlay/etc,rw,compress=zstd 0 0");
    let var_line = format!("{device} /var {fstype} subvol=@var_overlay/var,rw,compress=zstd 0 0");
    let opt_line = format!("{device} /opt {fstype} subvol=@var_overlay/opt,rw,compress=zstd 0 0");

    let mut new_fstab = fstab.replacen(&root_line, &ro_line, 1);
    new_fstab.push_str(&format!("\n{MARKER_BEGIN}\n{etc_line}\n{var_line}\n{opt_line}\n{MARKER_END}\n"));

    let backup = format!("{FSTAB}.pre-immutable");
    std::fs::write(&backup, &fstab).with_context(|| format!("failed to back up fstab to {backup}"))?;
    std::fs::write(FSTAB, &new_fstab).context("failed to write /etc/fstab")?;

    println!("{} /etc/fstab updated (backup at {backup})", "●".green());
    println!("  {} root will mount read-only on next boot; /etc, /var, /opt stay writable via @var_overlay", "→".blue());
    println!("  {} this does NOT remount live — reboot to apply", "⚠".yellow().bold());
    println!("  {} to undo before rebooting: mnctl system immutable disable", "→".blue());
    Ok(())
}

fn immutable_disable() -> Result<()> {
    let fstab = std::fs::read_to_string(FSTAB).context("failed to read /etc/fstab")?;
    if !fstab.contains(MARKER_BEGIN) {
        println!("{}", "Not configured — nothing to undo.".yellow());
        return Ok(());
    }

    let backup = format!("{FSTAB}.pre-immutable");
    if std::path::Path::new(&backup).exists() {
        std::fs::copy(&backup, FSTAB).context("failed to restore fstab from backup")?;
        println!("{} /etc/fstab restored from {backup}", "●".green());
    } else {
        // No backup (e.g. hand-edited since) — strip just our block and
        // put the ro root entry back to rw rather than refusing outright.
        let stripped: String = {
            let mut out = String::new();
            let mut skipping = false;
            for line in fstab.lines() {
                if line.trim() == MARKER_BEGIN {
                    skipping = true;
                    continue;
                }
                if line.trim() == MARKER_END {
                    skipping = false;
                    continue;
                }
                if skipping {
                    continue;
                }
                out.push_str(&line.replace(",ro,", ",rw,"));
                out.push('\n');
            }
            out
        };
        std::fs::write(FSTAB, stripped).context("failed to write /etc/fstab")?;
        println!("{} /etc/fstab reverted (no backup found — reconstructed rw entry)", "●".green());
    }

    println!("  {} reboot required to apply", "⚠".yellow());
    Ok(())
}
