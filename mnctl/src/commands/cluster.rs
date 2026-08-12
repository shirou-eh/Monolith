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
    /// Show real Raft quorum health via etcd — who's leader, whether a
    /// majority is currently reachable. This is what `rolling-update`
    /// gates on before touching anything.
    Quorum,
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
            ClusterCommand::Quorum => cluster_quorum(),
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

// --- etcd-backed quorum ---------------------------------------------
//
// Before this, "master"/"worker" in cluster.toml was just a label —
// nothing ever elected a leader or agreed on cluster membership, and
// nothing could tell the difference between "a peer is unreachable"
// and "we've lost quorum and shouldn't be accepting writes anywhere".
// etcd is a proven Raft implementation; the right call here is to run
// it as the actual consensus layer rather than hand-roll one. This
// section wraps `etcd`/`etcdctl` the same way the rest of this file
// wraps `ssh`/`systemctl` — shelling out, not a Rust etcd client, for
// the same one-thing-to-audit reason the update-tarball extraction
// uses direct argv instead of `sh -c`.
//
// Verified against a real local 3-node etcd cluster before any of this
// was wired up: bootstrap, `member add`-based join, leader election,
// and failover (killing the leader, confirming the survivors re-elect
// and keep serving) — see the commit message for how.

const ETCD_CLIENT_PORT: u16 = 2379;
const ETCD_PEER_PORT: u16 = 2380;
const ETCD_DATA_DIR: &str = "/var/lib/monolith/etcd";
const ETCD_UNIT_PATH: &str = "/etc/systemd/system/monolith-etcd.service";
const ETCD_UNIT_NAME: &str = "monolith-etcd.service";
const CLUSTER_CONFIG_DIR: &str = "/etc/monolith/cluster";
const CLUSTER_CONFIG_PATH: &str = "/etc/monolith/cluster/cluster.toml";

fn etcd_client_url(ip: &str) -> String {
    format!("http://{ip}:{ETCD_CLIENT_PORT}")
}

fn etcd_peer_url(ip: &str) -> String {
    format!("http://{ip}:{ETCD_PEER_PORT}")
}

/// systemd unit for the local etcd member. `Type=notify` + etcd's own
/// sd_notify support means systemd genuinely waits for etcd to finish
/// joining the cluster (not just forking) before `--now` returns —
/// callers don't need their own sleep-and-poll after starting this.
fn etcd_unit_content(name: &str, advertise_ip: &str, initial_cluster: &str, state: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Monolith cluster etcd (Raft quorum/consensus)\n\
         Documentation=https://etcd.io/docs\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=notify\n\
         User=root\n\
         ExecStart=/usr/bin/etcd \\\n\
         \x20 --name {name} \\\n\
         \x20 --data-dir {ETCD_DATA_DIR} \\\n\
         \x20 --listen-client-urls http://0.0.0.0:{ETCD_CLIENT_PORT} \\\n\
         \x20 --advertise-client-urls {} \\\n\
         \x20 --listen-peer-urls http://0.0.0.0:{ETCD_PEER_PORT} \\\n\
         \x20 --initial-advertise-peer-urls {} \\\n\
         \x20 --initial-cluster {initial_cluster} \\\n\
         \x20 --initial-cluster-state {state}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         LimitNOFILE=65536\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        etcd_client_url(advertise_ip),
        etcd_peer_url(advertise_ip),
    )
}

/// Write the unit, reload systemd, enable+start it, and wait (systemd
/// itself does the waiting, via Type=notify) for etcd to report ready.
fn install_and_start_etcd(unit: &str) -> Result<()> {
    std::fs::create_dir_all(ETCD_DATA_DIR).context("failed to create etcd data dir")?;
    std::fs::write(ETCD_UNIT_PATH, unit).context("failed to write etcd systemd unit")?;

    let reload = Command::new("systemctl")
        .arg("daemon-reload")
        .status()
        .context("failed to run systemctl daemon-reload")?;
    if !reload.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    let enable = Command::new("systemctl")
        .args(["enable", "--now", ETCD_UNIT_NAME])
        .status()
        .context("failed to start monolith-etcd.service")?;
    if !enable.success() {
        anyhow::bail!("failed to start {ETCD_UNIT_NAME} — check `journalctl -u {ETCD_UNIT_NAME}`");
    }
    Ok(())
}

