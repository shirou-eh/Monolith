use std::io::{self, BufRead, BufReader};
use std::process::Stdio;
use std::sync::mpsc;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

/// Preset keyboard layouts offered on the Keyboard step. Not exhaustive
/// (localectl knows hundreds) — covers the common cases with an actual
/// selectable list rather than a text hint nobody could act on.
const KEYBOARD_LAYOUTS: &[&str] = &[
    "us", "uk", "de", "fr", "es", "it", "pt", "nl", "se", "no", "dk", "fi", "pl", "cz", "gr", "ru",
    "ua", "tr", "jp", "br",
];

/// Preset timezones offered on the Timezone step, same rationale as
/// KEYBOARD_LAYOUTS above.
const TIMEZONES: &[&str] = &[
    "UTC",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/Sao_Paulo",
    "Europe/London",
    "Europe/Paris",
    "Europe/Berlin",
    "Europe/Madrid",
    "Europe/Moscow",
    "Asia/Dubai",
    "Asia/Kolkata",
    "Asia/Shanghai",
    "Asia/Tokyo",
    "Australia/Sydney",
    "Africa/Cairo",
];

const MONOLITH_LOGO: &str = r#"
,ggg, ,ggg,_,ggg,                                                                                 _,gggggg,_
dP""Y8dP""Y88P""Y8b                                          ,dPYb,         I8    ,dPYb,         ,d8P""d8P"Y8b,
Yb, `88'  `88'  `88                                          IP'`Yb         I8    IP'`Yb        ,d8'   Y8   "8b,dP
 `"  88    88    88                                          I8  8I  gg  88888888 I8  8I        d8'    `Ybaaad88P'
     88    88    88                                          I8  8'  ""     I8    I8  8'        8P       `""""Y8
     88    88    88    ,ggggg,     ,ggg,,ggg,     ,ggggg,    I8 dP   gg     I8    I8 dPgg,      8b            d8    ,g,
     88    88    88   dP"  "Y8ggg ,8" "8P" "8,   dP"  "Y8ggg I8dP    88     I8    I8dP" "8I     Y8,          ,8P   ,8'8,
     88    88    88  i8'    ,8I   I8   8I   8I  i8'    ,8I   I8P     88    ,I8,   I8P    I8     `Y8,        ,8P'  ,8'  Yb
     88    88    Y8,,d8,   ,d8'  ,dP   8I   Yb,,d8,   ,d8'  ,d8b,_ _,88,_ ,d88b, ,d8     I8,     `Y8b,,__,,d8P'  ,8'_   8)
     88    88    `Y8P"Y8888P"    8P'   8I   `Y8P"Y8888P"    8P'"Y888P""Y888P""Y8888P     `Y8       `"Y8888P"'    P' "YY8P8P
"#;

/// Written to `/mnt/etc/os-release` at install time so every tool that
/// reads it — fastfetch, neofetch, lsb_release, desktop "About" panels —
/// identifies the machine as Monolith rather than plain Arch. `ID_LIKE`
/// stays "arch" so anything checking compatibility (AUR helpers, some
/// pacman hooks) keeps working unmodified.
const MONOLITH_OS_RELEASE: &str = r#"NAME="Monolith OS"
PRETTY_NAME="Monolith OS 1.4.0 (Diorite)"
ID=monolith
ID_LIKE=arch
BUILD_ID=1.4.0
VERSION="1.4.0 (Diorite)"
VERSION_ID=1.4.0
VERSION_CODENAME=diorite
ANSI_COLOR="38;2;80;200;120"
LOGO=monolith
HOME_URL="https://shirou-eh.github.io/Monolith-website/"
DOCUMENTATION_URL="https://github.com/shirou-eh/Monolith/blob/main/README.md"
SUPPORT_URL="https://github.com/shirou-eh/Monolith/discussions"
BUG_REPORT_URL="https://github.com/shirou-eh/Monolith/issues"
"#;

#[derive(Clone, PartialEq)]
enum Step {
    Welcome,
    Keyboard,
    DiskSelection,
    Encryption,
    Timezone,
    Network,
    UserCreation,
    Packages,
    Review,
    Installing,
    Complete,
}

struct InstallerApp {
    step: Step,
    hostname: String,
    username: String,
    timezone: String,
    disk: String,
    use_encryption: bool,
    packages: Vec<(String, bool)>,
    keyboard_layout: String,
    disk_list: Vec<String>,
    disk_list_state: ListState,
    keyboard_list_state: ListState,
    timezone_list_state: ListState,
    package_list_state: ListState,
    should_quit: bool,
    install_progress: u16,
    install_log: Vec<String>,
    install_started: bool,
    install_failed: bool,
}

impl InstallerApp {
    fn new() -> Self {
        let mut keyboard_list_state = ListState::default();
        keyboard_list_state.select(Some(0));
        let mut timezone_list_state = ListState::default();
        timezone_list_state.select(Some(0));
        let mut package_list_state = ListState::default();
        package_list_state.select(Some(0));

        Self {
            step: Step::Welcome,
            hostname: String::new(),
            username: String::new(),
            timezone: "UTC".to_string(),
            disk: String::new(),
            use_encryption: false,
            packages: vec![
                ("Docker + Docker Compose".to_string(), true),
                (
                    "Monitoring stack (Prometheus + Grafana + Loki)".to_string(),
                    true,
                ),
                ("Game server tools".to_string(), false),
                ("Development tools (git, vim, tmux, etc.)".to_string(), true),
            ],
            keyboard_layout: "us".to_string(),
            disk_list: vec![],
            disk_list_state: ListState::default(),
            keyboard_list_state,
            timezone_list_state,
            package_list_state,
            should_quit: false,
            install_progress: 0,
            install_log: Vec::new(),
            install_started: false,
            install_failed: false,
        }
    }

    fn next_step(&mut self) {
        self.step = match self.step {
            Step::Welcome => Step::Keyboard,
            Step::Keyboard => Step::DiskSelection,
            Step::DiskSelection => Step::Encryption,
            Step::Encryption => Step::Timezone,
            Step::Timezone => Step::Network,
            Step::Network => Step::UserCreation,
            Step::UserCreation => Step::Packages,
            Step::Packages => Step::Review,
            Step::Review => Step::Installing,
            Step::Installing => Step::Complete,
            Step::Complete => Step::Complete,
        };
    }

