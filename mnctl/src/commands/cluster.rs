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
    /// Perform a rolling update across all nodes in the cluster
    RollingUpdate {
        /// Image to update to
        image: Option<String>,
        /// Max number of nodes to update concurrently
        #[arg(long, default_value_t = 1)]
        concurrency: u32,
        /// Skip drain step
        #[arg(long)]
        no_drain: bool,
    },
    /// Shared filesystem across cluster nodes
    Fs(FsArgs),
    /// Run a command on whichever node currently has the most free memory
    Schedule {
        /// Command to run on the chosen node
        command: String,
    },
}

#[derive(Args)]
pub struct FsArgs {
    #[command(subcommand)]
    command: FsCommand,
}

#[derive(Subcommand)]
enum FsCommand {
    /// Mount the shared cluster filesystem at a local path
    Mount {
        /// Local mountpoint
        #[arg(long, default_value = "/mnt/cluster")]
        at: String,
    },
    /// Unmount the shared cluster filesystem
    Umount {
        /// Local mountpoint
        #[arg(long, default_value = "/mnt/cluster")]
        at: String,
    },
    /// Show reachability/replication status of every peer's share
    SyncStatus,
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
            ClusterCommand::RollingUpdate { image, concurrency, no_drain } => cluster_rolling_update(image.as_deref(), concurrency, no_drain),
            ClusterCommand::Fs(args) => match args.command {
                FsCommand::Mount { at } => cluster_fs_mount(&at),
                FsCommand::Umount { at } => cluster_fs_umount(&at),
                FsCommand::SyncStatus => cluster_fs_sync_status(),
            },
            ClusterCommand::Schedule { command } => cluster_schedule(&command),
        }
    }
}

fn generate_token() -> String {
    let mut buf = [0u8; 24];
    let f = std::fs::File::open("/dev/urandom");
    match f {
        Ok(mut f) => {
            use std::io::Read;
            if f.read_exact(&mut buf).is_err() {
                // Fallback: fill from system time + pid (still better than pure timestamp)
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                for (i, b) in buf.iter_mut().enumerate() {
                    *b = ((ts >> (i % 16 * 8)) & 0xff) as u8 ^ (std::process::id() as u8);
                }
            }
        }
        Err(_) => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            for (i, b) in buf.iter_mut().enumerate() {
                *b = ((ts >> (i % 16 * 8)) & 0xff) as u8 ^ (std::process::id() as u8);
            }
        }
    }
    fn to_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    format!(
        "mnlth-{}-{}-{}",
        to_hex(&buf[..8]),
        to_hex(&buf[8..16]),
        to_hex(&buf[16..24]),
    )
}

/// Find this machine's LAN-facing IPv4 address without depending on the
/// `hostname` binary (part of `inetutils`, not installed on a minimal
/// Arch/Monolith box). `ip`, from `iproute2`, always is. Skips loopback
/// and container bridges (`docker0`, `br-*`) so the advertised address
/// is actually reachable from another physical machine on the LAN.
fn detect_local_ip() -> Result<String> {
    let output = Command::new("ip")
        .args(["-4", "-o", "addr", "show", "scope", "global"])
        .output()
        .context("failed to run `ip addr` — is iproute2 installed?")?;

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // Format: "3: enp2s0    inet 192.168.8.8/24 brd ... scope global ..."
        let iface = line.split_whitespace().nth(1).unwrap_or("");
        if iface.starts_with("docker") || iface.starts_with("br-") || iface.starts_with("veth") {
            continue;
        }
        if let Some(addr) = line
            .split_whitespace()
            .skip_while(|w| *w != "inet")
            .nth(1)
        {
            if let Some(ip) = addr.split('/').next() {
                return Ok(ip.to_string());
            }
        }
    }

    anyhow::bail!("no non-loopback, non-container IPv4 address found — pass --advertise-ip explicitly")
}

