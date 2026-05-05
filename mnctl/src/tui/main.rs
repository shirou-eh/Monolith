use std::io;
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline, Tabs, Wrap},
};
use sysinfo::{Networks, System};

struct ContainerInfo {
    name: String,
    image: String,
    status: String,
}

struct NetIfaceInfo {
    name: String,
    rx_bytes: u64,
    tx_bytes: u64,
}

struct App {
    sys: System,
    networks: Networks,
    cpu_history: Vec<u64>,
    active_tab: usize,
    should_quit: bool,
    tick_count: u64,
    containers: Vec<ContainerInfo>,
    net_ifaces: Vec<NetIfaceInfo>,
    log_lines: Vec<String>,
    alerts: Vec<String>,
}

fn detect_container_runtime() -> Option<String> {
    for bin in &["docker", "podman"] {
        if which::which(bin).is_ok() {
            return Some(bin.to_string());
        }
    }
    None
}

fn fetch_containers() -> Vec<ContainerInfo> {
    let runtime = match detect_container_runtime() {
        Some(r) => r,
        None => return Vec::new(),
    };
    let output = Command::new(&runtime)
        .args([
            "ps",
            "-a",
            "--format",
            "{{.Names}}\t{{.Image}}\t{{.Status}}",
        ])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '\t').collect();
                if parts.len() == 3 {
                    Some(ContainerInfo {
                        name: parts[0].to_string(),
                        image: parts[1].to_string(),
                        status: parts[2].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn fetch_logs() -> Vec<String> {
    let output = Command::new("journalctl")
        .args(["--no-pager", "-n", "50", "--output=short"])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect(),
        _ => vec!["journalctl not available".to_string()],
    }
}

fn fetch_alerts() -> Vec<String> {
    let mut alerts = Vec::new();

    // Check high load
    let load = System::load_average();
    let cores = num_cpus();
    if load.one > cores as f64 * 0.9 {
        alerts.push(format!(
            "HIGH LOAD: 1m avg {:.2} (cores: {})",
            load.one, cores
        ));
    }

    // Check disk space
    for disk in sysinfo::Disks::new_with_refreshed_list().list() {
        let total = disk.total_space();
        let avail = disk.available_space();
        if total > 0 {
            let used_pct = ((total - avail) as f64 / total as f64 * 100.0) as u64;
            if used_pct > 90 {
                alerts.push(format!(
                    "DISK: {} is {}% full",
                    disk.mount_point().to_string_lossy(),
                    used_pct
                ));
            }
        }
    }

    // Check failed systemd units
    if let Ok(o) = Command::new("systemctl")
        .args(["--failed", "--no-pager", "--plain", "--no-legend"])
        .output()
    {
        if o.status.success() {
            let out = String::from_utf8_lossy(&o.stdout);
            for line in out.lines() {
                let unit = line.split_whitespace().next().unwrap_or("unknown");
                if !unit.is_empty() {
                    alerts.push(format!("FAILED UNIT: {unit}"));
                }
            }
        }
    }

    if alerts.is_empty() {
        alerts.push("No active alerts".to_string());
    }
    alerts
}

fn num_cpus() -> usize {
    let mut s = System::new();
    s.refresh_cpu_all();
    s.cpus().len().max(1)
}

impl App {
    fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let networks = Networks::new_with_refreshed_list();
        let containers = fetch_containers();
        let net_ifaces = Vec::new();
        let log_lines = fetch_logs();
        let alerts = fetch_alerts();
        Self {
            sys,
            networks,
            cpu_history: vec![0; 60],
            active_tab: 0,
            should_quit: false,
            tick_count: 0,
            containers,
            net_ifaces,
            log_lines,
            alerts,
        }
    }

    fn on_tick(&mut self) {
        self.sys.refresh_all();
        self.networks.refresh();
        self.tick_count += 1;

        let cpu_usage = self.sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
            / self.sys.cpus().len() as f32;

        self.cpu_history.push(cpu_usage as u64);
        if self.cpu_history.len() > 60 {
            self.cpu_history.remove(0);
        }

        // Refresh network interfaces every tick
        self.net_ifaces = self
            .networks
            .iter()
            .map(|(name, data)| NetIfaceInfo {
                name: name.to_string(),
                rx_bytes: data.total_received(),
                tx_bytes: data.total_transmitted(),
            })
            .collect();

        // Refresh containers, logs, and alerts every 10 ticks
        if self.tick_count.is_multiple_of(10) {
            self.containers = fetch_containers();
            self.log_lines = fetch_logs();
            self.alerts = fetch_alerts();
        }
    }

    fn on_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.active_tab = (self.active_tab + 1) % 5,
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

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(3), // Tabs
            Constraint::Min(10),   // Main content
            Constraint::Length(3), // Footer
        ])
        .split(f.area());

    // Header
    let hostname = System::host_name().unwrap_or_else(|| "monolith".to_string());
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());
    let uptime = System::uptime();
    let days = uptime / 86400;
    let hours = (uptime % 86400) / 3600;
    let mins = (uptime % 3600) / 60;

    let header_text = format!(
        " MONOLITH  |  {}  |  Kernel {}  |  Up {}d {}h {}m  |  v{}",
        hostname,
        kernel,
        days,
        hours,
        mins,
        env!("CARGO_PKG_VERSION")
    );
    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Green).bg(Color::Black))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    // Tabs
    let tab_titles = vec!["System", "Containers", "Network", "Logs", "Alerts"];
    let tabs = Tabs::new(tab_titles)
        .select(app.active_tab)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" | ");
    f.render_widget(tabs, chunks[1]);

    // Main content
    match app.active_tab {
        0 => render_system_tab(f, app, chunks[2]),
        1 => render_containers_tab(f, app, chunks[2]),
        2 => render_network_tab(f, app, chunks[2]),
        3 => render_logs_tab(f, app, chunks[2]),
        4 => render_alerts_tab(f, app, chunks[2]),
        _ => {}
    }

    // Footer
    let footer = Paragraph::new(
        " q:Quit  Tab:Next  s:System  c:Containers  n:Network  l:Logs  a:Alerts  ?:Help",
    )
    .style(Style::default().fg(Color::DarkGray))
    .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[3]);
}

