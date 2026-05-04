use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{
        Block, BorderType, Borders, Gauge, List, ListItem, Padding, Paragraph, Sparkline, Tabs,
        Wrap,
    },
};
use sysinfo::System;

/// Brand palette for mntui — kept in sync with mnweb's emerald/teal aurora
/// look so the CLI and the web UI feel like the same product.
mod palette {
    use ratatui::style::Color;
    pub const ACCENT: Color = Color::Rgb(53, 224, 161); // emerald
    pub const ACCENT_2: Color = Color::Rgb(92, 201, 255); // cyan
    pub const WARN: Color = Color::Rgb(245, 196, 81);
    pub const BAD: Color = Color::Rgb(255, 107, 107);
    pub const TEXT: Color = Color::Rgb(232, 236, 241);
    pub const MUTE: Color = Color::Rgb(176, 182, 192);
    pub const DIM: Color = Color::Rgb(125, 132, 143);
    pub const PANEL: Color = Color::Rgb(30, 35, 45);
}

const TAB_TITLES: [(&str, &str); 5] = [
    ("System", "s"),
    ("Containers", "c"),
    ("Network", "n"),
    ("Logs", "l"),
    ("Alerts", "a"),
];

struct App {
    sys: System,
    cpu_history: Vec<u64>,
    active_tab: usize,
    should_quit: bool,
    tick_count: u64,
}

impl App {
    fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys,
            cpu_history: vec![0; 60],
            active_tab: 0,
            should_quit: false,
            tick_count: 0,
        }
    }

    fn on_tick(&mut self) {
        self.sys.refresh_all();
        self.tick_count += 1;

        let cpu_usage = self.sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
            / self.sys.cpus().len().max(1) as f32;

        self.cpu_history.push(cpu_usage as u64);
        if self.cpu_history.len() > 60 {
            self.cpu_history.remove(0);
        }
    }

    fn on_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab | KeyCode::Right => {
                self.active_tab = (self.active_tab + 1) % TAB_TITLES.len()
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.active_tab = (self.active_tab + TAB_TITLES.len() - 1) % TAB_TITLES.len()
            }
            KeyCode::Char('s') => self.active_tab = 0,
            KeyCode::Char('c') => self.active_tab = 1,
            KeyCode::Char('n') => self.active_tab = 2,
            KeyCode::Char('l') => self.active_tab = 3,
            KeyCode::Char('a') => self.active_tab = 4,
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let tick_rate = Duration::from_secs(1);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui(f, &app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.on_key(key.code);
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
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
        .padding(Padding::new(1, 1, 0, 0))
}

fn ui(f: &mut Frame, app: &App) {
    // Outer chrome: a tinted backdrop so the overall TUI matches the web UI.
    let backdrop = Block::default().style(Style::default().bg(Color::Rgb(7, 9, 13)));
    f.render_widget(backdrop, f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Tabs
            Constraint::Min(10),   // Main content
            Constraint::Length(2), // Footer
        ])
        .split(f.area());

    render_header(f, chunks[0]);
    render_tabs(f, app, chunks[1]);

    match app.active_tab {
        0 => render_system_tab(f, app, chunks[2]),
        1 => render_containers_tab(f, chunks[2]),
        2 => render_network_tab(f, chunks[2]),
        3 => render_logs_tab(f, chunks[2]),
        4 => render_alerts_tab(f, chunks[2]),
        _ => {}
    }

    render_footer(f, chunks[3]);
}

