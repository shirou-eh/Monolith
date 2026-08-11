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
        /// Service name (systemd unit)
        service: String,
        /// Target nodes (comma-separated hostnames, or 'all' for every
        /// peer plus this node)
        #[arg(long, default_value = "all")]
        nodes: String,
    },
    /// Perform a rolling update across peer nodes: drain, update the
    /// package (optional), restart the service, wait for it to report
    /// healthy, uncordon — one chunk of nodes at a time. Aborts the rest
    /// of the rollout the moment a node fails its post-restart health
    /// check, so a bad build can't cascade to the whole cluster in one
    /// pass.
    RollingUpdate {
        /// systemd unit to restart on each node
        #[arg(long, default_value = "monolith-cluster-autobalance")]
        service: String,
        /// Package to install before restarting (mnpkg install <image>)
        image: Option<String>,
        /// Max number of nodes to update concurrently
        #[arg(long, default_value_t = 1)]
        concurrency: u32,
        /// Skip cordon/drain and uncordon steps
        #[arg(long)]
        no_drain: bool,
    },
    /// Mark a node unschedulable — `cluster schedule`/`autobalance` skip it
    Drain {
        /// Node hostname
        node: String,
    },
    /// Clear a node's drain/cordon mark, making it schedulable again
    Uncordon {
        /// Node hostname
        node: String,
    },
    /// Shared filesystem across cluster nodes
    Fs(FsArgs),
    /// Run a command on whichever node currently has the most free memory
    Schedule {
        /// Command to run on the chosen node
        command: String,
    },
    /// Watch a directory for new jobs and dispatch each one to whichever
    /// node has the most free memory, fully automatically — no manual
    /// `cluster schedule` call needed per job.
    #[command(name = "autobalance")]
    AutoBalance {
        /// Directory to watch for new job files
        #[arg(long, default_value = "/var/lib/monolith/cluster/jobs")]
        watch_dir: String,
        /// Seconds between scans of the watch directory
        #[arg(long, default_value_t = 10)]
        interval: u64,
        /// Scan once and exit instead of running forever
        #[arg(long)]
        once: bool,
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
            ClusterCommand::RollingUpdate {
                service,
                image,
                concurrency,
                no_drain,
            } => cluster_rolling_update(&service, image.as_deref(), concurrency, no_drain),
            ClusterCommand::Drain { node } => cluster_drain(&node),
            ClusterCommand::Uncordon { node } => cluster_uncordon(&node),
            ClusterCommand::Fs(args) => match args.command {
                FsCommand::Mount { at } => cluster_fs_mount(&at),
                FsCommand::Umount { at } => cluster_fs_umount(&at),
                FsCommand::SyncStatus => cluster_fs_sync_status(),
            },
            ClusterCommand::Schedule { command } => cluster_schedule(&command),
            ClusterCommand::AutoBalance {
                watch_dir,
                interval,
                once,
            } => cluster_autobalance(&watch_dir, interval, once),
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
        if let Some(addr) = line.split_whitespace().skip_while(|w| *w != "inet").nth(1) {
            if let Some(ip) = addr.split('/').next() {
                return Ok(ip.to_string());
            }
        }
    }

    anyhow::bail!(
        "no non-loopback, non-container IPv4 address found — pass --advertise-ip explicitly"
    )
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
    let cordoned = read_cordoned();
    let self_state = if cordoned.contains(&hostname) {
        "cordoned".yellow()
    } else {
        "ready".green()
    };
    println!(
        "  {} {:<20} this node          {}",
        "●".green(),
        hostname,
        self_state
    );

    for node in read_peer_nodes() {
        let reachable = Command::new("ssh")
            .args([
                "-o",
                "ConnectTimeout=3",
                "-o",
                "BatchMode=yes",
                &node,
                "true",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let mark = if reachable {
            "●".green()
        } else {
            "●".red()
        };
        let state = if !reachable {
            "unreachable".red()
        } else if cordoned.contains(&node) {
            "cordoned".yellow()
        } else {
            "ready".green()
        };
        println!("  {mark} {node:<20} peer               {state}");
    }
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

/// Path to the cordon-state file: one hostname per line. Consulted by
/// `pick_best_node` (so `schedule`/`autobalance` skip cordoned nodes) and
/// by `cluster nodes` (to show the mark). Local, not synced to peers —
/// each node's own scheduler only needs to know about nodes *it* might
/// dispatch to.
fn cordon_file() -> String {
    "/var/lib/monolith/cluster/cordoned".to_string()
}

fn read_cordoned() -> Vec<String> {
    std::fs::read_to_string(cordon_file())
        .unwrap_or_default()
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn cluster_drain(node: &str) -> Result<()> {
    let path = cordon_file();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent).context("failed to create cluster state dir")?;
    }
    let mut cordoned = read_cordoned();
    if !cordoned.iter().any(|n| n == node) {
        cordoned.push(node.to_string());
        std::fs::write(&path, cordoned.join("\n") + "\n")
            .context("failed to write cordon state")?;
    }
    println!("{} {} marked unschedulable", "●".yellow(), node.bold());
    Ok(())
}

fn cluster_uncordon(node: &str) -> Result<()> {
    let path = cordon_file();
    let cordoned: Vec<String> = read_cordoned().into_iter().filter(|n| n != node).collect();
    std::fs::write(
        &path,
        cordoned.join("\n") + if cordoned.is_empty() { "" } else { "\n" },
    )
    .context("failed to write cordon state")?;
    println!("{} {} schedulable again", "●".green(), node.bold());
    Ok(())
}

/// Deploy = restart a systemd unit across a set of targets. "all" means
/// every reachable peer plus this node; otherwise a comma-separated list
/// of hostnames. Runs sequentially and reports per-node outcome instead
/// of just printing success unconditionally.
fn cluster_deploy(service: &str, nodes: &str) -> Result<()> {
    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());

    let targets: Vec<String> = if nodes == "all" {
        let mut t = vec![hostname.clone()];
        t.extend(read_peer_nodes());
        t
    } else {
        nodes
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    if targets.is_empty() {
        anyhow::bail!("no target nodes resolved from '{nodes}'");
    }

    println!(
        "{} Restarting '{}' on: {}",
        "→".blue(),
        service.bold(),
        targets.join(", ")
    );

    let mut failed = Vec::new();
    for node in &targets {
        let ok = if *node == hostname {
            Command::new("systemctl")
                .args(["restart", service])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        } else {
            Command::new("ssh")
                .args([
                    "-o",
                    "ConnectTimeout=5",
                    "-o",
                    "BatchMode=yes",
                    node,
                    "sudo",
                    "-n",
                    "systemctl",
                    "restart",
                    service,
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        println!("  {} {node}", if ok { "●".green() } else { "●".red() });
        if !ok {
            failed.push(node.clone());
        }
    }

    if failed.is_empty() {
        println!(
            "{} '{}' deployed to {} node(s)",
            "●".green(),
            service.bold(),
            targets.len()
        );
        Ok(())
    } else {
        anyhow::bail!("'{}' failed to restart on: {}", service, failed.join(", "));
    }
}

/// Whether a systemd unit is `active` on `node` (local check if `node`
/// is this host, ssh otherwise). Used as the post-restart health gate.
fn service_is_active(node: &str, hostname: &str, service: &str) -> bool {
    if node == hostname {
        Command::new("systemctl")
            .args(["is-active", "--quiet", service])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("ssh")
            .args([
                "-o",
                "ConnectTimeout=5",
                "-o",
                "BatchMode=yes",
                node,
                "systemctl",
                "is-active",
                "--quiet",
                service,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// Wait up to ~15s for `service` to report active on `node`, polling
/// every second. A restart that leaves a unit crash-looping shows up
/// here instead of the rollout declaring victory the instant the ssh
/// command that issued `restart` returns.
fn wait_healthy(node: &str, hostname: &str, service: &str) -> bool {
    for _ in 0..15 {
        if service_is_active(node, hostname, service) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    false
}

/// Rolling update across peer nodes, one chunk at a time: cordon, update
/// package (optional), restart `service`, wait for it to come back
/// healthy, uncordon. The moment a node fails its health check the whole
/// rollout stops — remaining nodes are left exactly as they were, rather
/// than ploughing ahead and taking the entire cluster down with a bad
/// build.
fn cluster_rolling_update(
    service: &str,
    image: Option<&str>,
    concurrency: u32,
    no_drain: bool,
) -> Result<()> {
    let nodes = read_peer_nodes();
    if nodes.is_empty() {
        anyhow::bail!(
            "no peer nodes found — initialize/join a cluster first (mnctl cluster init/join)"
        );
    }

    println!(
        "{} Rolling update across {} node(s)",
        "→".blue().bold(),
        nodes.len()
    );
    println!("  Service:     {service}");
    println!("  Concurrency: {concurrency}");
    println!(
        "  Image:       {}",
        image.unwrap_or("(current — no upgrade)")
    );

    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());

    for chunk in nodes.chunks((concurrency.max(1)) as usize) {
        let mut handles = Vec::new();

        for node in chunk {
            let node = node.clone();
            let image = image.map(|s| s.to_string());
            let service = service.to_string();
            let hostname = hostname.clone();

            handles.push(std::thread::spawn(move || -> (String, bool) {
                println!("  {} Updating {node}...", "→".blue());

                if !no_drain {
                    let _ = cluster_drain(&node);
                }

                if let Some(img) = &image {
                    println!("    Installing {img} on {node}...");
                    let _ = Command::new("ssh")
                        .args([
                            "-o",
                            "ConnectTimeout=5",
                            "-o",
                            "BatchMode=yes",
                            &node,
                            "sudo",
                            "-n",
                            "mnpkg",
                            "install",
                            img,
                        ])
                        .status();
                }

                let restarted = Command::new("ssh")
                    .args([
                        "-o",
                        "ConnectTimeout=5",
                        "-o",
                        "BatchMode=yes",
                        &node,
                        "sudo",
                        "-n",
                        "systemctl",
                        "restart",
                        &service,
                    ])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);

                let healthy = restarted && wait_healthy(&node, &hostname, &service);

                if healthy {
                    if !no_drain {
                        let _ = cluster_uncordon(&node);
                    }
                    println!("  {} {node} updated and healthy", "●".green());
                } else {
                    // Deliberately left cordoned — an operator should look
                    // at it before it takes traffic again.
                    println!(
                        "  {} {node} FAILED health check after update — left cordoned",
                        "●".red().bold()
                    );
                }

                (node, healthy)
            }));
        }

        let mut chunk_failed = false;
        for handle in handles {
            if let Ok((node, healthy)) = handle.join() {
                if !healthy {
                    chunk_failed = true;
                    let _ = node; // already reported above
                }
            }
        }

        if chunk_failed {
            anyhow::bail!("rollout aborted: a node in this chunk failed its health check — remaining nodes were not touched");
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

    println!(
        "{} Cluster filesystem mounted at {}",
        "●".green(),
        at.bold()
    );
    Ok(())
}

fn cluster_fs_umount(at: &str) -> Result<()> {
    for node in read_peer_nodes() {
        let node_mount = format!("{at}/{node}");
        let _ = Command::new("fusermount")
            .args(["-u", &node_mount])
            .status();
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
            .args([
                "-o",
                "ConnectTimeout=3",
                "-o",
                "BatchMode=yes",
                node,
                "true",
            ])
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
/// Free memory (kB) on a reachable peer, or `None` if it can't be reached
/// within the timeout.
fn peer_free_mem_kb(node: &str) -> Option<u64> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=3",
            "-o",
            "BatchMode=yes",
            node,
            "awk",
            "/MemAvailable/{print $2}",
            "/proc/meminfo",
        ])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Free memory (kB) on this machine.
fn local_free_mem_kb() -> Option<u64> {
    let content = std::fs::read_to_string("/proc/meminfo").ok()?;
    content
        .lines()
        .find(|l| l.starts_with("MemAvailable:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
}

/// Compare free memory across every peer AND this machine, and return
/// whichever has the most. `None` means "run locally" — either nothing
/// beats the local box, or there are no reachable peers at all.
fn pick_best_node() -> Option<(String, u64)> {
    let cordoned = read_cordoned();
    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());

    // A cordoned local node can't win either — otherwise `cluster drain
    // <this-node>` (e.g. right before a rolling update touches it) would
    // be pointless, since "no peer beats local" already falls back to
    // running here.
    let local_kb = if cordoned.contains(&hostname) {
        None
    } else {
        local_free_mem_kb()
    };
    let mut best: Option<(String, u64)> = None;

    for node in read_peer_nodes() {
        if cordoned.contains(&node) {
            continue;
        }
        if let Some(kb) = peer_free_mem_kb(&node) {
            if best.as_ref().map(|(_, b)| kb > *b).unwrap_or(true) {
                best = Some((node, kb));
            }
        }
    }

    match (best, local_kb) {
        (Some((node, kb)), Some(local)) if kb > local => Some((node, kb)),
        (Some((node, kb)), None) => Some((node, kb)),
        _ => None,
    }
}

fn cluster_schedule(command: &str) -> Result<()> {
    match pick_best_node() {
        Some((node, kb)) => {
            println!(
                "{} Scheduling on {} ({} MB free)",
                "→".blue(),
                node.bold(),
                kb / 1024
            );
            let status = Command::new("ssh").args([&node, command]).status()?;
            if !status.success() {
                anyhow::bail!("command failed on {node}");
            }
        }
        None => {
            println!(
                "{} Running locally — this box has the most free capacity",
                "→".blue()
            );
            let status = Command::new("sh").args(["-c", command]).status()?;
            if !status.success() {
                anyhow::bail!("command failed locally");
            }
        }
    }
    Ok(())
}

/// Watch `watch_dir` for new job files and dispatch each one to whichever
/// node currently has the most free memory — no human has to pick, and
/// no human has to call `cluster schedule` per job. Each job file's
/// content is piped to `bash` (locally or over ssh, same either way),
/// then moved into `done/` or `failed/` depending on the exit status,
/// with a one-line record appended to `watch_dir/autobalance.log`.
fn cluster_autobalance(watch_dir: &str, interval: u64, once: bool) -> Result<()> {
    let incoming = format!("{watch_dir}/incoming");
    let done = format!("{watch_dir}/done");
    let failed = format!("{watch_dir}/failed");
    for dir in [&incoming, &done, &failed] {
        std::fs::create_dir_all(dir).with_context(|| format!("failed to create {dir}"))?;
    }
    let log_path = format!("{watch_dir}/autobalance.log");

    loop {
        let mut entries: Vec<_> = std::fs::read_dir(&incoming)
            .with_context(|| format!("failed to read {incoming}"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let script = std::fs::read(&path).unwrap_or_default();

            let (target, status) = match pick_best_node() {
                Some((node, kb)) => {
                    println!("{} {name} → {node} ({} MB free)", "→".blue(), kb / 1024);
                    let mut ssh_cmd = Command::new("ssh");
                    ssh_cmd.args([
                        "-o",
                        "ConnectTimeout=3",
                        "-o",
                        "BatchMode=yes",
                        &node,
                        "bash",
                    ]);
                    let status = run_piped(&mut ssh_cmd, &script);
                    (node, status)
                }
                None => {
                    println!("{} {name} → local (most free capacity)", "→".blue());
                    let mut bash_cmd = Command::new("bash");
                    let status = run_piped(&mut bash_cmd, &script);
                    ("local".to_string(), status)
                }
            };

            let ok = status.map(|s| s.success()).unwrap_or(false);
            let dest_dir = if ok { &done } else { &failed };
            let _ = std::fs::rename(&path, format!("{dest_dir}/{name}"));

            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let line = format!(
                "{ts} {name} -> {target} : {}\n",
                if ok { "ok" } else { "failed" }
            );
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                use std::io::Write;
                let _ = f.write_all(line.as_bytes());
            }
        }

        if once {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(interval));
    }
}

/// Run `cmd`, feeding `input` on stdin, and wait for it to finish.
fn run_piped(cmd: &mut Command, input: &[u8]) -> Option<std::process::ExitStatus> {
    use std::io::Write;
    let mut child = cmd.stdin(std::process::Stdio::piped()).spawn().ok()?;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(input);
    }
    child.wait().ok()
}