fn render_system_tab(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ])
        .split(area);

    // Left panel — CPU & Memory
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // CPU sparkline
            Constraint::Length(3), // RAM gauge
            Constraint::Length(3), // Swap gauge
            Constraint::Min(3),    // Load average
        ])
        .split(chunks[0]);

    let cpu_sparkline = Sparkline::default()
        .block(Block::default().title(" CPU (60s) ").borders(Borders::ALL))
        .data(&app.cpu_history)
        .max(100)
        .style(Style::default().fg(Color::Green));
    f.render_widget(cpu_sparkline, left_chunks[0]);

    let total_mem = app.sys.total_memory();
    let used_mem = app.sys.used_memory();
    let mem_pct = if total_mem > 0 {
        (used_mem as f64 / total_mem as f64 * 100.0) as u16
    } else {
        0
    };
    let mem_gauge = Gauge::default()
        .block(Block::default().title(" RAM ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(if mem_pct > 90 {
            Color::Red
        } else if mem_pct > 75 {
            Color::Yellow
        } else {
            Color::Green
        }))
        .percent(mem_pct)
        .label(format!(
            "{} / {} MB ({mem_pct}%)",
            used_mem / 1024 / 1024,
            total_mem / 1024 / 1024
        ));
    f.render_widget(mem_gauge, left_chunks[1]);

    let total_swap = app.sys.total_swap();
    let used_swap = app.sys.used_swap();
    let swap_pct = if total_swap > 0 {
        (used_swap as f64 / total_swap as f64 * 100.0) as u16
    } else {
        0
    };
    let swap_gauge = Gauge::default()
        .block(Block::default().title(" Swap ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(swap_pct)
        .label(format!(
            "{} / {} MB",
            used_swap / 1024 / 1024,
            total_swap / 1024 / 1024
        ));
    f.render_widget(swap_gauge, left_chunks[2]);

    let load = System::load_average();
    let load_text = format!(
        " Load Average\n  1m: {:.2}  5m: {:.2}  15m: {:.2}\n  Cores: {}",
        load.one,
        load.five,
        load.fifteen,
        app.sys.cpus().len()
    );
    let load_widget =
        Paragraph::new(load_text).block(Block::default().title(" Load ").borders(Borders::ALL));
    f.render_widget(load_widget, left_chunks[3]);

    // Center panel — Disks & Processes
    let center_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let mut disk_items = Vec::new();
    for disk in sysinfo::Disks::new_with_refreshed_list().list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        let total = disk.total_space();
        let avail = disk.available_space();
        let pct = if total > 0 {
            ((total - avail) as f64 / total as f64 * 100.0) as u64
        } else {
            0
        };
        disk_items.push(ListItem::new(format!(
            " {mount:<20} {pct:>3}% ({} / {} GB)",
            (total - avail) / 1024 / 1024 / 1024,
            total / 1024 / 1024 / 1024
        )));
    }
    let disk_list =
        List::new(disk_items).block(Block::default().title(" Disks ").borders(Borders::ALL));
    f.render_widget(disk_list, center_chunks[0]);

    let mut proc_items: Vec<(&sysinfo::Pid, &sysinfo::Process)> =
        app.sys.processes().iter().collect();
    proc_items.sort_by(|a, b| {
        b.1.cpu_usage()
            .partial_cmp(&a.1.cpu_usage())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_procs: Vec<ListItem> = proc_items
        .iter()
        .take(10)
        .map(|(pid, proc_info)| {
            ListItem::new(format!(
                " {:>7} {:>5.1}% {:>8} MB  {}",
                pid.as_u32(),
                proc_info.cpu_usage(),
                proc_info.memory() / 1024 / 1024,
                proc_info.name().to_string_lossy(),
            ))
        })
        .collect();
    let proc_list = List::new(top_procs).block(
        Block::default()
            .title(" Top Processes ")
            .borders(Borders::ALL),
    );
    f.render_widget(proc_list, center_chunks[1]);

    // Right panel — Status summary
    let status_text = " Services: checking...\n\n Alerts: none\n\n Last backup: N/A";
    let status_widget =
        Paragraph::new(status_text).block(Block::default().title(" Status ").borders(Borders::ALL));
    f.render_widget(status_widget, chunks[2]);
}

fn render_containers_tab(f: &mut Frame, app: &App, area: Rect) {
    if app.containers.is_empty() {
        let runtime = detect_container_runtime().unwrap_or_else(|| "docker/podman".to_string());
        let msg = format!(
            " No containers found (runtime: {runtime})\n\n \
             Start a container with: mnctl container start <name>"
        );
        let widget =
            Paragraph::new(msg).block(Block::default().title(" Containers ").borders(Borders::ALL));
        f.render_widget(widget, area);
        return;
    }

    let header = ListItem::new(format!(" {:<25} {:<30} {}", "NAME", "IMAGE", "STATUS"))
        .style(Style::default().add_modifier(Modifier::BOLD));

    let mut items = vec![header];
    for c in &app.containers {
        let style = if c.status.contains("Up") {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        items.push(
            ListItem::new(format!(" {:<25} {:<30} {}", c.name, c.image, c.status)).style(style),
        );
    }

    let list = List::new(items).block(
        Block::default()
            .title(format!(" Containers ({}) ", app.containers.len()))
            .borders(Borders::ALL),
    );
    f.render_widget(list, area);
}

fn render_network_tab(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(6)])
        .split(area);

    // Network interfaces
    let header = ListItem::new(format!(
        " {:<18} {:>14} {:>14}",
        "INTERFACE", "RX (total)", "TX (total)"
    ))
    .style(Style::default().add_modifier(Modifier::BOLD));

    let mut items = vec![header];
    for iface in &app.net_ifaces {
        items.push(ListItem::new(format!(
            " {:<18} {:>10} MB {:>10} MB",
            iface.name,
            iface.rx_bytes / 1024 / 1024,
            iface.tx_bytes / 1024 / 1024,
        )));
    }

    if app.net_ifaces.is_empty() {
        items.push(ListItem::new(" No network interfaces detected"));
    }

    let iface_list = List::new(items).block(
        Block::default()
            .title(" Network Interfaces ")
            .borders(Borders::ALL),
    );
    f.render_widget(iface_list, chunks[0]);

    // Listening ports (quick ss check)
    let ports_text = match Command::new("ss").args(["-tlnp"]).output() {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = out.lines().take(5).collect();
            lines.join("\n")
        }
        _ => " ss not available".to_string(),
    };
    let ports_widget = Paragraph::new(ports_text)
        .block(
            Block::default()
                .title(" Listening Ports (top 5) ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(ports_widget, chunks[1]);
}

fn render_logs_tab(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .log_lines
        .iter()
        .map(|line| ListItem::new(format!(" {line}")))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" System Logs (journalctl, last 50) ")
            .borders(Borders::ALL),
    );
    f.render_widget(list, area);
}

fn render_alerts_tab(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .alerts
        .iter()
        .map(|alert| {
            let style = if alert.starts_with("No active") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            };
            ListItem::new(format!(" {alert}")).style(style)
        })
        .collect();

    let has_issues = app.alerts.iter().any(|a| !a.starts_with("No active"));
    let title = if has_issues {
        format!(" Alerts ({}) ", app.alerts.len())
    } else {
        " Alerts ".to_string()
    };

    let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(list, area);
}
