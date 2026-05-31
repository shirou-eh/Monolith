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

#[path = "../lang.rs"]
mod lang;

struct ContainerInfo {
    name: String,
    image: String,
    status: String,
}

#[derive(Clone)]
struct NetIfaceInfo {
    name: String,
    ip: String,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    rx_errors: u64,
    tx_errors: u64,
    is_up: bool,
}

#[derive(Clone)]
struct DiskIoInfo {
    name: String,
    read_bytes: u64,
    write_bytes: u64,
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
    net_ifaces_prev: Vec<NetIfaceInfo>,
    rx_history: Vec<Vec<u64>>,
    tx_history: Vec<Vec<u64>>,
    disk_io: Vec<DiskIoInfo>,
    disk_io_prev: Vec<DiskIoInfo>,
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
        let l = lang::detect();
        alerts.push(l.no_alerts.to_string());
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
        let net_ifaces_prev = net_ifaces.clone();
        let rx_history = vec![vec![0u64; 60]; net_ifaces.len().max(1)];
        let tx_history = vec![vec![0u64; 60]; net_ifaces.len().max(1)];
        let disk_io = collect_disk_io();
        let disk_io_prev = disk_io.clone();
        Self {
            sys,
            networks,
            cpu_history: vec![0; 60],
            active_tab: 0,
            should_quit: false,
            tick_count: 0,
            containers,
            net_ifaces,
            net_ifaces_prev,
            rx_history,
            tx_history,
            disk_io,
            disk_io_prev,
            log_lines,
            alerts,
        }
    }

    fn on_tick(&mut self) {
        self.sys.refresh_all();
        self.networks.refresh();
        self.tick_count += 1;

        // ARM-safe CPU usage: clamp to 0..100, avoid saturating cast
        let cpu_usage = self.sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
            / self.sys.cpus().len() as f32;
        let cpu_val = (cpu_usage as u64).min(100);
        self.cpu_history.push(cpu_val);
        if self.cpu_history.len() > 60 {
            self.cpu_history.remove(0);
        }

        // Stash previous tick before updating
        self.net_ifaces_prev = self.net_ifaces.clone();

        // Refresh network interfaces every tick
        let now = self
            .networks
            .iter()
            .map(|(name, data)| NetIfaceInfo {
                name: name.to_string(),
                ip: iface_ip(name),
                rx_bytes: data.total_received(),
                tx_bytes: data.total_transmitted(),
                rx_packets: data.total_packets_received(),
                tx_packets: data.total_packets_transmitted(),
                rx_errors: data.total_errors_on_received(),
                tx_errors: data.total_errors_on_transmitted(),
                is_up: std::fs::read_to_string(format!("/sys/class/net/{name}/operstate"))
                    .map(|s| s.trim() == "up")
                    .unwrap_or(false),
            })
            .collect::<Vec<_>>();

        // Merge: keep previously seen interfaces even if they disappeared
        let mut merged = now.clone();
        for old in &self.net_ifaces_prev {
            if !merged.iter().any(|n| n.name == old.name) {
                merged.push(NetIfaceInfo {
                    is_up: false,
                    ..old.clone()
                });
            }
        }
        self.net_ifaces = merged;

        // Update sparkline histories (index by position)
        self.rx_history.resize(self.net_ifaces.len(), vec![0u64; 60]);
        self.tx_history.resize(self.net_ifaces.len(), vec![0u64; 60]);
        for (i, iface) in self.net_ifaces.iter().enumerate() {
            if iface.is_up {
                self.rx_history[i].push(iface.rx_bytes / 1024); // KB
                self.tx_history[i].push(iface.tx_bytes / 1024);
            } else {
                self.rx_history[i].push(0);
                self.tx_history[i].push(0);
            }
            if self.rx_history[i].len() > 60 {
                self.rx_history[i].remove(0);
            }
            if self.tx_history[i].len() > 60 {
                self.tx_history[i].remove(0);
            }
        }

        // Track disk I/O every tick for rate calculation
        self.disk_io_prev = self.disk_io.clone();
        self.disk_io = collect_disk_io();

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
                KeyCode::Char('1') => self.active_tab = 0,
                KeyCode::Char('2') => self.active_tab = 1,
                KeyCode::Char('3') => self.active_tab = 2,
                KeyCode::Char('4') => self.active_tab = 3,
                KeyCode::Char('5') => self.active_tab = 4,
            _ => {}
        }
    }
}

/// Collect per-disk I/O counters from /proc/diskstats.
fn collect_disk_io() -> Vec<DiskIoInfo> {
    let data = std::fs::read_to_string("/proc/diskstats").unwrap_or_default();
    let mut out = Vec::new();
    for line in data.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 14 {
            continue;
        }
        let name = parts[2];
        // Skip partitions, loop, ram, nbd, zram
        if name.ends_with(|c: char| c.is_ascii_digit())
            || name.starts_with("loop")
            || name.starts_with("ram")
            || name.starts_with("nbd")
            || name.starts_with("zram")
        {
            continue;
        }
        let read_sectors: u64 = parts[5].parse().unwrap_or(0);
        let write_sectors: u64 = parts[9].parse().unwrap_or(0);
        out.push(DiskIoInfo {
            name: name.to_string(),
            read_bytes: read_sectors * 512,
            write_bytes: write_sectors * 512,
        });
    }
    out
}

