use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Clear, Gauge, List, ListItem, ListState, Padding, Paragraph,
        Wrap,
    },
};

/// Brand palette for the installer — same emerald + cyan aurora used by
/// mnweb and mntui so the install experience already feels like Monolith.
mod palette {
    use ratatui::style::Color;
    pub const ACCENT: Color = Color::Rgb(53, 224, 161);
    pub const ACCENT_2: Color = Color::Rgb(92, 201, 255);
    pub const TEXT: Color = Color::Rgb(232, 236, 241);
    pub const MUTE: Color = Color::Rgb(176, 182, 192);
    pub const DIM: Color = Color::Rgb(125, 132, 143);
    pub const PANEL: Color = Color::Rgb(30, 35, 45);
    pub const BG: Color = Color::Rgb(7, 9, 13);
}

/// ASCII brand mark for the installer welcome / completion screens.
///
/// Uses solid block + half-block characters only — these render reliably on
/// the Linux console TTY where the installer actually runs, and on any
/// terminal emulator. No box-drawing characters that need pixel-perfect
/// alignment.
const MONOLITH_LOGO: &str = r#"
    █▀▄▀█  █▀█  █▄░█  █▀█  █░░  █  ▀█▀  █░█
    █░▀░█  █▄█  █░▀█  █▄█  █▄▄  █  ░█░  █▀█
"#;

/// Build a panel block — rounded border, dim title, soft padding.
fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::PANEL))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                title,
                Style::default()
                    .fg(palette::MUTE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]))
        .padding(Padding::new(2, 2, 1, 1))
}

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
            // Complete reports one past the last visible step so the
            // step-progress gauge can reach 100%. The brand bar clamps to
            // `total` so the user-visible label still reads "10/10".
            Step::Complete => 11,
        }
    }
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

    loop {
        terminal.draw(|f| render_ui(f, &mut app))?;

        if event::poll(std::time::Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') if app.step != Step::Installing => {
                            app.should_quit = true;
                        }
                        KeyCode::Enter => app.next_step(),
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

        if app.step == Step::Installing && app.install_progress < 100 {
            app.install_progress += 1;
            if app.install_progress >= 100 {
                app.next_step();
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
    // Tinted backdrop matches mnweb/mntui — same product, three surfaces.
    let backdrop = Block::default().style(Style::default().bg(palette::BG));
    f.render_widget(backdrop, f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Header brand line
            Constraint::Length(3), // Step progress bar
            Constraint::Min(10),   // Content
            Constraint::Length(2), // Footer
        ])
        .split(f.area());

    render_brand_bar(f, app, chunks[0]);
    render_step_progress(f, app, chunks[1]);

    f.render_widget(Clear, chunks[2]);
    match app.step {
        Step::Welcome => render_welcome(f, chunks[2]),
        Step::Keyboard => render_keyboard(f, app, chunks[2]),
        Step::DiskSelection => render_disk_selection(f, app, chunks[2]),
        Step::Encryption => render_encryption(f, app, chunks[2]),
        Step::Timezone => render_timezone(f, app, chunks[2]),
        Step::Network => render_network(f, app, chunks[2]),
        Step::UserCreation => render_user(f, app, chunks[2]),
        Step::Packages => render_packages(f, app, chunks[2]),
        Step::Review => render_review(f, app, chunks[2]),
        Step::Installing => render_installing(f, app, chunks[2]),
        Step::Complete => render_complete(f, chunks[2]),
    }

    render_footer(f, app, chunks[3]);
}

fn step_label(step: &Step) -> &'static str {
    match step {
        Step::Welcome => "Welcome",
        Step::Keyboard => "Keyboard",
        Step::DiskSelection => "Disk",
        Step::Encryption => "Encryption",
        Step::Timezone => "Timezone",
        Step::Network => "Network",
        Step::UserCreation => "User",
        Step::Packages => "Packages",
        Step::Review => "Review",
        Step::Installing => "Install",
        Step::Complete => "Done",
    }
}

fn render_brand_bar(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(40)])
        .split(area);

    let brand = Paragraph::new(Line::from(vec![
        Span::styled(
            " ▮ ",
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "MONOLITH",
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("installer", Style::default().fg(palette::DIM)),
        Span::raw("  "),
        Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            Style::default().fg(palette::ACCENT_2),
        ),
    ]));
    f.render_widget(brand, columns[0]);

    let total = 10u8;
    let n = app.step_number().min(total);
    let stage = Paragraph::new(Line::from(vec![
        Span::styled(
            format!("{} ", step_label(&app.step)),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("· step {n}/{total} "),
            Style::default().fg(palette::DIM),
        ),
    ]))
    .alignment(Alignment::Right);
    f.render_widget(stage, columns[1]);
}

