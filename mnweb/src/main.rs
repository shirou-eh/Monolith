//! Monolith OS — web management UI.
//!
//! Serves a small SPA from in-binary assets and exposes a JSON API that
//! reads system state via sysinfo + a few well-scoped subprocess calls.
//!
//! By default we bind to `127.0.0.1:9911`. Set `--bind` (or the env var
//! `MNWEB_BIND`) to expose it on a different address. Production deployments
//! should reverse-proxy through nginx/`mnctl proxy`.

use anyhow::Context;
use axum::{
    extract::{Query, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use sysinfo::System;
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod assets;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "mnweb",
    version = env!("CARGO_PKG_VERSION"),
    about = "Monolith OS web management UI"
)]
struct Args {
    /// Bind address (default 127.0.0.1:9911)
    #[arg(long, env = "MNWEB_BIND", default_value = "127.0.0.1:9911")]
    bind: String,
    /// Read-only mode (disable any future mutating endpoints)
    #[arg(long, env = "MNWEB_READ_ONLY", default_value_t = false)]
    read_only: bool,
    /// Bearer token for API authentication (if unset, reads from config or disables auth)
    #[arg(long, env = "MNWEB_TOKEN")]
    token: Option<String>,
}

struct AppState {
    sys: Mutex<System>,
    /// Reserved for future endpoints that gate behavior on CLI flags.
    #[allow(dead_code)]
    args: Args,
    /// Resolved auth token (if any)
    auth_token: Option<String>,
    /// Number of log lines to tail from journalctl
    log_tail_lines: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_target(false)
        .init();

    let args = Args::parse();

    // Resolve token: CLI flag > config file > no auth
    let auth_token = args.token.clone().or_else(|| {
        let cfg = std::fs::read_to_string("/etc/monolith/monolith.toml").ok()?;
        let doc = cfg.parse::<toml::Value>().ok()?;
        doc.get("webui")
            .and_then(|w| w.get("token"))
            .and_then(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .map(|t| t.to_string())
    });

    if auth_token.is_some() {
        tracing::info!("API authentication enabled");
    } else {
        tracing::warn!("API authentication disabled — set --token or [webui].token in config");
    }

    // Read log_tail_lines from config (default 500)
    let log_tail_lines: usize = (|| {
        let cfg = std::fs::read_to_string("/etc/monolith/monolith.toml").ok()?;
        let doc: toml::Value = cfg.parse().ok()?;
        doc.get("webui")
            .and_then(|w| w.get("log_tail_lines"))
            .and_then(|v| v.as_integer())
            .map(|n| n as usize)
    })()
    .unwrap_or(500);

    let state = Arc::new(AppState {
        sys: Mutex::new(System::new_all()),
        args: args.clone(),
        auth_token,
        log_tail_lines,
    });

    // API routes protected by auth middleware
    let api_routes = Router::new()
        .route("/api/overview", get(api_overview))
        .route("/api/services", get(api_services))
        .route("/api/containers", get(api_containers))
        .route("/api/disks", get(api_disks))
        .route("/api/cluster", get(api_cluster))
        .route("/api/templates", get(api_templates))
        .route("/api/logs", get(api_logs))
        .route("/api/metrics/export", get(api_metrics_export))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let app = Router::new()
        .route("/", get(index))
        .route("/static/app.css", get(asset_css))
        .route("/static/app.js", get(asset_js))
        .route("/healthz", get(healthz))
        .merge(api_routes)
        .with_state(state);

    let addr: SocketAddr = args
        .bind
        .parse()
        .with_context(|| format!("invalid bind address: {}", args.bind))?;
    tracing::info!(%addr, "mnweb starting");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn auth_middleware(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let Some(ref expected) = state.auth_token else {
        return next.run(req).await;
    };

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(val) if val.strip_prefix("Bearer ").unwrap_or("") == expected.as_str() => {
            next.run(req).await
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "unauthorized — provide a valid Bearer token",
        )
            .into_response(),
    }
}

async fn index() -> Response {
    asset(assets::INDEX_HTML, "text/html; charset=utf-8")
}

async fn asset_css() -> Response {
    asset(assets::APP_CSS, "text/css; charset=utf-8")
}

async fn asset_js() -> Response {
    asset(assets::APP_JS, "application/javascript; charset=utf-8")
}

fn asset(body: &'static str, ctype: &'static str) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, ctype)],
        body.to_string(),
    )
        .into_response()
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

#[derive(Debug, Serialize)]
struct OverviewResponse {
    version: String,
    codename: String,
    hostname: String,
    os: String,
    kernel: String,
    arch: String,
    uptime_seconds: u64,
    load_avg: [f64; 3],
    cpu: CpuInfo,
    memory: MemoryInfo,
}