fn render_header(f: &mut Frame, area: Rect) {
    let hostname = System::host_name().unwrap_or_else(|| "monolith".to_string());
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());
    let uptime = System::uptime();
    let days = uptime / 86400;
    let hours = (uptime % 86400) / 3600;
    let mins = (uptime % 3600) / 60;
    let version = env!("CARGO_PKG_VERSION");

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(48)])
        .split(area);

    // Brand on the left
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
        Span::styled(
            format!("v{version}"),
            Style::default().fg(palette::ACCENT_2),
        ),
        Span::styled("  ·  Obsidian", Style::default().fg(palette::DIM)),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(palette::PANEL)),
    );
    f.render_widget(brand, columns[0]);

    // System summary on the right
    let info = Paragraph::new(Line::from(vec![
        Span::styled("host ", Style::default().fg(palette::DIM)),
        Span::styled(hostname, Style::default().fg(palette::TEXT)),
        Span::raw("  "),
        Span::styled("kernel ", Style::default().fg(palette::DIM)),
        Span::styled(kernel, Style::default().fg(palette::MUTE)),
        Span::raw("  "),
        Span::styled("up ", Style::default().fg(palette::DIM)),
        Span::styled(
            format!("{days}d {hours}h {mins}m "),
            Style::default().fg(palette::ACCENT),
        ),
    ]))
    .alignment(Alignment::Right)
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(palette::PANEL)),
    );
    f.render_widget(info, columns[1]);
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = TAB_TITLES
        .iter()
        .enumerate()
        .map(|(i, (label, key))| {
            let is_active = i == app.active_tab;
            let label_style = if is_active {
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette::MUTE)
            };
            let key_style = if is_active {
                Style::default().fg(palette::ACCENT_2)
            } else {
                Style::default().fg(palette::DIM)
            };
            Line::from(vec![
                Span::styled(format!(" {label} "), label_style),
                Span::styled(format!("[{key}] "), key_style),
            ])
        })
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.active_tab)
        .divider(Span::styled("·", Style::default().fg(palette::DIM)))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(palette::PANEL))
                .padding(Padding::new(1, 1, 0, 0)),
        );
    f.render_widget(tabs, area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" q ", Style::default().fg(palette::ACCENT)),
        Span::styled("quit  ", Style::default().fg(palette::DIM)),
        Span::styled("⇥ ", Style::default().fg(palette::ACCENT)),
        Span::styled("next  ", Style::default().fg(palette::DIM)),
        Span::styled("← → ", Style::default().fg(palette::ACCENT)),
        Span::styled("switch  ", Style::default().fg(palette::DIM)),
        Span::styled("s c n l a ", Style::default().fg(palette::ACCENT)),
        Span::styled("jump to tab", Style::default().fg(palette::DIM)),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(palette::PANEL)),
    );
    f.render_widget(footer, area);
}

fn meter_color(pct: u16) -> Color {
    if pct > 90 {
        palette::BAD
    } else if pct > 75 {
        palette::WARN
    } else {
        palette::ACCENT
    }
}

