//! Kubernetes (k3s) integration for Monolith OS.
//!
//! These subcommands wrap the upstream `k3s` installer and the resulting
//! `kubectl` binary so that operators can stand up a single-node cluster or a
//! multi-node cluster with a few `mnctl kube` calls.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

const KUBECONFIG_PATH: &str = "/etc/rancher/k3s/k3s.yaml";

#[derive(Args)]
pub struct KubeArgs {
    #[command(subcommand)]
    command: KubeCommand,
}

#[derive(Subcommand)]
enum KubeCommand {
    /// Install k3s on this node (server or agent)
    Install {
        /// Node role: server (default) or agent
        #[arg(long, default_value = "server")]
        role: String,
        /// k3s server URL (required for agents, e.g. https://master:6443)
        #[arg(long)]
        server_url: Option<String>,
        /// Cluster join token (required for agents)
        #[arg(long)]
        token: Option<String>,
        /// Disable Traefik (Monolith ships its own nginx/proxy). Pass
        /// `--disable-traefik=false` to keep Traefik as the ingress.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        disable_traefik: bool,
        /// Disable ServiceLB so external LBs (MetalLB, etc.) can take over
        #[arg(long)]
        disable_servicelb: bool,
        /// Pin a specific k3s channel (e.g. v1.30, latest, stable)
        #[arg(long, default_value = "stable")]
        channel: String,
    },
    /// Uninstall k3s from this node
    Uninstall {
        /// Force uninstall server (default uninstalls whichever is installed)
        #[arg(long)]
        agent: bool,
    },
    /// Print cluster status (`kubectl cluster-info`)
    Status,
    /// List nodes
    Nodes,
    /// List pods (across namespaces by default)
    Pods {
        /// Namespace (default: all)
        #[arg(long, default_value = "")]
        namespace: String,
    },
    /// Apply a manifest from a file or URL
    Apply {
        /// File path or URL of the manifest
        manifest: String,
    },
    /// Show the join token (server-only)
    Token,
    /// Print the kubeconfig path or contents
    Kubeconfig {
        /// Print contents to stdout instead of the path
        #[arg(long)]
        cat: bool,
    },
    /// Pass-through to kubectl (any extra args are forwarded)
    Kubectl {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

impl KubeArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            KubeCommand::Install {
                role,
                server_url,
                token,
                disable_traefik,
                disable_servicelb,
                channel,
            } => install(
                &role,
                server_url.as_deref(),
                token.as_deref(),
                disable_traefik,
                disable_servicelb,
                &channel,
            ),
            KubeCommand::Uninstall { agent } => uninstall(agent),
            KubeCommand::Status => kubectl(&["cluster-info"]),
            KubeCommand::Nodes => kubectl(&["get", "nodes", "-o", "wide"]),
            KubeCommand::Pods { namespace } => {
                if namespace.is_empty() {
                    kubectl(&["get", "pods", "-A", "-o", "wide"])
                } else {
                    kubectl(&["get", "pods", "-n", &namespace, "-o", "wide"])
                }
            }
            KubeCommand::Apply { manifest } => kubectl(&["apply", "-f", &manifest]),
            KubeCommand::Token => show_token(),
            KubeCommand::Kubeconfig { cat } => kubeconfig(cat),
            KubeCommand::Kubectl { args } => {
                let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                kubectl(&str_args)
            }
        }
    }
}