    fn prev_step(&mut self) {
        self.step = match self.step {
            Step::Welcome => Step::Welcome,
            Step::Keyboard => Step::Welcome,
            Step::DiskSelection => Step::Keyboard,
            Step::Encryption => Step::DiskSelection,
            Step::Timezone => Step::Encryption,
            Step::Network => Step::Timezone,
            Step::UserCreation => Step::Network,
            Step::Packages => Step::UserCreation,
            Step::Review => Step::Packages,
            Step::Installing => Step::Installing,
            Step::Complete => Step::Complete,
        };
    }

    fn step_number(&self) -> u8 {
        match self.step {
            Step::Welcome => 1,
            Step::Keyboard => 2,
            Step::DiskSelection => 3,
            Step::Encryption => 4,
            Step::Timezone => 5,
            Step::Network => 6,
            Step::UserCreation => 7,
            Step::Packages => 8,
            Step::Review => 9,
            Step::Installing => 10,
            Step::Complete => 10,
        }
    }
}

#[allow(dead_code)]
enum InstallMsg {
    Progress(u16),
    Log(String),
    Done,
    Error(String),
}

fn run_install_step(
    tx: &mpsc::Sender<InstallMsg>,
    progress: u16,
    desc: &str,
    cmd: &str,
    args: &[&str],
) -> bool {
    let _ = tx.send(InstallMsg::Log(desc.to_string()));
    let _ = tx.send(InstallMsg::Progress(progress));
    match std::process::Command::new(cmd).args(args).output() {
        Ok(o) if o.status.success() => {
            let _ = tx.send(InstallMsg::Log(format!("  [ok] {desc}")));
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let _ = tx.send(InstallMsg::Log(format!("  [warn] {desc}: {stderr}")));
            true // non-fatal, continue
        }
        Err(e) => {
            let _ = tx.send(InstallMsg::Log(format!("  [err] {desc}: {e}")));
            true // still continue, some commands may not be available in all environments
        }
    }
}

/// Like `run_install_step`, but for steps where failure genuinely means
/// nothing after it can be trusted — partitioning, formatting, mounting,
/// pacstrap. The plain helper deliberately treats every failure as
/// "log a warning and keep going" (right for soft steps, e.g. an
/// optional package failing to install), but the same leniency applied
/// here meant a completely failed install — wrong disk, disk in use,
/// pacstrap unable to reach a mirror — still crawled all the way to a
/// fraudulent "Installation complete!" screen instead of stopping.
/// Returns false on failure so the caller can abort.
fn run_critical_step(
    tx: &mpsc::Sender<InstallMsg>,
    progress: u16,
    desc: &str,
    cmd: &str,
    args: &[&str],
) -> bool {
    let _ = tx.send(InstallMsg::Log(desc.to_string()));
    let _ = tx.send(InstallMsg::Progress(progress));
    match std::process::Command::new(cmd).args(args).output() {
        Ok(o) if o.status.success() => {
            let _ = tx.send(InstallMsg::Log(format!("  [ok] {desc}")));
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            let _ = tx.send(InstallMsg::Log(format!("  [err] {desc}: {stderr}")));
            false
        }
        Err(e) => {
            let _ = tx.send(InstallMsg::Log(format!("  [err] {desc}: {e}")));
            false
        }
    }
}

/// Like `run_install_step`, but streams the child's stdout/stderr into
/// the install log line-by-line as it runs instead of buffering
/// everything until the process exits, and returns whether it actually
/// succeeded (like `run_critical_step`) rather than always reporting
/// true — callers that can't tolerate this step failing (pacstrap)
/// check the result and abort; callers installing an optional package
/// group can ignore it and keep going, same as before.
///
/// Exists because `pacstrap`/`pacman -S` were run through the buffered
/// helper above, so the TUI held one static percentage on screen for
/// however long the whole download took — anywhere from seconds to
/// several minutes depending on mirror speed — with zero visible
/// feedback in between. Reported as "downloads terribly": not that the
/// download itself was slower, but that a live install and a hung one
/// looked identical. pacman's own progress bars use carriage returns,
/// which line-based reading can't reproduce faithfully, but forwarding
/// each newline-terminated chunk (which pacman does emit between
/// packages/files) is enough to turn "frozen screen" into "visibly
/// making progress".
fn run_install_step_streaming(
    tx: &mpsc::Sender<InstallMsg>,
    progress: u16,
    desc: &str,
    cmd: &str,
    args: &[&str],
) -> bool {
    let _ = tx.send(InstallMsg::Log(desc.to_string()));
    let _ = tx.send(InstallMsg::Progress(progress));

    let mut child = match std::process::Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(InstallMsg::Log(format!("  [err] {desc}: {e}")));
            return false;
        }
    };

    let stdout_handle = child.stdout.take().map(|s| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(s).lines().map_while(std::result::Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    let _ = tx.send(InstallMsg::Log(format!("  {line}")));
                }
            }
        })
    });
    let stderr_handle = child.stderr.take().map(|s| {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(s).lines().map_while(std::result::Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    let _ = tx.send(InstallMsg::Log(format!("  {line}")));
                }
            }
        })
    });
    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }

    match child.wait() {
        Ok(status) if status.success() => {
            let _ = tx.send(InstallMsg::Log(format!("  [ok] {desc}")));
            true
        }
        Ok(status) => {
            let _ = tx.send(InstallMsg::Log(format!(
                "  [err] {desc}: exited with {status}"
            )));
            false
        }
        Err(e) => {
            let _ = tx.send(InstallMsg::Log(format!("  [err] {desc}: {e}")));
            false
        }
    }
}