#[derive(Debug, Serialize)]
struct CpuInfo {
    brand: String,
    cores: usize,
    usage_pct: f32,
}

#[derive(Debug, Serialize)]
struct MemoryInfo {
    total: u64,
    used: u64,
    swap_total: u64,
    swap_used: u64,
}

async fn api_overview(State(state): State<Arc<AppState>>) -> Json<OverviewResponse> {
    let mut sys = state.sys.lock().await;
    sys.refresh_all();

    let cores = sys.cpus().len().max(1);
    let usage_pct: f32 = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>() / cores as f32;
    let brand = sys
        .cpus()
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let load = System::load_average();

    Json(OverviewResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        codename: "Onyx".to_string(),
        hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
        os: System::long_os_version().unwrap_or_else(|| "unknown".to_string()),
        kernel: System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        arch: std::env::consts::ARCH.to_string(),
        uptime_seconds: System::uptime(),
        load_avg: [load.one, load.five, load.fifteen],
        cpu: CpuInfo {
            brand,
            cores,
            usage_pct,
        },
        memory: MemoryInfo {
            total: sys.total_memory(),
            used: sys.used_memory(),
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
        },
    })
}

#[derive(Debug, Serialize)]
struct ServiceRow {
    name: String,
    load: String,
    active: String,
}

async fn api_services() -> Json<Vec<ServiceRow>> {
    let output = std::process::Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--no-pager",
            "--plain",
            "--no-legend",
            "--all",
        ])
        .output();
    let mut rows = Vec::new();
    if let Ok(o) = output {
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let active = parts[2];
                // Include activating/deactivating units alongside
                // active, inactive, and failed (fix 1.0.2-p2).
                rows.push(ServiceRow {
                    name: parts[0].to_string(),
                    load: parts[1].to_string(),
                    active: active.to_string(),
                });
            }
        }
    }
    Json(rows)
}

#[derive(Debug, Serialize)]
struct ContainerRow {
    name: String,
    image: String,
    status: String,
    runtime: String,
}

async fn api_containers() -> Json<Vec<ContainerRow>> {
    // Detect runtime by checking which socket is present
    let bins: &[(&str, &str)] = &[
        ("docker", "docker"),
        ("podman", "podman"),
    ];
    for (bin, runtime_name) in bins {
        let sock_exists = match *runtime_name {
            "podman" => std::path::Path::new("/run/podman/podman.sock").exists(),
            _ => std::path::Path::new("/run/docker.sock").exists(),
        };
        if !sock_exists && std::process::Command::new(bin).output().is_err() {
            continue;
        }
        let output = std::process::Command::new(bin)
            .args(["ps", "--format", "{{.Names}}\t{{.Image}}\t{{.Status}}"])
            .output();
        if let Ok(o) = output {
            if o.status.success() {
                let mut rows = Vec::new();
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let parts: Vec<&str> = line.splitn(3, '\t').collect();
                    if parts.len() == 3 {
                        rows.push(ContainerRow {
                            name: parts[0].to_string(),
                            image: parts[1].to_string(),
                            status: parts[2].to_string(),
                            runtime: runtime_name.to_string(),
                        });
                    }
                }
                return Json(rows);
            }
        }
    }
    Json(Vec::new())
}

#[derive(Debug, Serialize)]
struct DiskRow {
    name: String,
    size: u64,
    mount: String,
    fstype: String,
}

async fn api_disks() -> Json<Vec<DiskRow>> {
    let mut rows = Vec::new();
    for d in sysinfo::Disks::new_with_refreshed_list().list() {
        rows.push(DiskRow {
            name: d.name().to_string_lossy().to_string(),
            size: d.total_space(),
            mount: d.mount_point().to_string_lossy().to_string(),
            fstype: d.file_system().to_string_lossy().to_string(),
        });
    }
    Json(rows)
}

#[derive(Debug, Serialize)]
struct ClusterResponse {
    in_cluster: bool,
    config: String,
}

async fn api_cluster() -> Json<ClusterResponse> {
    let path = "/etc/monolith/cluster/cluster.toml";
    if let Ok(content) = std::fs::read_to_string(path) {
        Json(ClusterResponse {
            in_cluster: true,
            config: content,
        })
    } else {
        Json(ClusterResponse {
            in_cluster: false,
            config: String::new(),
        })
    }
}

#[derive(Debug, Serialize)]
struct TemplateRow {
    name: &'static str,
    description: &'static str,
    category: &'static str,
}

