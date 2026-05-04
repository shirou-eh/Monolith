use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct VpnArgs {
    #[command(subcommand)]
    command: VpnCommand,
}

#[derive(Subcommand)]
enum VpnCommand {
    /// Create a new WireGuard tunnel
    Create {
        /// Tunnel name
        name: String,
    },
    /// List VPN tunnels
    List,
    /// Connect a VPN tunnel
    Connect {
        /// Tunnel name
        name: String,
    },
    /// Disconnect a VPN tunnel
    Disconnect {
        /// Tunnel name
        name: String,
    },
    /// Show VPN status and statistics
    Status {
        /// Tunnel name (optional)
        name: Option<String>,
    },
    /// Manage peers
    Peer(PeerArgs),
}

#[derive(Args)]
struct PeerArgs {
    #[command(subcommand)]
    command: PeerCommand,
}

#[derive(Subcommand)]
enum PeerCommand {
    /// Add a peer to a tunnel
    Add {
        /// Tunnel name
        tunnel: String,
        /// Peer public key
        pubkey: String,
    },
    /// Remove a peer from a tunnel
    Remove {
        /// Tunnel name
        tunnel: String,
        /// Peer public key
        peer: String,
    },
}

impl VpnArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            VpnCommand::Create { name } => create_tunnel(&name),
            VpnCommand::List => list_tunnels(),
            VpnCommand::Connect { name } => connect_tunnel(&name),
            VpnCommand::Disconnect { name } => disconnect_tunnel(&name),
            VpnCommand::Status { name } => tunnel_status(name.as_deref()),
            VpnCommand::Peer(args) => match args.command {
                PeerCommand::Add { tunnel, pubkey } => add_peer(&tunnel, &pubkey),
                PeerCommand::Remove { tunnel, peer } => remove_peer(&tunnel, &peer),
            },
        }
    }
}

fn create_tunnel(name: &str) -> Result<()> {
    let config_dir = "/etc/wireguard";
    std::fs::create_dir_all(config_dir).context("failed to create /etc/wireguard")?;

    // Generate key pair
    let privkey_output = Command::new("wg")
        .arg("genkey")
        .output()
        .context("failed to generate WireGuard private key — is wireguard-tools installed?")?;

    let privkey = String::from_utf8_lossy(&privkey_output.stdout)
        .trim()
        .to_string();

    let pubkey_output = Command::new("wg")
        .arg("pubkey")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(privkey.as_bytes())?;
            child.wait_with_output()
        })
        .context("failed to derive public key")?;

    let pubkey = String::from_utf8_lossy(&pubkey_output.stdout)
        .trim()
        .to_string();

    let config = format!(
        "[Interface]\n\
         PrivateKey = {privkey}\n\
         Address = 10.0.0.1/24\n\
         ListenPort = 51820\n\
         \n\
         # Add peers below with: mnctl vpn peer add {name} <pubkey>\n"
    );

    let config_path = format!("{config_dir}/{name}.conf");
    std::fs::write(&config_path, &config)
        .with_context(|| format!("failed to write {config_path}"))?;

    // Restrict permissions
    Command::new("chmod")
        .args(["600", &config_path])
        .status()
        .context("failed to set permissions on config")?;

    println!("{} WireGuard tunnel '{}' created", "●".green(), name.bold());
    println!("  Public key: {}", pubkey.bold());
    println!("  Config: {config_path}");
    println!("  Connect with: {} vpn connect {}", "mnctl".bold(), name);
    Ok(())
}

fn list_tunnels() -> Result<()> {
    let config_dir = "/etc/wireguard";
    let path = std::path::Path::new(config_dir);

    if !path.exists() {
        println!("{}", "No WireGuard tunnels configured.".dimmed());
        return Ok(());
    }

    println!("{}", "WireGuard Tunnels:".bold().underline());
    for entry in std::fs::read_dir(path).context("failed to read /etc/wireguard")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".conf") {
            let tunnel_name = name.trim_end_matches(".conf");

            let wg_show = Command::new("wg").args(["show", tunnel_name]).output();

            let status = match wg_show {
                Ok(o) if o.status.success() => "active".green(),
                _ => "inactive".dimmed(),
            };

            println!("  {} {:<20} {}", "●".green(), tunnel_name, status);
        }
    }
    Ok(())
}

fn connect_tunnel(name: &str) -> Result<()> {
    let status = Command::new("wg-quick")
        .args(["up", name])
        .status()
        .with_context(|| format!("failed to connect tunnel {name}"))?;

    if status.success() {
        println!("{} Tunnel '{}' connected", "●".green(), name.bold());
    } else {
        anyhow::bail!("failed to connect tunnel {name}");
    }
    Ok(())
}

fn disconnect_tunnel(name: &str) -> Result<()> {
    let status = Command::new("wg-quick")
        .args(["down", name])
        .status()
        .with_context(|| format!("failed to disconnect tunnel {name}"))?;

    if status.success() {
        println!("{} Tunnel '{}' disconnected", "●".green(), name.bold());
    } else {
        anyhow::bail!("failed to disconnect tunnel {name}");
    }
    Ok(())
}

fn tunnel_status(name: Option<&str>) -> Result<()> {
    let args = match name {
        Some(n) => vec!["show", n],
        None => vec!["show"],
    };

    let output = Command::new("wg")
        .args(&args)
        .output()
        .context("failed to get WireGuard status")?;

    if output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        println!("{}", "No active WireGuard tunnels.".dimmed());
    }
    Ok(())
}

fn add_peer(tunnel: &str, pubkey: &str) -> Result<()> {
    let config_path = format!("/etc/wireguard/{tunnel}.conf");

    let peer_entry = format!(
        "\n[Peer]\n\
         PublicKey = {pubkey}\n\
         AllowedIPs = 10.0.0.0/24\n"
    );

    let mut config = std::fs::read_to_string(&config_path)
        .with_context(|| format!("tunnel {tunnel} not found"))?;
    config.push_str(&peer_entry);
    std::fs::write(&config_path, &config).context("failed to update config")?;

    // Sync live if tunnel is active
    let _ = Command::new("wg")
        .args(["set", tunnel, "peer", pubkey, "allowed-ips", "10.0.0.0/24"])
        .status();

    println!("{} Peer added to tunnel '{}'", "●".green(), tunnel.bold());
    Ok(())
}

fn remove_peer(tunnel: &str, peer: &str) -> Result<()> {
    let status = Command::new("wg")
        .args(["set", tunnel, "peer", peer, "remove"])
        .status()
        .with_context(|| format!("failed to remove peer from {tunnel}"))?;

    if status.success() {
        println!(
            "{} Peer removed from tunnel '{}'",
            "●".green(),
            tunnel.bold()
        );
    }
    Ok(())
}
