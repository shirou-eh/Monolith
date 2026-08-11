//! Kubernetes (k3s) integration for Monolith OS.
//!
//! These subcommands wrap the upstream `k3s` installer and the resulting
//! `kubectl` binary so that operators can stand up a single-node cluster or a
//! multi-node cluster with a few `mnctl kube` calls.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::os::unix::process::CommandExt;
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
    /// Autoscale a Deployment — the per-template autoscaling hook: point
    /// it at whatever Deployment a template (`mnctl template deploy`) or
    /// `mnctl kube apply` already created. Needs metrics-server running
    /// in-cluster for the resulting HPA to actually do anything.
    Autoscale {
        /// Deployment name to autoscale
        deployment: String,
        /// Namespace
        #[arg(long, default_value = "default")]
        namespace: String,
        /// Minimum replicas
        #[arg(long, default_value_t = 1)]
        min: u32,
        /// Maximum replicas
        #[arg(long, default_value_t = 5)]
        max: u32,
        /// Target average CPU utilization percent that triggers scaling
        #[arg(long, default_value_t = 70)]
        cpu_target: u32,
        /// Print the manifest instead of applying it
        #[arg(long)]
        dry_run: bool,
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
                if args.is_empty() {
                    anyhow::bail!("no kubectl arguments provided");
                }
                let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                kubectl_execvp(&args_ref)
            }
            KubeCommand::Autoscale { deployment, namespace, min, max, cpu_target, dry_run } => {
                autoscale(&deployment, &namespace, min, max, cpu_target, dry_run)
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
        // FIX (1.0.2): mode 0644 (world-readable) leaked cluster credentials
        // to all local users. 0640 restricts access to the owning group.
        // We follow up with `chown root:wheel` so members of the wheel/sudo
        // group can use kubectl without sudo.
        exec_args.push("--write-kubeconfig-mode=640".to_string());
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
        // Tighten ownership so only root:wheel can read the kubeconfig.
        let _ = Command::new("chown")
            .args(["root:wheel", KUBECONFIG_PATH])
            .status();
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

/// Pass-through to kubectl using execvp so args with spaces are forwarded
/// verbatim instead of being interpolated through a shell string.
fn kubectl_execvp(args: &[&str]) -> Result<()> {
    let kubectl_bin = which::which("kubectl").ok();
    let k3s_bin = which::which("k3s").ok();

    let (bin, extra_args): (String, Vec<&str>) = match (kubectl_bin, k3s_bin) {
        (Some(k), _) => (k.to_string_lossy().to_string(), vec![]),
        (None, Some(k3)) => (k3.to_string_lossy().to_string(), vec!["kubectl"]),
        (None, None) => {
            anyhow::bail!("kubectl/k3s not found. Install k3s with: mnctl kube install --role server");
        }
    };

    let mut all_args: Vec<&str> = extra_args.iter().copied().collect();
    all_args.extend_from_slice(args);

    if Path::new(KUBECONFIG_PATH).exists() && std::env::var("KUBECONFIG").is_err() {
        std::env::set_var("KUBECONFIG", KUBECONFIG_PATH);
    }

    let err = Command::new(&bin).args(&all_args).exec();
    Err(anyhow::anyhow!("failed to exec {bin}: {err}"))
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

/// Build and apply (or print) a `autoscaling/v2` HorizontalPodAutoscaler
/// for one Deployment. Kept to the one metric that's universally
/// available without extra setup (CPU) rather than reaching for custom
/// metrics, which would need Prometheus adapter wiring most Monolith
/// installs won't have.
fn autoscale(deployment: &str, namespace: &str, min: u32, max: u32, cpu_target: u32, dry_run: bool) -> Result<()> {
    if min == 0 || max < min {
        anyhow::bail!("--max must be >= --min, and --min must be at least 1");
    }

    let manifest = format!(
        "apiVersion: autoscaling/v2\n\
         kind: HorizontalPodAutoscaler\n\
         metadata:\n\
         \x20 name: {deployment}-hpa\n\
         \x20 namespace: {namespace}\n\
         \x20 labels:\n\
         \x20   managed-by: mnctl\n\
         spec:\n\
         \x20 scaleTargetRef:\n\
         \x20   apiVersion: apps/v1\n\
         \x20   kind: Deployment\n\
         \x20   name: {deployment}\n\
         \x20 minReplicas: {min}\n\
         \x20 maxReplicas: {max}\n\
         \x20 metrics:\n\
         \x20   - type: Resource\n\
         \x20     resource:\n\
         \x20       name: cpu\n\
         \x20       target:\n\
         \x20         type: Utilization\n\
         \x20         averageUtilization: {cpu_target}\n"
    );

    if dry_run {
        print!("{manifest}");
        return Ok(());
    }

    println!(
        "{} Autoscaling {} in {}: {min}-{max} replicas, target {cpu_target}% CPU",
        "→".blue(),
        deployment.bold(),
        namespace
    );

    let bin = if which::which("kubectl").is_ok() { "kubectl" } else { "k3s" };
    let mut cmd = Command::new(bin);
    if bin == "k3s" {
        cmd.arg("kubectl");
    }
    cmd.args(["apply", "-f", "-"]);
    if Path::new(KUBECONFIG_PATH).exists() && std::env::var("KUBECONFIG").is_err() {
        cmd.env("KUBECONFIG", KUBECONFIG_PATH);
    }
    cmd.stdin(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("failed to invoke kubectl")?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin.write_all(manifest.as_bytes())?;
    }
    let status = child.wait().context("failed to wait for kubectl apply")?;

    if !status.success() {
        anyhow::bail!("kubectl apply failed for {deployment}-hpa");
    }

    println!("{} {}-hpa applied", "●".green(), deployment);
    println!(
        "  {} needs metrics-server running in-cluster to actually scale — check with: mnctl kube kubectl top pods",
        "→".blue()
    );
    Ok(())
}