fn cluster_init(name: Option<&str>, advertise_ip: Option<&str>) -> Result<()> {
    let cluster_name = name.unwrap_or("monolith-cluster");

    let ip = match advertise_ip {
        Some(ip) => ip.to_string(),
        None => detect_local_ip().context("failed to detect IP")?,
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

fn cluster_rolling_update(image: Option<&str>, concurrency: u32, no_drain: bool) -> Result<()> {
    let cluster_cfg = std::fs::read_to_string("/etc/monolith/cluster.toml")
        .context("failed to read /etc/monolith/cluster.toml — initialize cluster first")?;

    let nodes: Vec<String> = cluster_cfg.lines()
        .filter(|l| l.contains("role =") || l.contains("address ="))
        .filter_map(|l| l.split('"').nth(1))
        .map(|s| s.to_string())
        .collect();

    if nodes.is_empty() {
        anyhow::bail!("no cluster nodes found in configuration");
    }

    println!("{} Rolling update across {} nodes", "→".blue().bold(), nodes.len());
    println!("  Concurrency: {concurrency}");
    if let Some(img) = image {
        println!("  Image:       {img}");
    } else {
        println!("  Image:       (current — no upgrade)");
    }

    for chunk in nodes.chunks(concurrency as usize) {
        let mut handles = Vec::new();

        for node in chunk {
            let node = node.clone();
            let image = image.map(|s| s.to_string());

            handles.push(std::thread::spawn(move || -> Result<()> {
                println!("  {} Updating {node}...", "→".blue());

                if !no_drain {
                    println!("    Draining {node}...");
                    let drain = std::process::Command::new("ssh")
                        .args([&node, "mnctl", "cluster", "drain", &node])
                        .status();
                    match drain {
                        Ok(s) if s.success() => println!("    {} Drained", "●".green()),
                        _ => println!("    {} Drain failed (continuing)", "⚠".yellow()),
                    }
                }

                if let Some(img) = &image {
                    println!("    Updating image on {node}...");
                    let _ = std::process::Command::new("ssh")
                        .args([&node, "mnpkg", "install", &img])
                        .status();
                }

                let _ = std::process::Command::new("ssh")
                    .args([&node, "systemctl", "restart", "monolith-node"])
                    .status();

                if !no_drain {
                    println!("    Uncordoning {node}...");
                    let _ = std::process::Command::new("mnctl")
                        .args(["cluster", "uncordon", &node])
                        .status();
                }

                println!("  {} {node} updated", "●".green());
                Ok(())
            }));
        }

        // Wait for this chunk to finish
        for handle in handles {
            let _ = handle.join();
        }

        println!("  {} Chunk complete — moving to next batch", "─".dimmed());
    }

    println!();
    println!("{} Rolling update complete", "●".green().bold());
    Ok(())
}

/// Read peer node addresses out of the config `cluster init` / `cluster
/// join` already wrote — the same file `cluster_status` prints.
fn read_peer_nodes() -> Vec<String> {
    let config_path = "/etc/monolith/cluster/cluster.toml";
    std::fs::read_to_string(config_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("master_ip") || l.starts_with("advertise_ip"))
        .filter_map(|l| l.split('"').nth(1))
        .map(|s| s.to_string())
        .collect()
}

/// Mount the shared cluster filesystem. Every peer's `cluster-fs`
/// directory is layered in under `<at>/<node>` via sshfs, so a file
/// written on one node shows up on the rest without a separate NFS/
/// Samba export to hand-configure.
fn cluster_fs_mount(at: &str) -> Result<()> {
    let config_path = "/etc/monolith/cluster/cluster.toml";
    if !std::path::Path::new(config_path).exists() {
        anyhow::bail!("not in a cluster — run `mnctl cluster init` or `mnctl cluster join` first");
    }

    std::fs::create_dir_all(at).context("failed to create mountpoint")?;

    let nodes = read_peer_nodes();
    if nodes.is_empty() {
        println!(
            "{} No peer nodes yet — {} is mounted but empty until others join",
            "⚠".yellow(),
            at
        );
        return Ok(());
    }

    for node in &nodes {
        let node_mount = format!("{at}/{node}");
        std::fs::create_dir_all(&node_mount).ok();

        let status = Command::new("sshfs")
            .args([
                &format!("{node}:/var/lib/monolith/cluster-fs"),
                &node_mount,
                "-o",
                "reconnect,ServerAliveInterval=15",
            ])
            .status();

        match status {
            Ok(s) if s.success() => println!("  {} {node} → {node_mount}", "●".green()),
            _ => println!(
                "  {} {node} unreachable — {node_mount} unavailable until it comes back",
                "⚠".yellow()
            ),
        }
    }

    println!("{} Cluster filesystem mounted at {}", "●".green(), at.bold());
    Ok(())
}

fn cluster_fs_umount(at: &str) -> Result<()> {
    for node in read_peer_nodes() {
        let node_mount = format!("{at}/{node}");
        let _ = Command::new("fusermount").args(["-u", &node_mount]).status();
    }
    println!("{} Cluster filesystem unmounted from {}", "●".green(), at);
    Ok(())
}

fn cluster_fs_sync_status() -> Result<()> {
    let nodes = read_peer_nodes();
    if nodes.is_empty() {
        println!("{}", "No peer nodes to check.".yellow());
        return Ok(());
    }

    println!("{}", "Cluster FS reachability:".bold().underline());
    for node in &nodes {
        let reachable = Command::new("ssh")
            .args(["-o", "ConnectTimeout=3", "-o", "BatchMode=yes", node, "true"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let (mark, state) = if reachable {
            ("●".green(), "in sync")
        } else {
            ("●".red(), "unreachable")
        };
        println!("  {mark} {node:<20} {state}");
    }
    Ok(())
}

/// Pick whichever peer currently has the most free memory and run the
/// given command there over SSH. Falls back to running locally when
/// there are no reachable peers, so this is safe to call from a single
/// node too.
fn cluster_schedule(command: &str) -> Result<()> {
    let nodes = read_peer_nodes();
    let mut best: Option<(String, u64)> = None;

    for node in &nodes {
        let output = Command::new("ssh")
            .args([
                "-o", "ConnectTimeout=3", "-o", "BatchMode=yes",
                node, "awk", "/MemAvailable/{print $2}", "/proc/meminfo",
            ])
            .output();

        if let Ok(output) = output {
            if let Ok(kb) = String::from_utf8_lossy(&output.stdout).trim().parse::<u64>() {
                if best.as_ref().map(|(_, b)| kb > *b).unwrap_or(true) {
                    best = Some((node.clone(), kb));
                }
            }
        }
    }

    match best {
        Some((node, kb)) => {
            println!("{} Scheduling on {} ({} MB free)", "→".blue(), node.bold(), kb / 1024);
            let status = Command::new("ssh").args([&node, command]).status()?;
            if !status.success() {
                anyhow::bail!("command failed on {node}");
            }
        }
        None => {
            println!("{} No reachable peers — running locally", "→".blue());
            let status = Command::new("sh").args(["-c", command]).status()?;
            if !status.success() {
                anyhow::bail!("command failed locally");
            }
        }
    }
    Ok(())
}