fn render_step_progress(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let total = 10u16;
    let n = app.step_number() as u16;
    // Steps complete when you LEAVE them, so progress is (n - 1) / total.
    // Complete returns 11 → bar reaches 100 % on the success screen.
    let pct: u16 = (n.saturating_sub(1) * 100 / total).min(100);
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(palette::PANEL))
                .padding(Padding::new(1, 1, 0, 0)),
        )
        .gauge_style(
            Style::default()
                .fg(palette::ACCENT)
                .bg(Color::Rgb(20, 24, 32)),
        )
        .percent(pct)
        .label(format!("{} of {}", n.min(total), total));
    f.render_widget(gauge, area);
}

fn render_footer(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let spans: Vec<Span> = if app.step == Step::Complete {
        vec![
            Span::styled(
                " ⏎ ",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("reboot   ", Style::default().fg(palette::DIM)),
            Span::styled(
                "q ",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("quit", Style::default().fg(palette::DIM)),
        ]
    } else if app.step == Step::Installing {
        vec![
            Span::styled(" ● ", Style::default().fg(palette::ACCENT)),
            Span::styled(
                "installation in progress…",
                Style::default().fg(palette::MUTE),
            ),
        ]
    } else {
        vec![
            Span::styled(
                " ⏎ ",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("continue   ", Style::default().fg(palette::DIM)),
            Span::styled(
                "⌫/esc ",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("back   ", Style::default().fg(palette::DIM)),
            Span::styled(
                "q ",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("quit", Style::default().fg(palette::DIM)),
        ]
    };
    let footer = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(palette::PANEL)),
    );
    f.render_widget(footer, area);
}

fn render_welcome(f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            MONOLITH_LOGO,
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("    "),
            Span::styled(
                format!("v{} ", env!("CARGO_PKG_VERSION")),
                Style::default().fg(palette::ACCENT_2),
            ),
            Span::styled(
                "Obsidian",
                Style::default()
                    .fg(palette::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — Built for the ones who mean it.",
                Style::default().fg(palette::DIM),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "    System requirements",
            Style::default().fg(palette::MUTE),
        )),
        Line::from(Span::styled(
            "      · CPU      x86_64 or ARM64",
            Style::default().fg(palette::DIM),
        )),
        Line::from(Span::styled(
            "      · RAM      2 GB minimum (8 GB recommended)",
            Style::default().fg(palette::DIM),
        )),
        Line::from(Span::styled(
            "      · Disk     20 GB minimum (100 GB recommended)",
            Style::default().fg(palette::DIM),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "    Press Enter to begin.",
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Welcome"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_keyboard(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let lines = vec![
        kv_line("Selected layout", &app.keyboard_layout),
        Line::from(""),
        Line::from(Span::styled(
            "Common layouts: us · uk · de · fr · es · ru · jp",
            Style::default().fg(palette::DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Press Enter",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" to continue with '{}'.", app.keyboard_layout),
                Style::default().fg(palette::MUTE),
            ),
        ]),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Keyboard layout"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn kv_line<'a>(key: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{key:<16}"), Style::default().fg(palette::DIM)),
        Span::styled(
            value.to_string(),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn render_disk_selection(f: &mut Frame, app: &mut InstallerApp, area: Rect) {
    let items: Vec<ListItem> = if app.disk_list.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No block devices detected. Boot from removable media to attach disks.",
            Style::default().fg(palette::DIM),
        )))]
    } else {
        app.disk_list
            .iter()
            .map(|d| {
                ListItem::new(Line::from(Span::styled(
                    d.clone(),
                    Style::default().fg(palette::TEXT),
                )))
            })
            .collect()
    };

    if app.disk_list_state.selected().is_none() && !app.disk_list.is_empty() {
        app.disk_list_state.select(Some(0));
    }

    let list = List::new(items)
        .block(panel("Select installation disk"))
        .highlight_style(
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut app.disk_list_state);
}

