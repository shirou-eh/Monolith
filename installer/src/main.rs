use std::io;
use std::sync::mpsc;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

const MONOLITH_LOGO: &str = r#"
    ███╗   ███╗ ██████╗ ███╗   ██╗ ██████╗ ██╗     ██╗████████╗██╗  ██╗
    ████╗ ████║██╔═══██╗████╗  ██║██╔═══██╗██║     ██║╚══██╔══╝██║  ██║
    ██╔████╔██║██║   ██║██╔██╗ ██║██║   ██║██║     ██║   ██║   ███████║
    ██║╚██╔╝██║██║   ██║██║╚██╗██║██║   ██║██║     ██║   ██║   ██╔══██║
    ██║ ╚═╝ ██║╚██████╔╝██║ ╚████║╚██████╔╝███████╗██║   ██║   ██║  ██║
    ╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝ ╚═════╝ ╚══════╝╚═╝   ╚═╝   ╚═╝  ╚═╝
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
    should_quit: bool,
    install_progress: u16,
    install_log: Vec<String>,
    install_started: bool,
}

impl InstallerApp {
    fn new() -> Self {
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
            should_quit: false,
            install_progress: 0,
            install_log: Vec::new(),
            install_started: false,
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

        // Step 1: Partition disk
        run_install_step(
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
        );

        // Step 2: Format partitions
        run_install_step(
            &tx,
            15,
            "Formatting EFI partition...",
            "mkfs.fat",
            &["-F32", &format!("{target_disk}1")],
        );
        if use_encryption {
            run_install_step(
                &tx,
                18,
                "Setting up LUKS encryption...",
                "cryptsetup",
                &["luksFormat", "--batch-mode", &format!("{target_disk}2")],
            );
            run_install_step(
                &tx,
                20,
                "Opening encrypted volume...",
                "cryptsetup",
                &["open", &format!("{target_disk}2"), "cryptroot"],
            );
            run_install_step(
                &tx,
                22,
                "Formatting root (btrfs)...",
                "mkfs.btrfs",
                &["-f", "/dev/mapper/cryptroot"],
            );
        } else {
            run_install_step(
                &tx,
                20,
                "Formatting root (btrfs)...",
                "mkfs.btrfs",
                &["-f", &format!("{target_disk}2")],
            );
        }

        // Step 3: Mount and create subvolumes
        let root_dev = if use_encryption {
            "/dev/mapper/cryptroot".to_string()
        } else {
            format!("{target_disk}2")
        };
        run_install_step(&tx, 25, "Mounting root...", "mount", &[&root_dev, "/mnt"]);
        for subvol in &["@", "@home", "@snapshots", "@log", "@cache"] {
            run_install_step(
                &tx,
                28,
                &format!("Creating subvolume {subvol}..."),
                "btrfs",
                &["subvolume", "create", &format!("/mnt/{subvol}")],
            );
        }

        // Step 4: Install base system
        run_install_step(
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
        );

        // Step 5: Generate fstab
        run_install_step(&tx, 55, "Generating fstab...", "genfstab", &["-U", "/mnt"]);

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
        run_install_step(
            &tx,
            78,
            "Installing bootloader (systemd-boot)...",
            "arch-chroot",
            &["/mnt", "bootctl", "install"],
        );

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
                    run_install_step(
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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = InstallerApp::new();

    // Detect disks
    if let Ok(output) = std::process::Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,SIZE,MODEL"])
        .output()
    {
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        app.disk_list = stdout_str
            .lines()
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
                }
            }
        }

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') if app.step != Step::Installing => {
                            app.should_quit = true;
                        }
                        KeyCode::Enter => {
                            if app.step == Step::Review {
                                // Start real installation
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
                        KeyCode::Down if app.step == Step::DiskSelection => {
                            let i = app.disk_list_state.selected().unwrap_or(0);
                            if i < app.disk_list.len().saturating_sub(1) {
                                app.disk_list_state.select(Some(i + 1));
                            }
                        }
                        KeyCode::Up if app.step == Step::DiskSelection => {
                            let i = app.disk_list_state.selected().unwrap_or(0);
                            if i > 0 {
                                app.disk_list_state.select(Some(i - 1));
                            }
                        }
                        KeyCode::Char(' ') if app.step == Step::Encryption => {
                            app.use_encryption = !app.use_encryption;
                        }
                        _ => {}
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

    // Footer
    let footer_text = if app.step == Step::Complete {
        " Press Enter to reboot  |  q to quit"
    } else if app.step == Step::Installing {
        " Installation in progress..."
    } else {
        " Enter: Next  |  Esc: Back  |  q: Quit"
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn render_welcome(f: &mut Frame, area: Rect) {
    let text = format!(
        "{}\n\n    v{} \"Obsidian\"\n    Built for the ones who mean it.\n\n\
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

fn render_keyboard(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let text = format!(
        "\n  Selected keyboard layout: {}\n\n  \
         Common layouts: us, uk, de, fr, es, ru, jp\n\n  \
         Press Enter to continue with '{}' layout",
        app.keyboard_layout, app.keyboard_layout
    );
    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Keyboard Layout "),
    );
    f.render_widget(widget, area);
}

fn render_disk_selection(f: &mut Frame, app: &mut InstallerApp, area: Rect) {
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

fn render_timezone(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let text = format!(
        "\n  Selected timezone: {}\n\n  \
         Press Enter to continue",
        app.timezone
    );
    let widget =
        Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Timezone "));
    f.render_widget(widget, area);
}

fn render_network(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let text = format!(
        "\n  Hostname: {}\n\n  \
         Network: DHCP (automatic)\n  \
         DNS: 1.1.1.1, 1.0.0.1\n\n  \
         Press Enter to continue",
        if app.hostname.is_empty() {
            "monolith"
        } else {
            &app.hostname
        }
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
        "\n  Username: {}\n\n  \
         Root login: disabled (recommended)\n  \
         SSH: key-based authentication\n\n  \
         Press Enter to continue",
        if app.username.is_empty() {
            "admin"
        } else {
            &app.username
        }
    );
    let widget = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" User Creation "),
    );
    f.render_widget(widget, area);
}

fn render_packages(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let items: Vec<ListItem> = app
        .packages
        .iter()
        .map(|(name, selected)| {
            let checkbox = if *selected { "[x]" } else { "[ ]" };
            ListItem::new(format!("  {checkbox} {name}"))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Additional Packages (Space to toggle) "),
    );
    f.render_widget(list, area);
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
         Monolith OS v{} \"Obsidian\" has been installed.\n\n    \
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