/// `etcdctl member add` on `via_ip`'s etcd (over SSH — same
/// connectivity assumption every other cluster command here already
/// makes) and parse the `ETCD_INITIAL_CLUSTER=...` / `ETCD_INITIAL_CLUSTER_STATE=...`
/// lines it prints. This is etcd's own documented dynamic-membership
/// workflow, not something reverse-engineered — verified against a
/// real etcd binary's actual output before relying on the format.
fn etcd_member_add(via_ip: &str, new_name: &str, new_ip: &str) -> Result<String> {
    let peer_url = etcd_peer_url(new_ip);
    let output = Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            via_ip,
            "etcdctl",
            &format!("--endpoints={}", etcd_client_url(via_ip)),
            "member",
            "add",
            new_name,
            &format!("--peer-urls={peer_url}"),
        ])
        .output()
        .with_context(|| format!("failed to ssh to {via_ip} to run etcdctl member add"))?;

    if !output.status.success() {
        anyhow::bail!(
            "etcdctl member add failed on {via_ip}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let initial_cluster = stdout
        .lines()
        .find_map(|l| l.strip_prefix("ETCD_INITIAL_CLUSTER="))
        .map(|s| s.trim_matches('"').to_string())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "etcdctl member add on {via_ip} didn't print ETCD_INITIAL_CLUSTER — unexpected etcdctl output:\n{stdout}"
            )
        })?;

    Ok(initial_cluster)
}

/// `etcdctl endpoint health` against every etcd client URL, run
/// locally (etcd's own client-to-cluster forwarding means this only
/// needs one reachable endpoint, but passing all of them lets a
/// completely dead node still get reported instead of silently
/// dropped from the picture).
fn etcd_endpoint_health(client_urls: &[String]) -> Vec<(String, bool)> {
    client_urls
        .iter()
        .map(|url| {
            let ok = Command::new("etcdctl")
                .args(["--endpoints", url, "endpoint", "health"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            (url.clone(), ok)
        })
        .collect()
}

/// Every etcd client URL for the local cluster, self + peers, read
/// from cluster.toml's `advertise_ip`/`peer_ips` — see
/// `read_all_member_ips`.
fn cluster_member_client_urls() -> Vec<String> {
    read_all_member_ips()
        .iter()
        .map(|ip| etcd_client_url(ip))
        .collect()
}

/// All member IPs this node knows about: itself (from `advertise_ip`
/// on a master or detected locally on a worker) plus every
/// `master_ip`/peer entry already read by [`read_peer_nodes`]. Used
/// for etcd endpoint health checks, not SSH reachability — a
/// different question (an unreachable-over-SSH node can still be a
/// perfectly healthy etcd member if only the SSH port is firewalled).
fn read_all_member_ips() -> Vec<String> {
    let mut ips: Vec<String> = Vec::new();
    if let Ok(content) = std::fs::read_to_string(CLUSTER_CONFIG_PATH) {
        for line in content.lines() {
            if line.starts_with("advertise_ip") || line.starts_with("master_ip") {
                if let Some(ip) = line.split('"').nth(1) {
                    ips.push(ip.to_string());
                }
            }
        }
    }
    ips.extend(read_peer_nodes());
    ips.sort();
    ips.dedup();
    ips
}

/// Real quorum status: healthy member count vs. total, and whether
/// that's a majority. `(healthy, total, has_quorum)`.
fn quorum_health() -> (usize, usize, bool) {
    let urls = cluster_member_client_urls();
    if urls.is_empty() {
        return (0, 0, false);
    }
    let results = etcd_endpoint_health(&urls);
    let healthy = results.iter().filter(|(_, ok)| *ok).count();
    let total = results.len();
    (healthy, total, healthy * 2 > total)
}

fn cluster_quorum() -> Result<()> {
    if !std::path::Path::new(CLUSTER_CONFIG_PATH).exists() {
        println!("{}", "Not in a cluster.".yellow());
        return Ok(());
    }
    let (healthy, total, has_quorum) = quorum_health();
    println!("{}", "Cluster Quorum:".bold().underline());
    for (url, ok) in etcd_endpoint_health(&cluster_member_client_urls()) {
        let mark = if ok { "●".green() } else { "●".red() };
        let state = if ok {
            "healthy".green()
        } else {
            "unreachable".red()
        };
        println!("  {mark} {url:<28} {state}");
    }
    println!();
    if has_quorum {
        println!(
            "{} Quorum OK — {healthy}/{total} members healthy",
            "●".green().bold()
        );
    } else {
        println!(
            "{} QUORUM LOST — only {healthy}/{total} members healthy, need a majority",
            "●".red().bold()
        );
    }
    Ok(())
}

fn cluster_init(name: Option<&str>, advertise_ip: Option<&str>) -> Result<()> {
    let cluster_name = name.unwrap_or("monolith-cluster");

    let ip = match advertise_ip {
        Some(ip) => ip.to_string(),
        None => detect_local_ip().context("failed to detect IP")?,
    };

    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "node".to_string());

    std::fs::create_dir_all(CLUSTER_CONFIG_DIR)
        .context("failed to create cluster config directory")?;

    let token = generate_token();

    let config = format!(
        "[cluster]\n\
         name = \"{cluster_name}\"\n\
         role = \"master\"\n\
         advertise_ip = \"{ip}\"\n\
         node_name = \"{hostname}\"\n\
         token = \"{token}\"\n\
         \n\
         [etcd]\n\
         data_dir = \"{ETCD_DATA_DIR}\"\n\
         listen_client_urls = \"{}\"\n\
         advertise_client_urls = \"{}\"\n",
        etcd_client_url(&ip),
        etcd_client_url(&ip),
    );

    std::fs::write(CLUSTER_CONFIG_PATH, &config).context("failed to write cluster config")?;

    println!("{} Bootstrapping etcd (Raft quorum layer)...", "→".blue());
    let initial_cluster = format!("{hostname}={}", etcd_peer_url(&ip));
    let unit = etcd_unit_content(&hostname, &ip, &initial_cluster, "new");
    install_and_start_etcd(&unit)?;

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
    std::fs::create_dir_all(CLUSTER_CONFIG_DIR)?;

    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "node".to_string());
    let my_ip = detect_local_ip().context("failed to detect this node's IP")?;

    let config = format!(
        "[cluster]\n\
         role = \"worker\"\n\
         master_ip = \"{master_ip}\"\n\
         advertise_ip = \"{my_ip}\"\n\
         token = \"{token}\"\n\
         node_name = \"{hostname}\"\n\
         \n\
         [etcd]\n\
         data_dir = \"{ETCD_DATA_DIR}\"\n\
         listen_client_urls = \"{}\"\n\
         advertise_client_urls = \"{}\"\n",
        etcd_client_url(&my_ip),
        etcd_client_url(&my_ip),
    );

    std::fs::write(CLUSTER_CONFIG_PATH, &config).context("failed to write cluster config")?;

    println!(
        "{} Registering with etcd cluster via {}...",
        "→".blue(),
        master_ip
    );
    // etcd's own dynamic-membership workflow: ask an existing member to
    // add us, which returns the full initial-cluster string (existing
    // members + us) we need to bootstrap with --initial-cluster-state
    // existing. Trying to guess that string ourselves instead of using
    // what member add actually returns is exactly the kind of "should
    // work" assumption that's bitten this project before (BORE/BBR
    // Kconfig, PCI silently missing) — use the real output.
    let initial_cluster = etcd_member_add(master_ip, &hostname, &my_ip)?;

    println!("{} Bootstrapping local etcd member...", "→".blue());
    let unit = etcd_unit_content(&hostname, &my_ip, &initial_cluster, "existing");
    install_and_start_etcd(&unit)?;

    println!("{} Joined cluster at {}", "●".green(), master_ip.bold());
    println!("  This node:  {} ({})", hostname.bold(), my_ip);
    println!("  Members:    {initial_cluster}");
    Ok(())
}

