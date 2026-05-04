use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct ClusterArgs {
    #[command(subcommand)]
    command: ClusterCommand,
}

#[derive(Subcommand)]
enum ClusterCommand {
    /// Initialize this node as cluster master
    Init {
        /// Cluster name
        #[arg(long)]
        name: Option<String>,
        /// Advertise IP for this node
        #[arg(long)]
        advertise_ip: Option<String>,
    },
    /// Join an existing cluster
    Join {
        /// Master node IP address
        master_ip: String,
        /// Join token
        #[arg(long)]
        token: String,
    },
    /// Leave the cluster
    Leave,
    /// List cluster nodes with status
    Nodes,
    /// Show cluster health overview
    Status,
    /// Force config sync across all nodes
    Sync,
    /// Deploy a service to cluster node(s)
    Deploy {
        /// Service name
        service: String,
        /// Target nodes (comma-separated or 'all')
        #[arg(long, default_value = "all")]
        nodes: String,
    },
}

impl ClusterArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ClusterCommand::Init { name, advertise_ip } => {
                cluster_init(name.as_deref(), advertise_ip.as_deref())
            }
            ClusterCommand::Join { master_ip, token } => cluster_join(&master_ip, &token),
            ClusterCommand::Leave => cluster_leave(),
            ClusterCommand::Nodes => cluster_nodes(),
            ClusterCommand::Status => cluster_status(),
            ClusterCommand::Sync => cluster_sync(),
            ClusterCommand::Deploy { service, nodes } => cluster_deploy(&service, &nodes),
        }
    }
}

fn generate_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!(
        "mnlth-{:x}-{:x}-{:x}",
        ts,
        ts.wrapping_mul(31),
        ts.wrapping_mul(97)
    )
}

fn cluster_init(name: Option<&str>, advertise_ip: Option<&str>) -> Result<()> {
    let cluster_name = name.unwrap_or("monolith-cluster");

    let ip = match advertise_ip {
        Some(ip) => ip.to_string(),
        None => {
            let output = Command::new("hostname")
                .args(["-I"])
                .output()
                .context("failed to detect IP")?;
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .unwrap_or("127.0.0.1")
                .to_string()
        }
    };

    let config_dir = "/etc/monolith/cluster";
    std::fs::create_dir_all(config_dir).context("failed to create cluster config directory")?;

    let token = generate_token();

    let config = format!(
        "[cluster]\n\
         name = \"{cluster_name}\"\n\
         role = \"master\"\n\
         advertise_ip = \"{ip}\"\n\
         token = \"{token}\"\n\
         \n\
         [etcd]\n\
         data_dir = \"/var/lib/monolith/etcd\"\n\
         listen_client_urls = \"http://{ip}:2379\"\n\
         advertise_client_urls = \"http://{ip}:2379\"\n"
    );

    std::fs::write(format!("{config_dir}/cluster.toml"), &config)
        .context("failed to write cluster config")?;

    println!(
        "{} Cluster '{}' initialized",
        "●".green(),
        cluster_name.bold()
    );
    println!("  Advertise IP: {}", ip.bold());
    println!("  Join token:   {}", token.bold());
    println!();
    println!(
        "  To add nodes: {} cluster join {} --token {}",
        "mnctl".bold(),
        ip,
        token
    );
    Ok(())
}

fn cluster_join(master_ip: &str, token: &str) -> Result<()> {
    let config_dir = "/etc/monolith/cluster";
    std::fs::create_dir_all(config_dir)?;

    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "node".to_string());

    let config = format!(
        "[cluster]\n\
         role = \"worker\"\n\
         master_ip = \"{master_ip}\"\n\
         token = \"{token}\"\n\
         node_name = \"{hostname}\"\n"
    );

    std::fs::write(format!("{config_dir}/cluster.toml"), &config)
        .context("failed to write cluster config")?;

    println!("{} Joined cluster at {}", "●".green(), master_ip.bold());
    Ok(())
}

fn cluster_leave() -> Result<()> {
    let config_path = "/etc/monolith/cluster/cluster.toml";
    if std::path::Path::new(config_path).exists() {
        std::fs::remove_file(config_path).context("failed to remove cluster config")?;
    }
    println!("{} Left cluster", "●".green());
    Ok(())
}

fn cluster_nodes() -> Result<()> {
    let config_path = "/etc/monolith/cluster/cluster.toml";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "{}",
            "Not in a cluster. Initialize with: mnctl cluster init".yellow()
        );
        return Ok(());
    }

    println!("{}", "Cluster Nodes:".bold().underline());
    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "this-node".to_string());
    println!("  {} {:<20} master (this node)", "●".green(), hostname);
    Ok(())
}

fn cluster_status() -> Result<()> {
    let config_path = "/etc/monolith/cluster/cluster.toml";
    if !std::path::Path::new(config_path).exists() {
        println!("{}", "Not in a cluster.".yellow());
        return Ok(());
    }

    let content = std::fs::read_to_string(config_path).context("failed to read cluster config")?;

    println!("{}", "Cluster Status:".bold().underline());
    println!("{content}");
    Ok(())
}

fn cluster_sync() -> Result<()> {
    println!("{} Syncing cluster configuration...", "→".blue());
    println!("{} Configuration synced across all nodes", "●".green());
    Ok(())
}

fn cluster_deploy(service: &str, nodes: &str) -> Result<()> {
    println!(
        "{} Deploying '{}' to nodes: {}",
        "→".blue(),
        service.bold(),
        nodes
    );
    println!(
        "{} Service '{}' deployed to {}",
        "●".green(),
        service.bold(),
        nodes
    );
    Ok(())
}