fn render_system_tab(f: &mut Frame, app: &App, area: Rect) {
    let outer = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(38),
            Constraint::Percentage(22),
        ])
        .split(area);

    // ---- Left column: CPU sparkline + RAM/Swap gauges + Load average ----
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7), // CPU sparkline
            Constraint::Length(3), // RAM gauge
            Constraint::Length(3), // Swap gauge
            Constraint::Min(3),    // Load average
        ])
        .split(outer[0]);

    let cpu_now = app.cpu_history.last().copied().unwrap_or(0);
    let cpu_color = meter_color(cpu_now as u16);
    let cpu_title = format!("CPU · {cpu_now:>3}%");
    let cpu_sparkline = Sparkline::default()
        .block(panel(&cpu_title))
        .data(&app.cpu_history)
        .max(100)
        .style(Style::default().fg(cpu_color));
    f.render_widget(cpu_sparkline, left[0]);

    let total_mem = app.sys.total_memory();
    let used_mem = app.sys.used_memory();
    let mem_pct = if total_mem > 0 {
        (used_mem as f64 / total_mem as f64 * 100.0) as u16
    } else {
        0
    };
    let mem_gauge = Gauge::default()
        .block(panel("RAM"))
        .gauge_style(
            Style::default()
                .fg(meter_color(mem_pct))
                .bg(Color::Rgb(20, 24, 32)),
        )
        .percent(mem_pct)
        .label(format!(
            "{} / {} MiB · {mem_pct}%",
            used_mem / 1024 / 1024,
            total_mem / 1024 / 1024
        ));
    f.render_widget(mem_gauge, left[1]);

    let total_swap = app.sys.total_swap();
    let used_swap = app.sys.used_swap();
    let swap_pct = if total_swap > 0 {
        (used_swap as f64 / total_swap as f64 * 100.0) as u16
    } else {
        0
    };
    let swap_label = if total_swap > 0 {
        format!(
            "{} / {} MiB · {swap_pct}%",
            used_swap / 1024 / 1024,
            total_swap / 1024 / 1024
        )
    } else {
        "no swap configured".to_string()
    };
    let swap_gauge = Gauge::default()
        .block(panel("Swap"))
        .gauge_style(
            Style::default()
                .fg(palette::ACCENT_2)
                .bg(Color::Rgb(20, 24, 32)),
        )
        .percent(swap_pct)
        .label(swap_label);
    f.render_widget(swap_gauge, left[2]);

    let load = System::load_average();
    let load_widget = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("1m   ", Style::default().fg(palette::DIM)),
            Span::styled(
                format!("{:>6.2}", load.one),
                Style::default()
                    .fg(palette::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("5m   ", Style::default().fg(palette::DIM)),
            Span::styled(
                format!("{:>6.2}", load.five),
                Style::default().fg(palette::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("15m  ", Style::default().fg(palette::DIM)),
            Span::styled(
                format!("{:>6.2}", load.fifteen),
                Style::default().fg(palette::TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("cores ", Style::default().fg(palette::DIM)),
            Span::styled(
                format!("{}", app.sys.cpus().len()),
                Style::default().fg(palette::ACCENT),
            ),
        ]),
    ])
    .block(panel("Load average"));
    f.render_widget(load_widget, left[3]);

    // ---- Center column: Disks + Top processes ----
    let center = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(outer[1]);

    let mut disk_items: Vec<ListItem> = Vec::new();
    for disk in sysinfo::Disks::new_with_refreshed_list().list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        let total = disk.total_space();
        let avail = disk.available_space();
        let pct = if total > 0 {
            ((total - avail) as f64 / total as f64 * 100.0) as u64
        } else {
            0
        };
        let color = meter_color(pct as u16);
        disk_items.push(ListItem::new(Line::from(vec![
            Span::styled(
                format!(" {:<18} ", trim_to(&mount, 18)),
                Style::default().fg(palette::TEXT),
            ),
            Span::styled(
                format!("{pct:>3}%  "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{:>4} / {:>4} GiB",
                    (total - avail) / 1024 / 1024 / 1024,
                    total / 1024 / 1024 / 1024
                ),
                Style::default().fg(palette::DIM),
            ),
        ])));
    }
    let disk_list = List::new(disk_items).block(panel("Disks"));
    f.render_widget(disk_list, center[0]);

    let mut proc_items: Vec<(&sysinfo::Pid, &sysinfo::Process)> =
        app.sys.processes().iter().collect();
    proc_items.sort_by(|a, b| {
        b.1.cpu_usage()
            .partial_cmp(&a.1.cpu_usage())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut top_procs: Vec<ListItem> = Vec::with_capacity(11);
    top_procs.push(ListItem::new(Line::from(vec![Span::styled(
        format!(
            " {:>7}  {:>5}  {:>8}  {}",
            "PID", "CPU%", "MEM(MiB)", "NAME"
        ),
        Style::default()
            .fg(palette::DIM)
            .add_modifier(Modifier::BOLD),
    )])));
    top_procs.extend(proc_items.iter().take(10).map(|(pid, proc_info)| {
        let cpu = proc_info.cpu_usage();
        let cpu_color = if cpu > 80.0 {
            palette::BAD
        } else if cpu > 50.0 {
            palette::WARN
        } else {
            palette::ACCENT
        };
        ListItem::new(Line::from(vec![
            Span::styled(
                format!(" {:>7}  ", pid.as_u32()),
                Style::default().fg(palette::MUTE),
            ),
            Span::styled(format!("{cpu:>5.1}  "), Style::default().fg(cpu_color)),
            Span::styled(
                format!("{:>8}  ", proc_info.memory() / 1024 / 1024),
                Style::default().fg(palette::DIM),
            ),
            Span::styled(
                proc_info.name().to_string_lossy().to_string(),
                Style::default().fg(palette::TEXT),
            ),
        ]))
    }));
    let proc_list = List::new(top_procs).block(panel("Top processes"));
    f.render_widget(proc_list, center[1]);

    // ---- Right column: status summary ----
    let right_lines = vec![
        Line::from(vec![Span::styled(
            "Hostname",
            Style::default().fg(palette::DIM),
        )]),
        Line::from(vec![Span::styled(
            System::host_name().unwrap_or_else(|| "—".into()),
            Style::default()
                .fg(palette::TEXT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled("OS", Style::default().fg(palette::DIM))]),
        Line::from(vec![Span::styled(
            System::long_os_version().unwrap_or_else(|| "—".into()),
            Style::default().fg(palette::MUTE),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Tick",
            Style::default().fg(palette::DIM),
        )]),
        Line::from(vec![Span::styled(
            format!("#{}", app.tick_count),
            Style::default().fg(palette::ACCENT_2),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Health",
            Style::default().fg(palette::DIM),
        )]),
        Line::from(health_line(mem_pct, cpu_now as u16)),
    ];
    let status = Paragraph::new(right_lines)
        .block(panel("Status"))
        .wrap(Wrap { trim: true });
    f.render_widget(status, outer[2]);
}

fn health_line(mem_pct: u16, cpu_pct: u16) -> Vec<Span<'static>> {
    let (color, label) = if mem_pct > 90 || cpu_pct > 90 {
        (palette::BAD, "PRESSURE")
    } else if mem_pct > 75 || cpu_pct > 75 {
        (palette::WARN, "ELEVATED")
    } else {
        (palette::ACCENT, "NOMINAL")
    };
    vec![
        Span::styled("● ", Style::default().fg(color)),
        Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ]
}

fn trim_to(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn placeholder_panel<'a>(title: &'a str, body: &'a str, hint: &'a str) -> Paragraph<'a> {
    Paragraph::new(vec![
        Line::from(vec![Span::styled(body, Style::default().fg(palette::TEXT))]),
        Line::from(""),
        Line::from(vec![Span::styled(hint, Style::default().fg(palette::DIM))]),
    ])
    .block(panel(title))
    .wrap(Wrap { trim: true })
}

fn render_containers_tab(f: &mut Frame, area: Rect) {
    f.render_widget(
        placeholder_panel(
            "Containers",
            "Container view — Docker / Podman fan-out coming online.",
            "Tip: 's' returns to the system overview.",
        ),
        area,
    );
}

fn render_network_tab(f: &mut Frame, area: Rect) {
    f.render_widget(
        placeholder_panel(
            "Network",
            "Network interfaces, addresses, routes and active connections.",
            "Tip: 's' returns to the system overview.",
        ),
        area,
    );
}

fn render_logs_tab(f: &mut Frame, area: Rect) {
    f.render_widget(
        placeholder_panel(
            "Logs",
            "Streaming journalctl tail will land here.",
            "Tip: 's' returns to the system overview.",
        ),
        area,
    );
}

fn render_alerts_tab(f: &mut Frame, area: Rect) {
    let body = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("● ", Style::default().fg(palette::ACCENT)),
            Span::styled(
                "No active alerts.",
                Style::default()
                    .fg(palette::TEXT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Configure thresholds in /etc/monolith/monolith.toml under [monitor.alerts].",
            Style::default().fg(palette::DIM),
        )]),
    ])
    .block(panel("Alerts"))
    .wrap(Wrap { trim: true });
    f.render_widget(body, area);
}
