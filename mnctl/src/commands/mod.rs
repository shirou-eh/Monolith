pub mod backup;
pub mod bench;
pub mod cluster;
pub mod config;
pub mod container;
pub mod deploy;
pub mod disk;
pub mod info;
pub mod iso;
pub mod kube;
pub mod monitor;
pub mod network;
pub mod notify;
pub mod plugin;
pub mod profile;
pub mod proxy;
pub mod security;
pub mod service;
pub mod template;
pub mod update;
pub mod vpn;
pub mod web;

use clap::{Parser, Subcommand};

/// mnctl — Monolith OS control CLI
///
/// Unified server management for Monolith OS.
/// Every millisecond matters. Every byte counts. Every default is intentional.
#[derive(Parser)]
#[command(
    name = "mnctl",
    version = env!("CARGO_PKG_VERSION"),
    about = "Monolith OS control CLI — unified server management",
    long_about = "mnctl is the unified control interface for Monolith OS.\n\n\
        It provides a single coherent CLI for managing services, containers,\n\
        deployments, monitoring, security, updates, backups, networking,\n\
        VPN tunnels, reverse proxies, clusters, benchmarks, and templates.",
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage system services (systemd units)
    Service(service::ServiceArgs),

    /// Manage containers (Docker / Podman)
    Container(container::ContainerArgs),

    /// Zero-config application deployment
    Deploy(deploy::DeployArgs),

    /// Monitoring and observability
    Monitor(monitor::MonitorArgs),

    /// Security management (firewall, AppArmor, fail2ban, auditing)
    Security(security::SecurityArgs),

    /// System update management with snapshot safety
    Update(update::UpdateArgs),

    /// Backup management (Btrfs snapshots + restic)
    Backup(backup::BackupArgs),

    /// Network management
    Network(network::NetworkArgs),

    /// WireGuard VPN management
    Vpn(vpn::VpnArgs),

    /// Reverse proxy management (nginx + ACME TLS)
    Proxy(proxy::ProxyArgs),

    /// Multi-node cluster management
    Cluster(cluster::ClusterArgs),

    /// Built-in benchmarking suite
    Bench(bench::BenchArgs),

    /// Application templates
    Template(template::TemplateArgs),

    /// System information
    Info(info::InfoArgs),

    /// Monolith configuration management
    Config(config::ConfigArgs),

    /// Disk inventory and SMART health monitoring
    Disk(disk::DiskArgs),

    /// Kubernetes (k3s) integration
    Kube(kube::KubeArgs),

    /// Plugin management — extend mnctl with external commands
    Plugin(plugin::PluginArgs),

    /// Resource profile (lite / full / pro) — toggle the heavy stack
    Profile(profile::ProfileArgs),

    /// Notifications (webhook + SMTP)
    Notify(notify::NotifyArgs),

    /// Build a custom Monolith OS ISO image
    Iso(iso::IsoArgs),

    /// Web management UI (mnweb)
    Web(web::WebArgs),
}

impl Cli {
    pub async fn run(self) -> anyhow::Result<()> {
        match self.command {
            Commands::Service(args) => args.run().await,
            Commands::Container(args) => args.run().await,
            Commands::Deploy(args) => args.run().await,
            Commands::Monitor(args) => args.run().await,
            Commands::Security(args) => args.run().await,
            Commands::Update(args) => args.run().await,
            Commands::Backup(args) => args.run().await,
            Commands::Network(args) => args.run().await,
            Commands::Vpn(args) => args.run().await,
            Commands::Proxy(args) => args.run().await,
            Commands::Cluster(args) => args.run().await,
            Commands::Bench(args) => args.run().await,
            Commands::Template(args) => args.run().await,
            Commands::Info(args) => args.run().await,
            Commands::Config(args) => args.run().await,
            Commands::Disk(args) => args.run().await,
            Commands::Kube(args) => args.run().await,
            Commands::Plugin(args) => args.run().await,
            Commands::Profile(args) => args.run().await,
            Commands::Notify(args) => args.run().await,
            Commands::Iso(args) => args.run().await,
            Commands::Web(args) => args.run().await,
        }
    }
}
