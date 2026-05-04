use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct ContainerArgs {
    #[command(subcommand)]
    command: ContainerCommand,
}

#[derive(Subcommand)]
enum ContainerCommand {
    /// List containers
    List {
        /// Show all containers (including stopped)
        #[arg(short, long)]
        all: bool,
    },
    /// Start a container
    Start {
        /// Container name or ID
        name: String,
    },
    /// Stop a container
    Stop {
        /// Container name or ID
        name: String,
    },
    /// Restart a container
    Restart {
        /// Container name or ID
        name: String,
    },
    /// View container logs
    Logs {
        /// Container name or ID
        name: String,
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show from the end
        #[arg(short, long, default_value = "100")]
        tail: String,
    },
    /// Execute a command inside a container
    Exec {
        /// Container name or ID
        name: String,
        /// Command to execute
        command: Vec<String>,
    },
    /// Show container resource usage statistics
    Stats {
        /// Container name (optional, shows all if omitted)
        name: Option<String>,
    },
    /// Show full container details
    Inspect {
        /// Container name or ID
        name: String,
    },
    /// Pull a container image
    Pull {
        /// Image name and tag
        image: String,
    },
    /// List container images
    Images,
    /// Remove unused containers, images, and volumes
    Prune,
    /// Docker Compose operations
    Compose(ComposeArgs),
}

#[derive(Args)]
struct ComposeArgs {
    #[command(subcommand)]
    command: ComposeCommand,
}

#[derive(Subcommand)]
enum ComposeCommand {
    /// Start services defined in a compose file
    Up {
        /// Path to docker-compose.yml
        path: String,
    },
    /// Stop services defined in a compose file
    Down {
        /// Path to docker-compose.yml
        path: String,
    },
    /// View logs for services in a compose file
    Logs {
        /// Path to docker-compose.yml
        path: String,
    },
}

fn docker_cmd() -> String {
    if which::which("docker").is_ok() {
        "docker".to_string()
    } else if which::which("podman").is_ok() {
        "podman".to_string()
    } else {
        "docker".to_string()
    }
}

impl ContainerArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ContainerCommand::List { all } => list_containers(all),
            ContainerCommand::Start { name } => container_action("start", &name),
            ContainerCommand::Stop { name } => container_action("stop", &name),
            ContainerCommand::Restart { name } => container_action("restart", &name),
            ContainerCommand::Logs { name, follow, tail } => container_logs(&name, follow, &tail),
            ContainerCommand::Exec { name, command } => container_exec(&name, &command),
            ContainerCommand::Stats { name } => container_stats(name.as_deref()),
            ContainerCommand::Inspect { name } => container_inspect(&name),
            ContainerCommand::Pull { image } => pull_image(&image),
            ContainerCommand::Images => list_images(),
            ContainerCommand::Prune => prune_containers(),
            ContainerCommand::Compose(args) => match args.command {
                ComposeCommand::Up { path } => compose_action("up", &path),
                ComposeCommand::Down { path } => compose_action("down", &path),
                ComposeCommand::Logs { path } => compose_logs(&path),
            },
        }
    }
}

fn list_containers(all: bool) -> Result<()> {
    let mut args = vec![
        "ps",
        "--format",
        "table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}",
    ];
    if all {
        args.push("-a");
    }

    let output = Command::new(docker_cmd())
        .args(&args)
        .output()
        .context("failed to list containers — is Docker installed?")?;

    println!("{}", "Containers:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn container_action(action: &str, name: &str) -> Result<()> {
    let output = Command::new(docker_cmd())
        .args([action, name])
        .output()
        .with_context(|| format!("failed to {action} container {name}"))?;

    if output.status.success() {
        println!(
            "{} Container {} {}",
            "●".green(),
            name.bold(),
            format!("{action}ed").green()
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to {action} container {name}: {stderr}");
    }
    Ok(())
}

fn container_logs(name: &str, follow: bool, tail: &str) -> Result<()> {
    let mut args = vec!["logs", "--tail", tail, name];
    if follow {
        args.insert(1, "-f");
    }

    let output = Command::new(docker_cmd())
        .args(&args)
        .output()
        .with_context(|| format!("failed to get logs for container {name}"))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    print!("{}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

fn container_exec(name: &str, command: &[String]) -> Result<()> {
    let mut args = vec!["exec", "-it", name];
    let cmd_refs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
    args.extend(cmd_refs);

    let status = Command::new(docker_cmd())
        .args(&args)
        .status()
        .with_context(|| format!("failed to exec in container {name}"))?;

    if !status.success() {
        anyhow::bail!("command exited with status {status}");
    }
    Ok(())
}

fn container_stats(name: Option<&str>) -> Result<()> {
    let mut args = vec!["stats", "--no-stream"];
    if let Some(n) = name {
        args.push(n);
    }

    let output = Command::new(docker_cmd())
        .args(&args)
        .output()
        .context("failed to get container stats")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn container_inspect(name: &str) -> Result<()> {
    let output = Command::new(docker_cmd())
        .args(["inspect", name])
        .output()
        .with_context(|| format!("failed to inspect container {name}"))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn pull_image(image: &str) -> Result<()> {
    println!("{} Pulling image {}...", "→".blue(), image.bold());
    let status = Command::new(docker_cmd())
        .args(["pull", image])
        .status()
        .with_context(|| format!("failed to pull image {image}"))?;

    if status.success() {
        println!("{} Image {} pulled successfully", "●".green(), image.bold());
    } else {
        anyhow::bail!("failed to pull image {image}");
    }
    Ok(())
}

fn list_images() -> Result<()> {
    let output = Command::new(docker_cmd())
        .args([
            "images",
            "--format",
            "table {{.Repository}}\t{{.Tag}}\t{{.Size}}\t{{.CreatedSince}}",
        ])
        .output()
        .context("failed to list images")?;

    println!("{}", "Images:".bold().underline());
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn prune_containers() -> Result<()> {
    println!(
        "{}",
        "Pruning unused containers, images, and volumes...".dimmed()
    );

    for resource in &["container", "image", "volume"] {
        let output = Command::new(docker_cmd())
            .args([resource, "prune", "-f"])
            .output()
            .with_context(|| format!("failed to prune {resource}s"))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.trim().is_empty() {
                println!("  {} {}: {}", "●".green(), resource, stdout.trim());
            }
        }
    }
    println!("{} Prune complete.", "●".green());
    Ok(())
}

fn compose_action(action: &str, path: &str) -> Result<()> {
    let mut args = vec!["-f", path];
    match action {
        "up" => args.extend(["up", "-d"]),
        "down" => args.push("down"),
        _ => args.push(action),
    }

    let status = Command::new("docker")
        .arg("compose")
        .args(&args)
        .status()
        .with_context(|| format!("failed to run compose {action} for {path}"))?;

    if status.success() {
        println!(
            "{} Compose {} completed for {}",
            "●".green(),
            action,
            path.bold()
        );
    } else {
        anyhow::bail!("compose {action} failed for {path}");
    }
    Ok(())
}

fn compose_logs(path: &str) -> Result<()> {
    let status = Command::new("docker")
        .args(["compose", "-f", path, "logs", "-f"])
        .status()
        .with_context(|| format!("failed to get compose logs for {path}"))?;

    if !status.success() {
        anyhow::bail!("compose logs failed for {path}");
    }
    Ok(())
}