/// This node's own etcd member ID, in the hex form `member remove`
/// expects — matched by name against our own hostname. `member list`
/// prints IDs as decimal in JSON but hex everywhere else (verified
/// against a real cluster: JSON's `"ID":13668033151171901709` is the
/// exact same member as table view's `bdae9bbc11dd390d` —
/// `format!("{:x}", id)` of the same number), so this converts rather
/// than trying to scrape the hex out of table output.
fn etcd_own_member_id(client_url: &str, hostname: &str) -> Result<String> {
    let output = Command::new("etcdctl")
        .args(["--endpoints", client_url, "member", "list", "-w", "json"])
        .output()
        .context("failed to run etcdctl member list")?;
    if !output.status.success() {
        anyhow::bail!(
            "etcdctl member list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse etcdctl JSON output")?;
    let members = parsed["members"]
        .as_array()
        .context("unexpected etcdctl member list JSON shape")?;
    for member in members {
        if member["name"].as_str() == Some(hostname) {
            let id = member["ID"]
                .as_u64()
                .context("member entry missing numeric ID")?;
            return Ok(format!("{id:x}"));
        }
    }
    anyhow::bail!("no etcd member named '{hostname}' found in the cluster")
}

/// This node's own `advertise_ip` as written to cluster.toml by
/// `init`/`join` — NOT a fresh `detect_local_ip()` call. Those can
/// legitimately disagree (e.g. `cluster init --advertise-ip <explicit>`
/// picks something other than what auto-detection would find), and
/// etcd was bootstrapped against whatever IP actually got written to
/// the unit file, not whatever `detect_local_ip()` happens to return
/// later. Found by actually running `cluster leave` end to end: it
/// re-detected 192.168.8.8 when the cluster had been explicitly
/// bootstrapped on 127.0.0.1, and only worked by accident because this
/// etcd listens on 0.0.0.0.
fn read_own_advertise_ip() -> Result<String> {
    let content =
        std::fs::read_to_string(CLUSTER_CONFIG_PATH).context("failed to read cluster config")?;
    content
        .lines()
        .find(|l| l.starts_with("advertise_ip"))
        .and_then(|l| l.split('"').nth(1))
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("cluster.toml has no advertise_ip"))
}