/// Quick lookup for the first non-loopback IPv4 address of an interface.
fn iface_ip(name: &str) -> String {
    let ifaddrs = match nix::ifaddrs::getifaddrs() {
        Ok(ifs) => ifs,
        Err(_) => return "?".to_string(),
    };
    for iface in ifaddrs {
        if iface.interface_name != name {
            continue;
        }
        if let Some(addr) = iface.address {
            use nix::sys::socket::AddressFamily;
            use nix::sys::socket::SockaddrLike;
            if addr.family() == Some(AddressFamily::Inet) {
                let s = addr.to_string();
                if let Some(ip) = s.split(':').next() {
                    return ip.to_string();
                }
            }
        }
    }
    "?".to_string()
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
    let l = lang::detect();
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
        " MONOLITH  |  {}  |  Kernel {}  |  {} {}d {}h {}m  |  v{}",
        hostname,
        kernel,
        if l.header_up == "UP" { "Up" } else { "Работает" },
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
    let tab_titles = l.tabs.clone();
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
        0 => render_system_tab(f, app, chunks[2], &l),
        1 => render_containers_tab(f, app, chunks[2], &l),
        2 => render_network_tab(f, app, chunks[2], &l),
        3 => render_logs_tab(f, app, chunks[2], &l),
        4 => render_alerts_tab(f, app, chunks[2], &l),
        _ => {}
    }

    // Footer
    let footer = Paragraph::new(l.footer)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[3]);
}

fn render_system_tab(f: &mut Frame, app: &App, area: Rect, l: &lang::Lang) {
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
        .block(Block::default().title(l.cpu).borders(Borders::ALL))
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
        .block(Block::default().title(l.ram).borders(Borders::ALL))
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
        .block(Block::default().title(l.swap).borders(Borders::ALL))
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
        Paragraph::new(load_text).block(Block::default().title(l.load).borders(Borders::ALL));
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
        let dev_name = disk.name().to_string_lossy().to_string();
        let io_str = if app.tick_count > 1 && app.disk_io.len() >= 2 && app.disk_io_prev.len() >= 2
        {
            // Match device name with /proc/diskstats entry
            let short = dev_name
                .trim_start_matches("/dev/")
                .trim_start_matches('/');
            let prev_io = app
                .disk_io_prev
                .iter()
                .find(|d| d.name == short);
            let curr_io = app.disk_io.iter().find(|d| d.name == short);
            match (prev_io, curr_io) {
                (Some(prev), Some(curr)) => {
                    let r_rate = (curr.read_bytes.saturating_sub(prev.read_bytes)) as f64
                        / 1024.0
                        / 1024.0;
                    let w_rate = (curr.write_bytes.saturating_sub(prev.write_bytes)) as f64
                        / 1024.0
                        / 1024.0;
                    format!("  R {r_rate:>5.1} MB/s  W {w_rate:>5.1} MB/s")
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };
        disk_items.push(ListItem::new(format!(
            " {mount:<20} {pct:>3}% ({} / {} GB){}",
            (total - avail) / 1024 / 1024 / 1024,
            total / 1024 / 1024 / 1024,
            io_str
        )));
    }
    let disk_list = List::new(disk_items).block(
        Block::default()
            .title(l.disks)
            .borders(Borders::ALL),
    );
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
            .title(l.top_procs)
            .borders(Borders::ALL),
    );
    f.render_widget(proc_list, center_chunks[1]);

    // Right panel — Status summary
    let status_text = " Services: checking...\n\n Alerts: none\n\n Last backup: N/A";
    let status_widget =
        Paragraph::new(status_text).block(Block::default().title(l.status).borders(Borders::ALL));
    f.render_widget(status_widget, chunks[2]);
}

fn render_containers_tab(f: &mut Frame, app: &App, area: Rect, l: &lang::Lang) {
    if app.containers.is_empty() {
        let runtime = detect_container_runtime().unwrap_or_else(|| "docker/podman".to_string());
        let msg = match l.tabs[0].as_ref() {
            "Система" => format!(
                " Контейнеров не найдено (runtime: {runtime})\n\n \
                 Запустите: mnctl container start <name>"
            ),
            _ => format!(
                " No containers found (runtime: {runtime})\n\n \
                 Start a container with: mnctl container start <name>"
            ),
        };
        let widget = Paragraph::new(msg)
            .block(Block::default().title(l.containers).borders(Borders::ALL));
        f.render_widget(widget, area);
        return;
    }

    let header_txt = match l.tabs[0].as_ref() {
        "Система" => format!(" {:<25} {:<30} {}", "ИМЯ", "ОБРАЗ", "СТАТУС"),
        _ => format!(" {:<25} {:<30} {}", "NAME", "IMAGE", "STATUS"),
    };
    let header = ListItem::new(header_txt)
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
            .title(format!("{} ({}) ", l.containers.trim(), app.containers.len()))
            .borders(Borders::ALL),
    );
    f.render_widget(list, area);
}