/// Resolves a block device's filesystem/partition UUID via `blkid`, for
/// writing stable `root=UUID=...` boot-loader entries instead of a raw
/// device path that can shift on the next boot.
fn blkid_uuid(dev: &str) -> Option<String> {
    let out = std::process::Command::new("blkid")
        .args(["-s", "UUID", "-o", "value", dev])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

struct InstallConfig {
    disk: String,
    hostname: String,
    username: String,
    timezone: String,
    use_encryption: bool,
    keyboard_layout: String,
    packages: Vec<(String, bool)>,
}

fn spawn_installer(tx: mpsc::Sender<InstallMsg>, cfg: InstallConfig) {
    std::thread::spawn(move || {
        let target_disk = if cfg.disk.is_empty() {
            "/dev/sda".to_string()
        } else {
            let name = cfg.disk.split_whitespace().next().unwrap_or("sda");
            format!("/dev/{name}")
        };
        let hn = if cfg.hostname.is_empty() {
            "monolith"
        } else {
            &cfg.hostname
        };
        let user = if cfg.username.is_empty() {
            "admin"
        } else {
            &cfg.username
        };
        let timezone = &cfg.timezone;
        let use_encryption = cfg.use_encryption;
        let keyboard_layout = &cfg.keyboard_layout;
        let packages = &cfg.packages;

        // Aborts the install immediately on a failed critical step
        // instead of letting the rest of spawn_installer march on top
        // of a disk that was never actually partitioned/formatted/
        // mounted. InstallMsg::Error existed before this fix but was
        // never sent from anywhere — a totally failed install (wrong
        // disk, disk in use, no network for pacstrap) still ran every
        // remaining step against nothing and finished on a fraudulent
        // "Installation complete!" screen.
        macro_rules! critical {
            ($result:expr, $msg:expr) => {
                if !$result {
                    let _ = tx.send(InstallMsg::Error($msg.to_string()));
                    return;
                }
            };
        }

        // Step 1: Partition disk
        critical!(
            run_critical_step(
                &tx,
                5,
                &format!("Partitioning {target_disk}..."),
                "sgdisk",
                &[
                    "-Z",
                    "-n",
                    "1:0:+512M",
                    "-t",
                    "1:ef00",
                    "-n",
                    "2:0:0",
                    "-t",
                    "2:8300",
                    &target_disk,
                ],
            ),
            format!("Partitioning {target_disk} failed — it may not exist or may be in use.")
        );

        // Step 1b: Make sure the kernel actually picked up the new
        // partition table (and created the /dev/sdaN device nodes)
        // before the very next command tries to format one of them.
        // sgdisk usually triggers this itself, but not reliably enough
        // across every environment to bet the rest of the install on
        // it — a race here means mkfs targets a device node that
        // doesn't exist yet.
        run_install_step(
            &tx,
            12,
            "Reloading partition table...",
            "partprobe",
            &[&target_disk],
        );
        let _ = std::process::Command::new("udevadm")
            .args(["settle"])
            .output();

        // Step 2: Format partitions
        critical!(
            run_critical_step(
                &tx,
                15,
                "Formatting EFI partition...",
                "mkfs.fat",
                &["-F32", &format!("{target_disk}1")],
            ),
            "Formatting the EFI partition failed."
        );
        if use_encryption {
            critical!(
                run_critical_step(
                    &tx,
                    18,
                    "Setting up LUKS encryption...",
                    "cryptsetup",
                    &["luksFormat", "--batch-mode", &format!("{target_disk}2")],
                ),
                "Setting up LUKS encryption failed."
            );
            critical!(
                run_critical_step(
                    &tx,
                    20,
                    "Opening encrypted volume...",
                    "cryptsetup",
                    &["open", &format!("{target_disk}2"), "cryptroot"],
                ),
                "Opening the encrypted volume failed."
            );
            critical!(
                run_critical_step(
                    &tx,
                    22,
                    "Formatting root (btrfs)...",
                    "mkfs.btrfs",
                    &["-f", "/dev/mapper/cryptroot"],
                ),
                "Formatting the root filesystem failed."
            );
        } else {
            critical!(
                run_critical_step(
                    &tx,
                    20,
                    "Formatting root (btrfs)...",
                    "mkfs.btrfs",
                    &["-f", &format!("{target_disk}2")],
                ),
                "Formatting the root filesystem failed."
            );
        }

        // Step 3: Mount and create subvolumes
        let root_dev = if use_encryption {
            "/dev/mapper/cryptroot".to_string()
        } else {
            format!("{target_disk}2")
        };
        critical!(
            run_critical_step(
                &tx,
                25,
                "Mounting root (top-level)...",
                "mount",
                &[&root_dev, "/mnt"]
            ),
            "Mounting the root filesystem failed."
        );
        for subvol in &["@", "@home", "@snapshots", "@log", "@cache"] {
            run_install_step(
                &tx,
                27,
                &format!("Creating subvolume {subvol}..."),
                "btrfs",
                &["subvolume", "create", &format!("/mnt/{subvol}")],
            );
        }
        // The subvolumes above were created under the *top-level*
        // volume, which is not what should actually end up mounted as
        // root. Without this remount, pacstrap installs straight into
        // the top-level volume and @/@home/@snapshots/@log/@cache just
        // sit there empty and unused forever — the whole
        // snapshot-friendly layout was decorative, never actually
        // mounted anywhere. Remount through subvol=@ (and mount the
        // rest at their real paths) so everything from here on lands
        // in the intended subvolume.
        run_install_step(
            &tx,
            28,
            "Unmounting top-level volume...",
            "umount",
            &["/mnt"],
        );
        critical!(
            run_critical_step(
                &tx,
                29,
                "Mounting @ subvolume as root...",
                "mount",
                &["-o", "subvol=@,compress=zstd", &root_dev, "/mnt"]
            ),
            "Mounting the @ subvolume as root failed."
        );
        let _ = std::fs::create_dir_all("/mnt/home");
        let _ = std::fs::create_dir_all("/mnt/.snapshots");
        let _ = std::fs::create_dir_all("/mnt/var/log");
        let _ = std::fs::create_dir_all("/mnt/var/cache");
        let _ = std::fs::create_dir_all("/mnt/boot");
        run_install_step(
            &tx,
            30,
            "Mounting @home subvolume...",
            "mount",
            &["-o", "subvol=@home,compress=zstd", &root_dev, "/mnt/home"],
        );
        run_install_step(
            &tx,
            30,
            "Mounting @snapshots subvolume...",
            "mount",
            &[
                "-o",
                "subvol=@snapshots,compress=zstd",
                &root_dev,
                "/mnt/.snapshots",
            ],
        );
        run_install_step(
            &tx,
            31,
            "Mounting @log subvolume...",
            "mount",
            &["-o", "subvol=@log,compress=zstd", &root_dev, "/mnt/var/log"],
        );
        run_install_step(
            &tx,
            31,
            "Mounting @cache subvolume...",
            "mount",
            &[
                "-o",
                "subvol=@cache,compress=zstd",
                &root_dev,
                "/mnt/var/cache",
            ],
        );
        // Mount the real EFI System Partition at /mnt/boot. Without
        // this, `bootctl install` later writes the systemd-boot EFI
        // binary into an ordinary directory inside the btrfs root
        // instead of onto the actual ESP, so the firmware finds
        // nothing bootable there at all. This was, on its own, enough
        // to explain "doesn't even boot" regardless of fstab.
        critical!(
            run_critical_step(
                &tx,
                32,
                "Mounting EFI system partition...",
                "mount",
                &[&format!("{target_disk}1"), "/mnt/boot"]
            ),
            "Mounting the EFI system partition failed."
        );

        // Step 4: Install base system. Streamed (not the buffered
        // helper) so the TUI shows real package/download progress
        // instead of sitting frozen on one percentage for the whole
        // pacstrap run.
        critical!(
            run_install_step_streaming(
                &tx,
                35,
                "Installing base system (pacstrap)...",
                "pacstrap",
                &[
                    "/mnt",
                    "base",
                    "linux",
                    "linux-firmware",
                    "btrfs-progs",
                    "networkmanager",
                    "sudo",
                    "openssh",
                    "nftables",
                ],
            ),
            "Installing the base system (pacstrap) failed — check network connectivity and mirror availability."
        );

        // Step 4b: LUKS needs the `encrypt` mkinitcpio hook to unlock
        // the root device at boot — it is NOT in Arch's default HOOKS
        // array, and pacstrap already generated one initramfs using
        // that default while installing the `linux` package. Without
        // this, an encrypted install partitions/formats/installs fine
        // and then can never actually decrypt its own root at boot.
        if use_encryption {
            let _ = tx.send(InstallMsg::Log(
                "Enabling LUKS unlock in initramfs...".to_string(),
            ));
            if let Ok(conf) = std::fs::read_to_string("/mnt/etc/mkinitcpio.conf") {
                if !conf.contains("encrypt") {
                    let updated =
                        conf.replacen("block filesystems", "block encrypt filesystems", 1);
                    let _ = std::fs::write("/mnt/etc/mkinitcpio.conf", updated);
                }
            }
            run_install_step(
                &tx,
                33,
                "Regenerating initramfs with encryption support...",
                "arch-chroot",
                &["/mnt", "mkinitcpio", "-P"],
            );
        }

        // Step 5: Generate fstab. Must go through Command directly
        // (not run_install_step, which only logs [ok]/[warn] and
        // throws the actual command output away) because genfstab's
        // stdout *is* the fstab — without writing it to
        // /mnt/etc/fstab, the installed system has no record of which
        // partitions/subvolumes to mount, so nothing (root included)
        // mounts at boot. This was the most direct cause of "doesn't
        // even boot".
        let _ = tx.send(InstallMsg::Log("Generating fstab...".to_string()));
        let _ = tx.send(InstallMsg::Progress(55));
        let fstab_ok = match std::process::Command::new("genfstab")
            .args(["-U", "/mnt"])
            .output()
        {
            Ok(o) if o.status.success() => match std::fs::write("/mnt/etc/fstab", &o.stdout) {
                Ok(()) => {
                    let _ = tx.send(InstallMsg::Log("  [ok] Generating fstab...".to_string()));
                    true
                }
                Err(e) => {
                    let _ = tx.send(InstallMsg::Log(format!(
                        "  [err] writing /mnt/etc/fstab: {e}"
                    )));
                    false
                }
            },
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let _ = tx.send(InstallMsg::Log(format!(
                    "  [err] genfstab failed: {stderr}"
                )));
                false
            }
            Err(e) => {
                let _ = tx.send(InstallMsg::Log(format!("  [err] genfstab: {e}")));
                false
            }
        };
        critical!(
            fstab_ok,
            "Generating /etc/fstab failed — the installed system would have no record of what to mount at boot."
        );

        // Step 6: Set timezone
        run_install_step(
            &tx,
            60,
            &format!("Setting timezone to {timezone}..."),
            "arch-chroot",
            &[
                "/mnt",
                "ln",
                "-sf",
                &format!("/usr/share/zoneinfo/{timezone}"),
                "/etc/localtime",
            ],
        );

        // Step 7: Set hostname
        let _ = tx.send(InstallMsg::Log(format!("Setting hostname to {hn}...")));
        let _ = tx.send(InstallMsg::Progress(65));
        let _ = std::fs::write("/mnt/etc/hostname", format!("{hn}\n"));

        // Step 7b: Brand /etc/os-release as Monolith, not bare Arch.
        // Without this, every tool that reads it — fastfetch, neofetch,
        // lsb_release, desktop "About" panels — reports the install as
        // plain "Arch Linux", which is what it's built on but not what
        // it is once mnctl/mnweb/the kernel patches are in place.
        let _ = tx.send(InstallMsg::Log("Writing /etc/os-release...".to_string()));
        let _ = std::fs::write("/mnt/etc/os-release", MONOLITH_OS_RELEASE);

        // Step 7c: Deploy the kernel build script. Without this,
        // `mnctl update kernel` looks for it at
        // /usr/share/monolith/kernel/build.sh, finds nothing (nothing
        // else ever put it there), and silently falls back to
        // `pacman -S monolith-kernel` regardless of what the user asked
        // for. Copied from the live ISO environment (airootfs ships it
        // at the same path) rather than embedded here, so it stays a
        // single source of truth in kernel/build.sh.
        let _ = std::fs::create_dir_all("/mnt/usr/share/monolith");
        if std::path::Path::new("/usr/share/monolith/kernel").exists() {
            run_install_step(
                &tx,
                65,
                "Installing kernel build script...",
                "cp",
                &[
                    "-r",
                    "/usr/share/monolith/kernel",
                    "/mnt/usr/share/monolith/",
                ],
            );
        }

        // Step 8: Set keyboard layout
        run_install_step(
            &tx,
            68,
            &format!("Setting keyboard layout to {keyboard_layout}..."),
            "arch-chroot",
            &["/mnt", "localectl", "set-keymap", keyboard_layout],
        );

        // Step 9: Create user
        run_install_step(
            &tx,
            72,
            &format!("Creating user {user}..."),
            "arch-chroot",
            &[
                "/mnt",
                "useradd",
                "-m",
                "-G",
                "wheel",
                "-s",
                "/bin/bash",
                user,
            ],
        );

        // Step 10: Install bootloader
        critical!(
            run_critical_step(
                &tx,
                78,
                "Installing bootloader (systemd-boot)...",
                "arch-chroot",
                &["/mnt", "bootctl", "install"],
            ),
            "Installing the systemd-boot bootloader failed."
        );

        // Step 10b: Write an actual loader entry. `bootctl install`
        // only puts the systemd-boot EFI binary on the ESP — it does
        // not create anything pointing at a kernel/initramfs, and
        // unlike GRUB there's no os-prober-style auto-detection to
        // fall back on. Without this file the firmware boots straight
        // into an empty systemd-boot menu with nothing to select,
        // independently of whether fstab/mounts are correct. This was
        // the other direct cause of "doesn't even boot".
        let options = if use_encryption {
            match blkid_uuid(&format!("{target_disk}2")) {
                Some(luks_uuid) => format!(
                    "cryptdevice=UUID={luks_uuid}:cryptroot root=/dev/mapper/cryptroot rootflags=subvol=@ rw"
                ),
                None => "root=/dev/mapper/cryptroot rootflags=subvol=@ rw".to_string(),
            }
        } else {
            match blkid_uuid(&root_dev) {
                Some(uuid) => format!("root=UUID={uuid} rootflags=subvol=@ rw"),
                None => format!("root={root_dev} rootflags=subvol=@ rw"),
            }
        };
        let entry = format!(
            "title   Monolith OS\nlinux   /vmlinuz-linux\ninitrd  /initramfs-linux.img\noptions {options}\n"
        );
        let _ = std::fs::create_dir_all("/mnt/boot/loader/entries");
        let _ = std::fs::write(
            "/mnt/boot/loader/loader.conf",
            "default monolith.conf\ntimeout 3\nconsole-mode max\neditor no\n",
        );
        match std::fs::write("/mnt/boot/loader/entries/monolith.conf", entry) {
            Ok(()) => {
                let _ = tx.send(InstallMsg::Log(
                    "  [ok] Writing boot loader entry...".to_string(),
                ));
            }
            Err(e) => {
                let _ = tx.send(InstallMsg::Log(format!(
                    "  [err] writing boot loader entry: {e}"
                )));
            }
        }

        // Step 11: Security hardening
        run_install_step(
            &tx,
            85,
            "Applying security hardening (SSH, nftables)...",
            "arch-chroot",
            &[
                "/mnt",
                "systemctl",
                "enable",
                "nftables",
                "sshd",
                "NetworkManager",
            ],
        );

        // Step 12: Install selected packages
        let selected: Vec<&str> = packages
            .iter()
            .filter(|(_, s)| *s)
            .map(|(n, _)| n.as_str())
            .collect();
        if !selected.is_empty() {
            let _ = tx.send(InstallMsg::Log(format!(
                "Installing packages: {}",
                selected.join(", ")
            )));
            let _ = tx.send(InstallMsg::Progress(90));
            // Map friendly names to actual packages
            for pkg_name in &selected {
                let pkgs: &[&str] = match *pkg_name {
                    s if s.contains("Docker") => &["docker", "docker-compose"],
                    s if s.contains("Monitoring") => &["prometheus", "grafana"],
                    s if s.contains("Game") => &["lib32-gcc-libs", "screen"],
                    s if s.contains("Development") => &["git", "vim", "tmux", "base-devel"],
                    _ => &[],
                };
                if !pkgs.is_empty() {
                    let mut args = vec!["-S", "--noconfirm", "--needed"];
                    args.extend(pkgs.iter());
                    run_install_step_streaming(
                        &tx,
                        92,
                        &format!("Installing {pkg_name}..."),
                        "arch-chroot",
                        &{
                            let mut full = vec!["/mnt", "pacman"];
                            full.extend(args);
                            full
                        },
                    );
                }
            }
        }

        // Step 13: Copy Monolith config
        let _ = tx.send(InstallMsg::Log(
            "Deploying Monolith configuration...".to_string(),
        ));
        let _ = tx.send(InstallMsg::Progress(96));
        let _ = std::fs::create_dir_all("/mnt/etc/monolith");
        let _ = std::fs::copy(
            "/etc/monolith/monolith.toml",
            "/mnt/etc/monolith/monolith.toml",
        );

        // Step 14: Finalize
        let _ = tx.send(InstallMsg::Log("Unmounting filesystems...".to_string()));
        let _ = tx.send(InstallMsg::Progress(98));
        let _ = std::process::Command::new("umount")
            .args(["-R", "/mnt"])
            .output();

        let _ = tx.send(InstallMsg::Progress(100));
        let _ = tx.send(InstallMsg::Done);
    });
}