fn install(
    role: &str,
    server_url: Option<&str>,
    token: Option<&str>,
    disable_traefik: bool,
    disable_servicelb: bool,
    channel: &str,
) -> Result<()> {
    if !matches!(role, "server" | "agent") {
        anyhow::bail!("role must be 'server' or 'agent'");
    }

    let mut env_pairs: Vec<(String, String)> =
        vec![("INSTALL_K3S_CHANNEL".to_string(), channel.to_string())];

    let mut exec_args: Vec<String> = Vec::new();
    if role == "server" {
        if disable_traefik {
            exec_args.push("--disable=traefik".to_string());
        }
        if disable_servicelb {
            exec_args.push("--disable=servicelb".to_string());
        }
        // Make the kubeconfig readable by the wheel/sudo group so non-root
        // operators can use kubectl after running `chgrp wheel`.
        exec_args.push("--write-kubeconfig-mode=644".to_string());
    } else {
        let url = server_url
            .ok_or_else(|| anyhow::anyhow!("--server-url is required when --role agent"))?;
        let tok = token.ok_or_else(|| anyhow::anyhow!("--token is required when --role agent"))?;
        env_pairs.push(("K3S_URL".to_string(), url.to_string()));
        env_pairs.push(("K3S_TOKEN".to_string(), tok.to_string()));
    }

    if !exec_args.is_empty() {
        env_pairs.push(("INSTALL_K3S_EXEC".to_string(), exec_args.join(" ")));
    }

    println!(
        "{} Installing k3s ({}) channel={}...",
        "→".blue(),
        role.bold(),
        channel.bold()
    );

    if which::which("curl").is_err() {
        anyhow::bail!("curl is required to install k3s");
    }

    // Download the installer script first, then execute it directly
    // to avoid shell injection through sh -c with interpolated values.
    let installer = Command::new("curl")
        .args(["-sfL", "https://get.k3s.io"])
        .output()
        .context("failed to download k3s installer script")?;
    if !installer.status.success() {
        anyhow::bail!("failed to download k3s installer from https://get.k3s.io");
    }

    let mut cmd = Command::new("sh");
    cmd.args(["-s", "-", role]);
    cmd.stdin(std::process::Stdio::piped());
    for (k, v) in &env_pairs {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().context("failed to spawn k3s installer")?;
    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        let _ = stdin.write_all(&installer.stdout);
    }
    let status = child.wait().context("failed to wait for k3s installer")?;
    if !status.success() {
        anyhow::bail!("k3s installer exited with non-zero status");
    }

    if role == "server" {
        println!();
        println!("{} k3s server installed. Useful commands:", "●".green());
        println!("  kubectl --kubeconfig {KUBECONFIG_PATH} get nodes");
        println!("  mnctl kube nodes");
        println!("  mnctl kube token        # share with agents");
    } else {
        println!("{} k3s agent installed and joined", "●".green());
    }
    Ok(())
}

fn uninstall(agent: bool) -> Result<()> {
    let script = if agent {
        "/usr/local/bin/k3s-agent-uninstall.sh"
    } else {
        "/usr/local/bin/k3s-uninstall.sh"
    };
    if !Path::new(script).exists() {
        anyhow::bail!("uninstall script not found: {script}. Is k3s installed on this node?");
    }
    let status = Command::new(script)
        .status()
        .with_context(|| format!("failed to run {script}"))?;
    if !status.success() {
        anyhow::bail!("k3s uninstall exited non-zero");
    }
    println!("{} k3s uninstalled", "●".green());
    Ok(())
}

fn kubectl(args: &[&str]) -> Result<()> {
    let bin = if which::which("kubectl").is_ok() {
        "kubectl".to_string()
    } else if which::which("k3s").is_ok() {
        "k3s".to_string()
    } else {
        anyhow::bail!("kubectl/k3s not found. Install k3s with: mnctl kube install --role server");
    };

    let mut cmd = Command::new(&bin);
    if bin == "k3s" {
        cmd.arg("kubectl");
    }
    cmd.args(args);

    if Path::new(KUBECONFIG_PATH).exists() && std::env::var("KUBECONFIG").is_err() {
        cmd.env("KUBECONFIG", KUBECONFIG_PATH);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to invoke {bin}"))?;
    if !status.success() {
        anyhow::bail!("{bin} exited with status {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

fn show_token() -> Result<()> {
    let path = "/var/lib/rancher/k3s/server/node-token";
    if !Path::new(path).exists() {
        anyhow::bail!("token file not found at {path}; not a k3s server?");
    }
    let token = std::fs::read_to_string(path).context("failed to read k3s join token")?;
    println!("{}", "k3s Join Token:".bold().underline());
    println!("{}", token.trim().bold());
    println!();
    println!(
        "Add agents with: {} kube install --role agent --server-url https://<this-host>:6443 --token <token>",
        "mnctl".bold()
    );
    Ok(())
}

fn kubeconfig(cat: bool) -> Result<()> {
    if !Path::new(KUBECONFIG_PATH).exists() {
        anyhow::bail!("kubeconfig not found at {KUBECONFIG_PATH}");
    }
    if cat {
        let content =
            std::fs::read_to_string(KUBECONFIG_PATH).context("failed to read kubeconfig")?;
        print!("{content}");
    } else {
        println!("{KUBECONFIG_PATH}");
    }
    Ok(())
}