fn render_network_tab(f: &mut Frame, app: &App, area: Rect, l: &lang::Lang) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),
            Constraint::Length(6),
            Constraint::Length(6),
        ])
        .split(area);

    // Network interfaces with per-interface RX/TX sparklines
    let num = app.net_ifaces.len();
    let iface_height = (num + 1) * 3 + 2; // header + data rows + borders
    let row_height = 6usize; // name+ip row + sparkline row

    let mut rows = Vec::with_capacity(num.max(1) * row_height);

    // Header
    let (iface_lbl, ip_lbl, rx_lbl, tx_lbl, state_lbl) = match l.tabs[0].as_ref() {
        "Система" => ("ИНТЕРФЕЙС", "IP", "RX (всего)", "TX (всего)", "СОСТ."),
        _ => ("INTERFACE", "IP", "RX (total)", "TX (total)", "STATE"),
    };
    rows.push(
        ListItem::new(format!(
            " {:<18} {:<16} {:>14} {:>14} {:>6}",
            iface_lbl, ip_lbl, rx_lbl, tx_lbl, state_lbl,
        ))
        .style(Style::default().add_modifier(Modifier::BOLD)),
    );

    for (i, iface) in app.net_ifaces.iter().enumerate() {
        let state = if iface.is_up {
            "UP".to_string()
        } else {
            "DOWN".to_string()
        };
        let name_fmt = if iface.is_up {
            format!(" {}", iface.name)
        } else {
            format!(" {} {}", "↓".to_string(), iface.name)
        };
        let style = if iface.is_up {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        };

        rows.push(
            ListItem::new(format!(
                " {:<18} {:<16} {:>10} MB {:>10} MB {:>6}",
                name_fmt,
                iface.ip,
                iface.rx_bytes / 1024 / 1024,
                iface.tx_bytes / 1024 / 1024,
                state,
            ))
            .style(style),
        );

        // Mini RX/TX sparkline
        let rx_sample = app.rx_history[i].last().copied().unwrap_or(0);
        let tx_sample = app.tx_history[i].last().copied().unwrap_or(0);
        let (rx_prefix, tx_prefix) = match l.tabs[0].as_ref() {
            "Система" => ("ПРМ", "ПРД"),
            _ => ("RX", "TX"),
        };
        rows.push(
            ListItem::new(format!(
                "   {rx_prefix} {} KB  {tx_prefix} {} KB",
                iface.rx_bytes / 1024,
                iface.tx_bytes / 1024,
            ))
            .style(Style::default().fg(Color::DarkGray)),
        );
    }

    if app.net_ifaces.is_empty() {
        rows.push(ListItem::new(format!(" {}", l.no_interfaces)));
    }

    let iface_list = List::new(rows).block(
        Block::default()
            .title(l.network_interfaces)
            .borders(Borders::ALL),
    );
    f.render_widget(iface_list, chunks[0]);

    // Per-interface sparklines (first 4 interfaces max)
    let spark_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(chunks[1]);

    for (i, iface) in app.net_ifaces.iter().take(4).enumerate() {
        let rx_data: Vec<u64> = app.rx_history[i].iter().map(|v| (*v).min(1024 * 1024)).collect();
        let max_rx = *rx_data.iter().max().unwrap_or(&1).max(&1);
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .title(format!(" {} RX ", iface.name))
                    .borders(Borders::ALL),
            )
            .data(&rx_data)
            .max(max_rx)
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(spark, spark_chunks[i]);
    }

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
                .title(l.listening_ports)
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(ports_widget, chunks[2]);
}

fn render_logs_tab(f: &mut Frame, app: &App, area: Rect, l: &lang::Lang) {
    let items: Vec<ListItem> = app
        .log_lines
        .iter()
        .map(|line| ListItem::new(format!(" {line}")))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(l.logs)
            .borders(Borders::ALL),
    );
    f.render_widget(list, area);
}

fn render_alerts_tab(f: &mut Frame, app: &App, area: Rect, l: &lang::Lang) {
    let items: Vec<ListItem> = app
        .alerts
        .iter()
        .map(|alert| {
            let style = if alert.starts_with("No active") || alert.starts_with("Нет активных") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            };
            ListItem::new(format!(" {alert}")).style(style)
        })
        .collect();

    let has_issues = app.alerts.iter().any(|a| !a.starts_with("No active") && !a.starts_with("Нет активных"));
    let title = if has_issues {
        format!("{} ({}) ", l.alerts.trim(), app.alerts.len())
    } else {
        l.alerts.to_string()
    };

    let list = List::new(items).block(Block::default().title(title).borders(Borders::ALL));
    f.render_widget(list, area);
}