fn main() -> Result<()> {
    // This is a TUI app with no other argument parsing at all — before
    // this fix, ANY argument (including `--help`) was silently ignored
    // and fell straight into enable_raw_mode() below, which needs a
    // real TTY and fails with a bare "No such device or address (os
    // error 6)" when there isn't one. Caught by the ISO boot self-test
    // running `monolith-installer --help` non-interactively — exactly
    // the class of bug that check exists to catch, just one layer
    // deeper than "does the binary exist and run at all". A CLI tool
    // silently ignoring --help instead of printing help is a real gap
    // on its own, independent of the crash.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("monolith-installer {}", env!("CARGO_PKG_VERSION"));
        println!("Monolith OS TUI installer — partitions, installs the base");
        println!("system, and configures the bootloader. Run with no arguments");
        println!("from a real terminal.");
        println!();
        println!("Usage: monolith-installer");
        println!();
        println!("Options:");
        println!("  -h, --help     Print this message");
        println!("  -V, --version  Print version");
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("monolith-installer {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    enable_raw_mode()
        .context("failed to enter raw terminal mode — monolith-installer needs a real interactive terminal, not a pipe/redirect/non-interactive session")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = InstallerApp::new();

    // Detect disks — filtering out loop/ram/zram/dm/fd/nbd/sr devices
    // from the start. On the live ISO, archiso's squashfs root is
    // mounted from /dev/loop0, so the very first row offered here was
    // the live system's OWN loop device: sgdisk can't repartition it
    // ("Unable to save backup partition table!", exactly the failure
    // the critical-step rework was built to surface) and it must never
    // be selectable as an install target. None of these names can be a
    // real install disk anyway.
    if let Ok(output) = std::process::Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,SIZE,MODEL"])
        .output()
    {
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        app.disk_list = stdout_str
            .lines()
            .filter(|l| {
                let name = l.split_whitespace().next().unwrap_or("");
                !["loop", "ram", "zram", "fd", "dm-", "nbd", "sr"]
                    .iter()
                    .any(|p| name.starts_with(p))
            })
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect();
    }

    let (tx, rx) = mpsc::channel::<InstallMsg>();

    loop {
        terminal.draw(|f| render_ui(f, &mut app))?;

        // Drain install messages
        while let Ok(msg) = rx.try_recv() {
            match msg {
                InstallMsg::Progress(p) => app.install_progress = p,
                InstallMsg::Log(line) => app.install_log.push(line),
                InstallMsg::Done => {
                    app.install_progress = 100;
                    app.next_step();
                }
                InstallMsg::Error(e) => {
                    app.install_log.push(format!("[ERROR] {e}"));
                    app.install_failed = true;
                }
            }
        }

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Dispatched per-step rather than as one flat match
                    // on the keycode. Before this, only DiskSelection
                    // and Encryption had any handling at all —
                    // Keyboard/Timezone/Network/UserCreation/Packages
                    // had none, so hostname/username could never be
                    // typed and layout/timezone/packages could never
                    // be changed. Keying off app.step first makes it
                    // structurally obvious which steps are still dead,
                    // instead of that being an easy-to-miss omission
                    // buried in a single giant match.
                    match app.step {
                        Step::Keyboard => match key.code {
                            KeyCode::Enter => app.next_step(),
                            KeyCode::Esc | KeyCode::Backspace => app.prev_step(),
                            KeyCode::Down => {
                                let i = app.keyboard_list_state.selected().unwrap_or(0);
                                let next = (i + 1).min(KEYBOARD_LAYOUTS.len() - 1);
                                app.keyboard_list_state.select(Some(next));
                                app.keyboard_layout = KEYBOARD_LAYOUTS[next].to_string();
                            }
                            KeyCode::Up => {
                                let i = app.keyboard_list_state.selected().unwrap_or(0);
                                let next = i.saturating_sub(1);
                                app.keyboard_list_state.select(Some(next));
                                app.keyboard_layout = KEYBOARD_LAYOUTS[next].to_string();
                            }
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                        Step::Timezone => match key.code {
                            KeyCode::Enter => app.next_step(),
                            KeyCode::Esc | KeyCode::Backspace => app.prev_step(),
                            KeyCode::Down => {
                                let i = app.timezone_list_state.selected().unwrap_or(0);
                                let next = (i + 1).min(TIMEZONES.len() - 1);
                                app.timezone_list_state.select(Some(next));
                                app.timezone = TIMEZONES[next].to_string();
                            }
                            KeyCode::Up => {
                                let i = app.timezone_list_state.selected().unwrap_or(0);
                                let next = i.saturating_sub(1);
                                app.timezone_list_state.select(Some(next));
                                app.timezone = TIMEZONES[next].to_string();
                            }
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                        Step::DiskSelection => match key.code {
                            KeyCode::Enter => {
                                // Don't advance past an empty disk list —
                                // with no real disks (the filtered-out
                                // loop devices were the only entries),
                                // proceeding would fall back to a
                                // /dev/sda that doesn't exist.
                                // Don't advance past an empty disk list —
                                // with no real disks (the filtered-out
                                // loop devices were the only entries),
                                // proceeding would fall back to a
                                // /dev/sda that doesn't exist.
                                if !app.disk_list.is_empty() {
                                    // The step already tracked which row
                                    // was highlighted but never wrote it
                                    // back into app.disk, so Enter always
                                    // fell through to spawn_installer's own
                                    // "empty disk -> /dev/sda" default no
                                    // matter what was selected on screen.
                                    if let Some(i) = app.disk_list_state.selected() {
                                        if let Some(d) = app.disk_list.get(i) {
                                            app.disk = d.clone();
                                        }
                                    }
                                    app.next_step();
                                }
                            }
                            KeyCode::Esc | KeyCode::Backspace => app.prev_step(),
                            KeyCode::Down => {
                                let i = app.disk_list_state.selected().unwrap_or(0);
                                if i < app.disk_list.len().saturating_sub(1) {
                                    app.disk_list_state.select(Some(i + 1));
                                }
                            }
                            KeyCode::Up => {
                                let i = app.disk_list_state.selected().unwrap_or(0);
                                if i > 0 {
                                    app.disk_list_state.select(Some(i - 1));
                                }
                            }
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                        Step::Encryption => match key.code {
                            KeyCode::Enter => app.next_step(),
                            KeyCode::Esc | KeyCode::Backspace => app.prev_step(),
                            KeyCode::Char(' ') => app.use_encryption = !app.use_encryption,
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                        Step::Network => match key.code {
                            KeyCode::Enter => app.next_step(),
                            KeyCode::Esc => app.prev_step(),
                            KeyCode::Backspace => {
                                app.hostname.pop();
                            }
                            KeyCode::Char(c)
                                if (c.is_ascii_alphanumeric() || c == '-')
                                    && app.hostname.len() < 63 =>
                            {
                                app.hostname.push(c.to_ascii_lowercase());
                            }
                            _ => {}
                        },
                        Step::UserCreation => match key.code {
                            KeyCode::Enter => app.next_step(),
                            KeyCode::Esc => app.prev_step(),
                            KeyCode::Backspace => {
                                app.username.pop();
                            }
                            KeyCode::Char(c)
                                if (c.is_ascii_alphanumeric() || c == '-' || c == '_')
                                    && app.username.len() < 32 =>
                            {
                                app.username.push(c.to_ascii_lowercase());
                            }
                            _ => {}
                        },
                        Step::Packages => match key.code {
                            KeyCode::Enter => app.next_step(),
                            KeyCode::Esc | KeyCode::Backspace => app.prev_step(),
                            KeyCode::Down => {
                                let i = app.package_list_state.selected().unwrap_or(0);
                                if i < app.packages.len().saturating_sub(1) {
                                    app.package_list_state.select(Some(i + 1));
                                }
                            }
                            KeyCode::Up => {
                                let i = app.package_list_state.selected().unwrap_or(0);
                                if i > 0 {
                                    app.package_list_state.select(Some(i - 1));
                                }
                            }
                            KeyCode::Char(' ') => {
                                if let Some(i) = app.package_list_state.selected() {
                                    if let Some(pkg) = app.packages.get_mut(i) {
                                        pkg.1 = !pkg.1;
                                    }
                                }
                            }
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                        Step::Complete => match key.code {
                            KeyCode::Enter => {
                                let _ = std::process::Command::new("systemctl")
                                    .arg("reboot")
                                    .spawn();
                                app.should_quit = true;
                            }
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                        // No input accepted while a live install is in
                        // progress (aborting mid-partition/mid-format
                        // could leave a disk in a worse state than
                        // just letting it finish or fail on its own) —
                        // except once it has actually failed, where
                        // the alternative was leaving the user with no
                        // way out at all short of killing the process.
                        Step::Installing => {
                            if app.install_failed {
                                if let KeyCode::Char('q') = key.code {
                                    app.should_quit = true;
                                }
                            }
                        }
                        Step::Welcome | Step::Review => match key.code {
                            KeyCode::Enter => {
                                if app.step == Step::Review {
                                    app.next_step();
                                    if !app.install_started {
                                        app.install_started = true;
                                        spawn_installer(
                                            tx.clone(),
                                            InstallConfig {
                                                disk: app.disk.clone(),
                                                hostname: app.hostname.clone(),
                                                username: app.username.clone(),
                                                timezone: app.timezone.clone(),
                                                use_encryption: app.use_encryption,
                                                keyboard_layout: app.keyboard_layout.clone(),
                                                packages: app.packages.clone(),
                                            },
                                        );
                                    }
                                } else {
                                    app.next_step();
                                }
                            }
                            KeyCode::Esc | KeyCode::Backspace => app.prev_step(),
                            KeyCode::Char('q') => app.should_quit = true,
                            _ => {}
                        },
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn render_ui(f: &mut Frame, app: &mut InstallerApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Content
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    // Header with step indicator
    let step_text = format!(" Monolith OS Installer  —  Step {}/10", app.step_number());
    let header = Paragraph::new(step_text)
        .style(Style::default().fg(Color::Green).bold())
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    // Content
    f.render_widget(Clear, chunks[1]);
    match app.step {
        Step::Welcome => render_welcome(f, chunks[1]),
        Step::Keyboard => render_keyboard(f, app, chunks[1]),
        Step::DiskSelection => render_disk_selection(f, app, chunks[1]),
        Step::Encryption => render_encryption(f, app, chunks[1]),
        Step::Timezone => render_timezone(f, app, chunks[1]),
        Step::Network => render_network(f, app, chunks[1]),
        Step::UserCreation => render_user(f, app, chunks[1]),
        Step::Packages => render_packages(f, app, chunks[1]),
        Step::Review => render_review(f, app, chunks[1]),
        Step::Installing => render_installing(f, app, chunks[1]),
        Step::Complete => render_complete(f, chunks[1]),
    }

    // Footer — step-aware, since the controls genuinely differ per
    // step (typing vs. Up/Down selection vs. toggling).
    let footer_text = match app.step {
        Step::Complete => " Enter: Reboot  |  q: Quit",
        Step::Installing if app.install_failed => {
            " Installation FAILED — see log above  |  q: Quit"
        }
        Step::Installing => " Installation in progress...",
        Step::Keyboard | Step::Timezone => " Up/Down: Select  |  Enter: Next  |  Esc: Back",
        Step::Network => {
            " Type to edit hostname  |  Backspace: Delete  |  Enter: Next  |  Esc: Back"
        }
        Step::UserCreation => {
            " Type to edit username  |  Backspace: Delete  |  Enter: Next  |  Esc: Back"
        }
        Step::Packages => " Up/Down: Select  |  Space: Toggle  |  Enter: Next  |  Esc: Back",
        Step::Encryption => " Space: Toggle  |  Enter: Next  |  Esc: Back",
        Step::DiskSelection => " Up/Down: Select  |  Enter: Next  |  Esc: Back",
        _ => " Enter: Next  |  Esc: Back  |  q: Quit",
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn render_welcome(f: &mut Frame, area: Rect) {
    let text = format!(
        "{}\n\n    v{} \"Diorite\"\n    Built for the ones who mean it.\n\n\
         \n    System Requirements:\n    \
         - CPU: x86_64 or ARM64\n    \
         - RAM: 2 GB minimum (8 GB recommended)\n    \
         - Disk: 20 GB minimum (100 GB recommended)\n\n    \
         Press Enter to begin installation...",
        MONOLITH_LOGO,
        env!("CARGO_PKG_VERSION")
    );
    let widget = Paragraph::new(text)
        .style(Style::default().fg(Color::Green))
        .block(Block::default().borders(Borders::ALL).title(" Welcome "))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_keyboard(f: &mut Frame, app: &mut InstallerApp, area: Rect) {
    let items: Vec<ListItem> = KEYBOARD_LAYOUTS
        .iter()
        .map(|l| ListItem::new(format!("  {l}")))
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Keyboard Layout (Up/Down to select, Enter to confirm) "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.keyboard_list_state);
}

fn render_disk_selection(f: &mut Frame, app: &mut InstallerApp, area: Rect) {
    if app.disk_list.is_empty() {
        let widget = Paragraph::new(
            "  No suitable disks found.\n\n  \
             The live medium's loop devices are never valid install targets\n  \
             and have been excluded. Attach a real disk to this machine\n  \
             and restart the installer.\n\n  \
             Press q to quit.",
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Installation Disk "),
        );
        f.render_widget(widget, area);
        return;
    }
    let items: Vec<ListItem> = app
        .disk_list
        .iter()
        .map(|d| ListItem::new(format!("  {d}")))
        .collect();

    if app.disk_list_state.selected().is_none() && !app.disk_list.is_empty() {
        app.disk_list_state.select(Some(0));
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Installation Disk (Up/Down to select, Enter to confirm) "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.disk_list_state);
}

fn render_encryption(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let status = if app.use_encryption { "[x]" } else { "[ ]" };
    let text = format!(
        "\n  {status} Enable LUKS2 full-disk encryption\n\n  \
         Press Space to toggle, Enter to continue\n\n  \
         Note: Encryption adds security but requires entering\n  \
         a password at every boot."
    );
    let widget =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Encryption "));
    f.render_widget(widget, area);
}

fn render_timezone(f: &mut Frame, app: &mut InstallerApp, area: Rect) {
    let items: Vec<ListItem> = TIMEZONES
        .iter()
        .map(|t| ListItem::new(format!("  {t}")))
        .collect();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Timezone (Up/Down to select, Enter to confirm) "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.timezone_list_state);
}

fn render_network(f: &mut Frame, app: &InstallerApp, area: Rect) {
    // Trailing block cursor makes clear this is an active text field,
    // not just a display of whatever the default will be — before
    // this rewrote key handling, there was no way to change it at all.
    let text = format!(
        "\n  Hostname: {}█\n\n  \
         Network: DHCP (automatic)\n  \
         DNS: 1.1.1.1, 1.0.0.1\n\n  \
         Type to edit, Backspace to delete, Enter to continue\n  \
         (leave blank to use the default 'monolith')",
        app.hostname
    );
    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Network Configuration "),
    );
    f.render_widget(widget, area);
}

fn render_user(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let text = format!(
        "\n  Username: {}█\n\n  \
         Root login: disabled (recommended)\n  \
         SSH: key-based authentication\n\n  \
         Type to edit, Backspace to delete, Enter to continue\n  \
         (leave blank to use the default 'admin')",
        app.username
    );
    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" User Creation "),
    );
    f.render_widget(widget, area);
}

fn render_packages(f: &mut Frame, app: &mut InstallerApp, area: Rect) {
    let items: Vec<ListItem> = app
        .packages
        .iter()
        .map(|(name, selected)| {
            let checkbox = if *selected { "[x]" } else { "[ ]" };
            ListItem::new(format!("  {checkbox} {name}"))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Additional Packages (Up/Down to select, Space to toggle) "),
        )
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.package_list_state);
}

fn render_review(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let selected_pkgs: Vec<&str> = app
        .packages
        .iter()
        .filter(|(_, s)| *s)
        .map(|(n, _)| n.as_str())
        .collect();

    let text = format!(
        "\n  Installation Summary\n  \
         ═══════════════════════\n\n  \
         Keyboard:   {}\n  \
         Disk:       {}\n  \
         Encryption: {}\n  \
         Timezone:   {}\n  \
         Hostname:   {}\n  \
         Username:   {}\n  \
         Packages:   {}\n\n  \
         Press Enter to begin installation...",
        app.keyboard_layout,
        if app.disk.is_empty() {
            "auto"
        } else {
            &app.disk
        },
        if app.use_encryption { "LUKS2" } else { "none" },
        app.timezone,
        if app.hostname.is_empty() {
            "monolith"
        } else {
            &app.hostname
        },
        if app.username.is_empty() {
            "admin"
        } else {
            &app.username
        },
        selected_pkgs.join(", "),
    );
    let widget = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Review & Install "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_installing(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5)])
        .split(area);

    // Progress bar
    let gauge = ratatui::widgets::Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Installing Monolith OS "),
        )
        .gauge_style(Style::default().fg(Color::Green))
        .percent(app.install_progress)
        .label(format!("{}%", app.install_progress));
    f.render_widget(gauge, chunks[0]);

    // Log output (show last N lines that fit)
    let visible_lines = chunks[1].height.saturating_sub(2) as usize;
    let start = app.install_log.len().saturating_sub(visible_lines);
    let items: Vec<ListItem> = app.install_log[start..]
        .iter()
        .map(|line| {
            let style = if line.contains("[ok]") {
                Style::default().fg(Color::Green)
            } else if line.contains("[err]") || line.contains("[ERROR]") {
                Style::default().fg(Color::Red)
            } else if line.contains("[warn]") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(format!(" {line}")).style(style)
        })
        .collect();

    let log_list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Installation Log "),
    );
    f.render_widget(log_list, chunks[1]);
}

fn render_complete(f: &mut Frame, area: Rect) {
    let text = format!(
        "{}\n\n    Installation complete!\n\n    \
         Monolith OS v{} \"Diorite\" has been installed.\n\n    \
         Remove installation media and press Enter to reboot.\n\n    \
         After reboot, connect via SSH on port 2222:\n    \
         ssh admin@<server-ip> -p 2222\n\n    \
         First steps:\n    \
         - mnctl info system          # Check system info\n    \
         - mnctl monitor status       # View system status\n    \
         - mnctl security audit       # Run security audit\n    \
         - mnctl template list        # Browse application templates",
        MONOLITH_LOGO,
        env!("CARGO_PKG_VERSION")
    );
    let widget = Paragraph::new(text)
        .style(Style::default().fg(Color::Green))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Installation Complete "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
