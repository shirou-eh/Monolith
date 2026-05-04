//! Custom ISO builder.
//!
//! Wraps the shipped `iso/build-iso.sh` helper around `mkarchiso` so that
//! Monolith operators can produce bootable install media without leaving
//! the `mnctl` umbrella.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Args)]
pub struct IsoArgs {
    #[command(subcommand)]
    command: IsoCommand,
}

#[derive(Subcommand)]
enum IsoCommand {
    /// Build a Monolith OS ISO image (calls into `mkarchiso`)
    Build {
        /// Output directory for the ISO file
        #[arg(long, default_value = "./out")]
        out: PathBuf,
        /// Optional path to a Monolith release tarball to vendor into the ISO
        #[arg(long)]
        release_tar: Option<PathBuf>,
        /// Override the archiso profile directory
        #[arg(long)]
        profile: Option<PathBuf>,
        /// Run with sudo (mkarchiso requires root). Pass `--sudo=false`
        /// when invoking from a script that already has root.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        sudo: bool,
        /// Resource tier to bake into the ISO's default monolith.toml
        /// (lite, full, or pro). The lite tier also strips the
        /// monitoring stack out of the ISO package list.
        #[arg(long, default_value = "full")]
        tier: String,
        /// Override the iso_version string written into profiledef.sh.
        /// CI uses this to pin the version to a release tag.
        #[arg(long)]
        version: Option<String>,
    },
    /// Show ISO build dependencies and verify they are available
    Doctor,
    /// Print the path to the bundled archiso profile
    ProfilePath,
}

impl IsoArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            IsoCommand::Build {
                out,
                release_tar,
                profile,
                sudo,
                tier,
                version,
            } => build(
                &out,
                release_tar.as_deref(),
                profile.as_deref(),
                sudo,
                &tier,
                version.as_deref(),
            ),
            IsoCommand::Doctor => doctor(),
            IsoCommand::ProfilePath => profile_path(),
        }
    }
}

fn locate_helper() -> Option<PathBuf> {
    for candidate in [
        "/usr/share/monolith/iso/build-iso.sh",
        "/usr/local/share/monolith/iso/build-iso.sh",
        "iso/build-iso.sh",
        "./iso/build-iso.sh",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn locate_profile() -> Option<PathBuf> {
    for candidate in [
        "/usr/share/monolith/iso/profile",
        "/usr/local/share/monolith/iso/profile",
        "iso/profile",
        "./iso/profile",
    ] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn build(
    out_dir: &Path,
    release_tar: Option<&Path>,
    profile: Option<&Path>,
    sudo: bool,
    tier: &str,
    version: Option<&str>,
) -> Result<()> {
    if !matches!(tier, "lite" | "full" | "pro") {
        anyhow::bail!("unknown --tier '{tier}'. Use one of: lite, full, pro");
    }
    let helper = locate_helper().ok_or_else(|| {
        anyhow::anyhow!(
            "iso/build-iso.sh not found. Are you running from the Monolith repo or a Monolith install?"
        )
    })?;

    let mut cmd_args: Vec<String> = vec![
        helper.to_string_lossy().into_owned(),
        "--out".to_string(),
        out_dir.to_string_lossy().into_owned(),
        "--tier".to_string(),
        tier.to_string(),
    ];
    if let Some(p) = profile {
        cmd_args.push("--profile".to_string());
        cmd_args.push(p.to_string_lossy().into_owned());
    }
    if let Some(tar) = release_tar {
        cmd_args.push("--release-tar".to_string());
        cmd_args.push(tar.to_string_lossy().into_owned());
    }
    if let Some(v) = version {
        cmd_args.push("--version".to_string());
        cmd_args.push(v.to_string());
    }

    println!(
        "{} Building Monolith OS ISO (tier={}) via mkarchiso → {}",
        "→".blue(),
        tier.bold(),
        out_dir.display().to_string().bold()
    );

    let mut command = if sudo && unsafe { libc_geteuid() } != 0 {
        let mut c = Command::new("sudo");
        c.arg("--").args(&cmd_args);
        c
    } else {
        let mut iter = cmd_args.into_iter();
        let bin = iter.next().expect("at least one arg");
        let mut c = Command::new(bin);
        c.args(iter);
        c
    };

    let status = command.status().context("failed to run iso build script")?;
    if !status.success() {
        anyhow::bail!(
            "ISO build exited non-zero ({})",
            status.code().unwrap_or(-1)
        );
    }
    println!(
        "{} ISO build complete. Look in: {}",
        "●".green(),
        out_dir.display().to_string().bold()
    );
    Ok(())
}

fn doctor() -> Result<()> {
    println!("{}", "ISO Build Doctor:".bold().underline());
    let checks: &[(&str, &str)] = &[
        ("mkarchiso", "Arch ISO builder (pacman -S archiso)"),
        ("grub-mkrescue", "GRUB rescue ISO support (pacman -S grub)"),
        ("xorriso", "ISO 9660 builder (pacman -S libisoburn)"),
        ("mksquashfs", "SquashFS support (pacman -S squashfs-tools)"),
    ];
    let mut ok = true;
    for (bin, hint) in checks {
        match which::which(bin) {
            Ok(p) => println!("  {} {:<14} {}", "●".green(), bin, p.display()),
            Err(_) => {
                println!("  {} {:<14} missing — {}", "●".red(), bin, hint);
                ok = false;
            }
        }
    }

    println!();
    if let Some(profile) = locate_profile() {
        println!("  Profile dir: {}", profile.display());
    } else {
        println!("  {} archiso profile dir not found", "●".yellow());
        ok = false;
    }
    if let Some(helper) = locate_helper() {
        println!("  Helper:      {}", helper.display());
    } else {
        println!("  {} build-iso.sh helper not found", "●".yellow());
        ok = false;
    }

    println!();
    if ok {
        println!("{}", "All ISO build prerequisites available.".green());
    } else {
        println!(
            "{}",
            "Some prerequisites are missing. Install them before running `mnctl iso build`."
                .yellow()
        );
    }
    Ok(())
}

fn profile_path() -> Result<()> {
    let profile = locate_profile().ok_or_else(|| anyhow::anyhow!("archiso profile not found"))?;
    println!("{}", profile.display());
    Ok(())
}

#[allow(non_snake_case)]
unsafe fn libc_geteuid() -> u32 {
    // Avoid pulling in the full libc crate just for this.
    extern "C" {
        fn geteuid() -> u32;
    }
    geteuid()
}