async fn api_templates() -> Json<Vec<TemplateRow>> {
    Json(vec![
        TemplateRow {
            name: "minecraft",
            description: "Minecraft Java Edition server",
            category: "Game Servers",
        },
        TemplateRow {
            name: "cs2",
            description: "Counter-Strike 2 dedicated server",
            category: "Game Servers",
        },
        TemplateRow {
            name: "valheim",
            description: "Valheim dedicated server",
            category: "Game Servers",
        },
        TemplateRow {
            name: "palworld",
            description: "Palworld dedicated server",
            category: "Game Servers",
        },
        TemplateRow {
            name: "factorio",
            description: "Factorio headless server",
            category: "Game Servers",
        },
        TemplateRow {
            name: "terraria",
            description: "Terraria TShock server",
            category: "Game Servers",
        },
        TemplateRow {
            name: "postgresql",
            description: "PostgreSQL 16",
            category: "Databases",
        },
        TemplateRow {
            name: "mariadb",
            description: "MariaDB 11",
            category: "Databases",
        },
        TemplateRow {
            name: "mongodb",
            description: "MongoDB 7",
            category: "Databases",
        },
        TemplateRow {
            name: "redis",
            description: "Redis 7",
            category: "Databases",
        },
        TemplateRow {
            name: "nodejs-app",
            description: "Node.js application",
            category: "Web",
        },
        TemplateRow {
            name: "discord-bot-python",
            description: "Python Discord bot",
            category: "Bots",
        },
        TemplateRow {
            name: "nginx-reverse-proxy",
            description: "nginx reverse proxy + ACME TLS",
            category: "Infrastructure",
        },
    ])
}

#[derive(Debug, Serialize)]
struct LogsResponse {
    lines: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MetricsExportQuery {
    format: Option<String>,
    since: Option<String>,
}

async fn api_metrics_export(Query(query): Query<MetricsExportQuery>) -> Response {

    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname = System::host_name().unwrap_or_else(|| "unknown".into());
    let timestamp = chrono::Utc::now().to_rfc3339();

    let cpu_per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    let cpu_avg = if cpu_per_core.is_empty() {
        0.0f32
    } else {
        cpu_per_core.iter().sum::<f32>() / cpu_per_core.len() as f32
    };

    let fmt = query.format.as_deref().unwrap_or("json");

    match fmt {
        "csv" => {
            let mut csv = String::new();
            csv.push_str("hostname,timestamp,cpu_avg_pct");
            for (i, _) in cpu_per_core.iter().enumerate() {
                csv.push_str(&format!(",cpu_core_{i}_pct"));
            }
            csv.push_str(",ram_used_bytes,ram_total_bytes,swap_used_bytes,swap_total_bytes\n");
            csv.push_str(&format!("{hostname},{timestamp},{:.1}", cpu_avg));
            for core in &cpu_per_core {
                csv.push_str(&format!(",{core:.1}"));
            }
            csv.push_str(&format!(
                ",{},{},{},{}",
                sys.used_memory(),
                sys.total_memory(),
                sys.used_swap(),
                sys.total_swap()
            ));
            csv.push('\n');
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/csv; charset=utf-8")],
                csv,
            )
                .into_response()
        }
        _ => {
            let disks: Vec<serde_json::Value> = sysinfo::Disks::new_with_refreshed_list()
                .list()
                .iter()
                .map(|d| {
                    let total = d.total_space();
                    let avail = d.available_space();
                    let used = total.saturating_sub(avail);
                    let pct = if total > 0 {
                        used as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    };
                    serde_json::json!({
                        "mount": d.mount_point().to_string_lossy(),
                        "used_bytes": used,
                        "total_bytes": total,
                        "used_pct": (pct * 10.0).round() / 10.0,
                    })
                })
                .collect();

            let doc = serde_json::json!({
                "schema": "monolith-export/1",
                "hostname": hostname,
                "timestamp": timestamp,
                "cpu": {
                    "avg_pct": (cpu_avg * 10.0).round() / 10.0,
                    "per_core_pct": cpu_per_core,
                },
                "ram": {
                    "used_bytes": sys.used_memory(),
                    "total_bytes": sys.total_memory(),
                },
                "swap": {
                    "used_bytes": sys.used_swap(),
                    "total_bytes": sys.total_swap(),
                },
                "disks": disks,
            });
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                serde_json::to_string_pretty(&doc).unwrap_or_default(),
            )
                .into_response()
        }
    }
}

async fn api_logs(State(state): State<Arc<AppState>>) -> Json<LogsResponse> {
    let n = state.log_tail_lines.to_string();
    let output = std::process::Command::new("journalctl")
        .args(["--no-pager", "-n", &n, "--output=short"])
        .output();
    let lines = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect(),
        Err(_) => vec!["journalctl not available on this host".to_string()],
    };
    Json(LogsResponse { lines })
}