fn render_encryption(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let (mark_color, mark) = if app.use_encryption {
        (palette::ACCENT, "[x]")
    } else {
        (palette::DIM, "[ ]")
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{mark} "),
                Style::default().fg(mark_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Enable LUKS2 full-disk encryption",
                Style::default()
                    .fg(palette::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Encryption adds tamper-resistance but requires entering a password at every boot.",
            Style::default().fg(palette::DIM),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Space",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" toggle   ", Style::default().fg(palette::DIM)),
            Span::styled(
                "Enter",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" continue", Style::default().fg(palette::DIM)),
        ]),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Encryption"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_timezone(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let lines = vec![
        kv_line("Selected", &app.timezone),
        Line::from(""),
        Line::from(Span::styled(
            "You can change this later with `mnctl config set timezone <Region/City>`.",
            Style::default().fg(palette::DIM),
        )),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Timezone"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_network(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let host = if app.hostname.is_empty() {
        "monolith"
    } else {
        &app.hostname
    };
    let lines = vec![
        kv_line("Hostname", host),
        kv_line("Network", "DHCP (automatic)"),
        kv_line("DNS", "1.1.1.1 · 1.0.0.1"),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Network configuration"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_user(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let user = if app.username.is_empty() {
        "admin"
    } else {
        &app.username
    };
    let lines = vec![
        kv_line("Username", user),
        kv_line("Root login", "disabled (recommended)"),
        kv_line("SSH", "key-based authentication, port 2222"),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("User creation"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_packages(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let items: Vec<ListItem> = app
        .packages
        .iter()
        .map(|(name, selected)| {
            let (color, mark) = if *selected {
                (palette::ACCENT, "[x]")
            } else {
                (palette::DIM, "[ ]")
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{mark} "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(name.clone(), Style::default().fg(palette::TEXT)),
            ]))
        })
        .collect();

    let list = List::new(items).block(panel("Additional packages — Space to toggle"));
    f.render_widget(list, area);
}

fn render_review(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let selected_pkgs: Vec<&str> = app
        .packages
        .iter()
        .filter(|(_, s)| *s)
        .map(|(n, _)| n.as_str())
        .collect();

    let pkgs = if selected_pkgs.is_empty() {
        "none".to_string()
    } else {
        selected_pkgs.join(" · ")
    };

    let lines = vec![
        kv_line("Keyboard", &app.keyboard_layout),
        kv_line(
            "Disk",
            if app.disk.is_empty() {
                "auto"
            } else {
                &app.disk
            },
        ),
        kv_line(
            "Encryption",
            if app.use_encryption { "LUKS2" } else { "none" },
        ),
        kv_line("Timezone", &app.timezone),
        kv_line(
            "Hostname",
            if app.hostname.is_empty() {
                "monolith"
            } else {
                &app.hostname
            },
        ),
        kv_line(
            "Username",
            if app.username.is_empty() {
                "admin"
            } else {
                &app.username
            },
        ),
        kv_line("Packages", &pkgs),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Press Enter",
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " to begin installation. Esc to go back.",
                Style::default().fg(palette::DIM),
            ),
        ]),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Review & install"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_installing(f: &mut Frame, app: &InstallerApp, area: Rect) {
    let steps = [
        "Partitioning disk",
        "Formatting partitions",
        "Installing base system",
        "Installing Monolith packages",
        "Installing kernel",
        "Configuring system",
        "Setting up bootloader",
        "Applying security hardening",
        "Configuring monitoring",
        "Finalizing",
    ];

    // Length(5) leaves 1 row of inner content height after the rounded
    // border (2 rows) and panel()'s vertical padding (2 rows). With Length(3)
    // ratatui clamped the gauge's inner area to zero and the bar disappeared.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)])
        .split(area);

    let pct: u16 = app.install_progress.min(100);
    let progress = Gauge::default()
        .block(panel("Installing Monolith OS"))
        .gauge_style(
            Style::default()
                .fg(palette::ACCENT)
                .bg(Color::Rgb(20, 24, 32)),
        )
        .percent(pct)
        .label(format!("{pct}%"));
    f.render_widget(progress, chunks[0]);

    let current_step = (app.install_progress as usize / 10).min(steps.len() - 1);
    let mut lines: Vec<Line> = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let (icon, icon_color, text_color) = if i < current_step {
            ("✔", palette::ACCENT, palette::MUTE)
        } else if i == current_step {
            ("●", palette::ACCENT_2, palette::TEXT)
        } else {
            ("○", palette::DIM, palette::DIM)
        };
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("{icon} "),
                Style::default().fg(icon_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(step.to_string(), Style::default().fg(text_color)),
        ]));
    }
    let stages = Paragraph::new(lines)
        .block(panel("Stages"))
        .wrap(Wrap { trim: false });
    f.render_widget(stages, chunks[1]);
}

fn render_complete(f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = vec![
        Line::from(Span::styled(
            MONOLITH_LOGO,
            Style::default()
                .fg(palette::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("    "),
            Span::styled(
                "Installation complete.",
                Style::default()
                    .fg(palette::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled(
                format!("Monolith OS v{} · Obsidian", env!("CARGO_PKG_VERSION")),
                Style::default().fg(palette::ACCENT_2),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "    Remove installation media and press Enter to reboot.",
            Style::default().fg(palette::MUTE),
        )),
        Line::from(Span::styled(
            "    Then connect via SSH on port 2222:",
            Style::default().fg(palette::DIM),
        )),
        Line::from(Span::styled(
            "      ssh admin@<server-ip> -p 2222",
            Style::default().fg(palette::ACCENT),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "    First steps",
            Style::default().fg(palette::MUTE),
        )),
        Line::from(Span::styled(
            "      mnctl info system        — system overview",
            Style::default().fg(palette::DIM),
        )),
        Line::from(Span::styled(
            "      mnctl monitor status     — live resource usage",
            Style::default().fg(palette::DIM),
        )),
        Line::from(Span::styled(
            "      mnctl security audit     — security check",
            Style::default().fg(palette::DIM),
        )),
        Line::from(Span::styled(
            "      mnctl template list      — application templates",
            Style::default().fg(palette::DIM),
        )),
    ];
    let widget = Paragraph::new(lines)
        .block(panel("Installation complete"))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
