//! Simple runtime locale detection for mnctl.
//! Reads $LANG and returns the appropriate string table.

pub struct Lang {
    pub tabs: Vec<&'static str>,
    pub header_up: &'static str,
    pub header_down: &'static str,
    pub footer: &'static str,
    pub cpu: &'static str,
    pub ram: &'static str,
    pub swap: &'static str,
    pub load: &'static str,
    pub disks: &'static str,
    pub top_procs: &'static str,
    pub status: &'static str,
    pub containers: &'static str,
    pub network_interfaces: &'static str,
    pub listening_ports: &'static str,
    pub logs: &'static str,
    pub alerts: &'static str,
    pub no_alerts: &'static str,
    pub no_containers: &'static str,
    pub no_interfaces: &'static str,
    pub no_active_tunnels: &'static str,
    pub not_available: &'static str,
}

pub fn detect() -> Lang {
    let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into());

    if lang.starts_with("ru") {
        Lang {
            tabs: vec!["Система", "Контейнеры", "Сеть", "Логи", "Оповещ"],
            header_up: "В работе",
            header_down: "Нет соединения",
            footer:
                " q:Выход  Tab:Далее  s:Система  c:Контейнеры  n:Сеть  l:Логи  a:Оповещ  ?:Помощь",
            cpu: " ЦП (60 с) ",
            ram: " ОЗУ ",
            swap: " Подкачка ",
            load: " Нагрузка ",
            disks: " Диски / I/O (1 с) ",
            top_procs: " Топ процессов ",
            status: " Статус ",
            containers: " Контейнеры ",
            network_interfaces: " Сетевые интерфейсы ",
            listening_ports: " Слушающие порты (топ 5) ",
            logs: " Системные логи (journalctl, посл. 50) ",
            alerts: " Оповещения ",
            no_alerts: "Нет активных оповещений",
            no_containers: "Контейнеров не найдено",
            no_interfaces: "Сетевые интерфейсы не обнаружены",
            no_active_tunnels: "Активных туннелей нет",
            not_available: "недоступен",
        }
    } else {
        Lang {
            tabs: vec!["System", "Containers", "Network", "Logs", "Alerts"],
            header_up: "UP",
            header_down: "DOWN",
            footer:
                " q:Quit  Tab:Next  s:System  c:Containers  n:Network  l:Logs  a:Alerts  ?:Help",
            cpu: " CPU (60s) ",
            ram: " RAM ",
            swap: " Swap ",
            load: " Load ",
            disks: " Disks / I/O rates (1s) ",
            top_procs: " Top Processes ",
            status: " Status ",
            containers: " Containers ",
            network_interfaces: " Network Interfaces ",
            listening_ports: " Listening Ports (top 5) ",
            logs: " System Logs (journalctl, last 50) ",
            alerts: " Alerts ",
            no_alerts: "No active alerts",
            no_containers: "No containers found",
            no_interfaces: "No network interfaces detected",
            no_active_tunnels: "No active tunnels found",
            not_available: "not available",
        }
    }
}