fn cluster_leave() -> Result<()> {
    if !std::path::Path::new(CLUSTER_CONFIG_PATH).exists() {
        println!("{}", "Not in a cluster.".yellow());
        return Ok(());
    }

    let hostname = nix::unistd::gethostname()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "node".to_string());

    // Best-effort: deregister from etcd before tearing down the local
    // unit. If this fails (etcd already down, we're the last member,
    // network partition) don't block leaving — a config file pointing
    // at a cluster this node no longer participates in is worse than
    // a stale etcd member entry an operator can clean up with
    // `etcdctl member remove` by hand.
    if let Ok(my_ip) = read_own_advertise_ip() {
        let client_url = etcd_client_url(&my_ip);
        match etcd_own_member_id(&client_url, &hostname) {
            Ok(id) => {
                let status = Command::new("etcdctl")
                    .args(["--endpoints", &client_url, "member", "remove", &id])
                    .status();
                match status {
                    Ok(s) if s.success() => {
                        println!("{} Removed from etcd membership ({id})", "●".green())
                    }
                    _ => println!(
                        "{} Couldn't remove etcd membership cleanly — you may need `etcdctl member remove {id}` on a remaining node",
                        "⚠".yellow()
                    ),
                }
            }
            Err(e) => println!(
                "{} Couldn't look up etcd membership before leaving: {e}",
                "⚠".yellow()
            ),
        }
    }

    let _ = Command::new("systemctl")
        .args(["disable", "--now", ETCD_UNIT_NAME])
        .status();
    let _ = std::fs::remove_file(ETCD_UNIT_PATH);
    let _ = std::fs::remove_dir_all(ETCD_DATA_DIR);
    let _ = Command::new("systemctl").arg("daemon-reload").status();

    std::fs::remove_file(CLUSTER_CONFIG_PATH).context("failed to remove cluster config")?;
    println!("{} Left cluster", "●".green());
    Ok(())
}

fn cluster_nodes() -> Result<()> {
    let config_path = CLUSTER_CONFIG_PATH;
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
    if !std::path::Path::new(CLUSTER_CONFIG_PATH).exists() {
        println!("{}", "Not in a cluster.".yellow());
        return Ok(());
    }

    let content =
        std::fs::read_to_string(CLUSTER_CONFIG_PATH).context("failed to read cluster config")?;

    println!("{}", "Cluster Status:".bold().underline());
    println!("{content}");

    let (healthy, total, has_quorum) = quorum_health();
    if total > 0 {
        println!("{}", "Quorum:".bold().underline());
        if has_quorum {
            println!("  {} {healthy}/{total} etcd members healthy", "●".green());
        } else {
            println!(
                "  {} QUORUM LOST — {healthy}/{total} etcd members healthy",
                "●".red().bold()
            );
        }
        println!("  (see `mnctl cluster quorum` for per-member detail)");
    }
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

    // Refuse to start a rollout without a healthy Raft majority. A
    // rolling update that restarts services chunk-by-chunk while the
    // cluster can't even agree on its own membership is exactly the
    // scenario that turns "one bad node" into "split-brain" — check
    // before touching anything, not after something goes wrong.
    let (healthy, total, has_quorum) = quorum_health();
    if total > 0 && !has_quorum {
        anyhow::bail!(
            "refusing to start rolling update: quorum lost ({healthy}/{total} etcd members healthy). \
             Fix the cluster first — see `mnctl cluster quorum`."
        );
    }
    if total > 0 {
        println!(
            "{} Quorum OK ({healthy}/{total} members) — proceeding",
            "●".green()
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
    let config_path = CLUSTER_CONFIG_PATH;
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
    let config_path = CLUSTER_CONFIG_PATH;
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
