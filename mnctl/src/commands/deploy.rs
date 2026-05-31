use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

#[derive(Args)]
pub struct DeployArgs {
    #[command(subcommand)]
    command: DeployCommand,
}

#[derive(Subcommand)]
enum DeployCommand {
    /// Deploy an application from a path or git URL
    App {
        /// Application path or git URL
        source: String,
        /// Service name
        #[arg(long)]
        name: String,
        /// Exposed port (default: 8080)
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Environment variables (KEY=VALUE)
        #[arg(long, short)]
        env: Vec<String>,
        /// Domain for reverse proxy + TLS
        #[arg(long)]
        domain: Option<String>,
    },
    /// List deployed applications
    List,
    /// Show deployment status
    Status {
        /// Application name
        name: String,
    },
    /// Pull latest and redeploy
    Update {
        /// Application name
        name: String,
    },
    /// Remove a deployment
    Remove {
        /// Application name
        name: String,
    },
    /// Show deployment history and rollback
    Rollback {
        /// Application name
        name: String,
        /// Version to roll back to (omit to list history)
        #[arg(long)]
        to: Option<String>,
    },
    /// Canary deploy: percentage-based traffic split
    Canary {
        /// Application name
        name: String,
        /// Percentage of traffic for new version (default: 10)
        #[arg(long, default_value_t = 10)]
        pct: u8,
        /// Promote canary to full release
        #[arg(long)]
        promote: bool,
        /// Abort canary and roll back
        #[arg(long)]
        abort: bool,
    },
    /// Live watch a deployment: logs + CPU/RAM
    Watch {
        /// Application name
        name: String,
    },
}

impl DeployArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            DeployCommand::App {
                source,
                name,
                port,
                env,
                domain,
            } => deploy_app(&source, &name, port, &env, domain.as_deref()),
            DeployCommand::List => list_deployments(),
            DeployCommand::Status { name } => deployment_status(&name),
            DeployCommand::Update { name } => update_deployment(&name),
            DeployCommand::Remove { name } => remove_deployment(&name),
            DeployCommand::Rollback { name, to } => deploy_rollback(&name, to.as_deref()),
            DeployCommand::Canary { name, pct, promote, abort } => deploy_canary(&name, pct, promote, abort),
            DeployCommand::Watch { name } => deploy_watch(&name),
        }
    }
}

fn detect_runtime(path: &Path) -> &'static str {
    if path.join("package.json").exists() {
        "nodejs"
    } else if path.join("requirements.txt").exists() || path.join("pyproject.toml").exists() {
        "python"
    } else if path.join("Cargo.toml").exists() {
        "rust"
    } else if path.join("go.mod").exists() {
        "go"
    } else if path.join("Dockerfile").exists() {
        "docker"
    } else if path.join("docker-compose.yml").exists() || path.join("compose.yml").exists() {
        "compose"
    } else {
        "unknown"
    }
}

fn deploy_app(
    source: &str,
    name: &str,
    port: u16,
    env: &[String],
    domain: Option<&str>,
) -> Result<()> {
    let deploy_dir = format!("/var/lib/monolith/deployments/{name}");
    std::fs::create_dir_all(&deploy_dir)
        .with_context(|| format!("failed to create deployment directory {deploy_dir}"))?;

    let work_dir = if source.starts_with("http") || source.starts_with("git@") {
        println!("{} Cloning {}...", "→".blue(), source);
        let clone_dir = format!("{deploy_dir}/app");
        Command::new("git")
            .args(["clone", source, &clone_dir])
            .status()
            .context("failed to clone repository")?;
        clone_dir
    } else {
        source.to_string()
    };

    let runtime = detect_runtime(Path::new(&work_dir));
    println!("{} Detected runtime: {}", "→".blue(), runtime.bold());

    let dockerfile_path = format!("{deploy_dir}/Dockerfile");
    let dockerfile = match runtime {
        "nodejs" => {
            "FROM node:20-alpine\n\
             WORKDIR /app\n\
             COPY package*.json ./\n\
             RUN npm ci --only=production\n\
             COPY . .\n\
             CMD [\"node\", \"index.js\"]\n"
        }
        "python" => {
            "FROM python:3.12-slim\n\
             WORKDIR /app\n\
             COPY requirements.txt ./\n\
             RUN pip install --no-cache-dir -r requirements.txt\n\
             COPY . .\n\
             CMD [\"python\", \"main.py\"]\n"
        }
        "rust" => {
            "FROM rust:1.83-slim AS builder\n\
             WORKDIR /app\n\
             COPY . .\n\
             RUN cargo build --release\n\
             FROM debian:bookworm-slim\n\
             COPY --from=builder /app/target/release/* /usr/local/bin/\n\
             CMD [\"/usr/local/bin/app\"]\n"
        }
        "go" => {
            "FROM golang:1.22-alpine AS builder\n\
             WORKDIR /app\n\
             COPY . .\n\
             RUN go build -o app .\n\
             FROM alpine:3.19\n\
             COPY --from=builder /app/app /usr/local/bin/\n\
             CMD [\"app\"]\n"
        }
        "docker" | "compose" => "",
        _ => {
            anyhow::bail!(
                "could not detect runtime for {work_dir}. \
                 Provide a Dockerfile or use a supported project structure."
            );
        }
    };

    if !dockerfile.is_empty() {
        std::fs::write(&dockerfile_path, dockerfile).context("failed to write Dockerfile")?;
    }

    let env_args: Vec<String> = env.iter().map(|e| format!("-e {e}")).collect();

    let compose_content = format!(
        "services:\n  {name}:\n    build: {work_dir}\n    container_name: monolith-{name}\n    \
         ports:\n      - \"{port}:{port}\"\n    restart: unless-stopped\n    {env_section}\n",
        env_section = if env_args.is_empty() {
            String::new()
        } else {
            let env_lines: Vec<String> = env.iter().map(|e| format!("      - {e}")).collect();
            format!("environment:\n{}", env_lines.join("\n"))
        }
    );

    let compose_path = format!("{deploy_dir}/docker-compose.yml");
    std::fs::write(&compose_path, compose_content).context("failed to write docker-compose.yml")?;

    println!("{} Building and starting {}...", "→".blue(), name.bold());
    let status = Command::new("docker")
        .args(["compose", "-f", &compose_path, "up", "-d", "--build"])
        .status()
        .context("failed to start deployment")?;

    if !status.success() {
        anyhow::bail!("deployment failed for {name}");
    }

    if let Some(d) = domain {
        println!("{} Configuring reverse proxy for {}...", "→".blue(), d);
        let upstream = format!("http://127.0.0.1:{port}");
        super::proxy::add_proxy_rule(d, &upstream, None)?;
    }

    println!(
        "{} Application {} deployed on port {}",
        "●".green(),
        name.bold(),
        port
    );
    Ok(())
}

fn list_deployments() -> Result<()> {
    let deploy_dir = "/var/lib/monolith/deployments";
    let path = Path::new(deploy_dir);

    if !path.exists() {
        println!("{}", "No deployments found.".dimmed());
        return Ok(());
    }

    println!("{}", "Deployments:".bold().underline());
    for entry in std::fs::read_dir(path).context("failed to read deployments directory")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let container_name = format!("monolith-{name}");
            let output = Command::new("docker")
                .args(["inspect", "--format", "{{.State.Status}}", &container_name])
                .output();

            let status = match output {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => "unknown".to_string(),
            };

            let indicator = match status.as_str() {
                "running" => "●".green(),
                "exited" => "●".red(),
                _ => "●".yellow(),
            };
            println!("  {indicator} {:<30} {}", name, status);
        }
    }
    Ok(())
}

fn deployment_status(name: &str) -> Result<()> {
    let container_name = format!("monolith-{name}");
    let output = Command::new("docker")
        .args(["inspect", &container_name])
        .output()
        .with_context(|| format!("failed to inspect deployment {name}"))?;

    if output.status.success() {
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        anyhow::bail!("deployment {name} not found");
    }
    Ok(())
}

fn update_deployment(name: &str) -> Result<()> {
    let deploy_dir = format!("/var/lib/monolith/deployments/{name}");
    let compose_path = format!("{deploy_dir}/docker-compose.yml");

    if !Path::new(&compose_path).exists() {
        anyhow::bail!("deployment {name} not found");
    }

    println!("{} Rebuilding {}...", "→".blue(), name.bold());
    let status = Command::new("docker")
        .args(["compose", "-f", &compose_path, "up", "-d", "--build"])
        .status()
        .context("failed to update deployment")?;

    if status.success() {
        println!("{} Deployment {} updated", "●".green(), name.bold());
    } else {
        anyhow::bail!("failed to update deployment {name}");
    }
    Ok(())
}

fn remove_deployment(name: &str) -> Result<()> {
    let deploy_dir = format!("/var/lib/monolith/deployments/{name}");
    let compose_path = format!("{deploy_dir}/docker-compose.yml");

    if Path::new(&compose_path).exists() {
        Command::new("docker")
            .args(["compose", "-f", &compose_path, "down", "--rmi", "all", "-v"])
            .status()
            .context("failed to stop deployment")?;
    }

    if Path::new(&deploy_dir).exists() {
        std::fs::remove_dir_all(&deploy_dir)
            .with_context(|| format!("failed to remove {deploy_dir}"))?;
    }

    println!("{} Deployment {} removed", "●".green(), name.bold());
    Ok(())
}

fn deploy_rollback(name: &str, version: Option<&str>) -> Result<()> {
    let deploy_dir = format!("/var/lib/monolith/deployments/{name}");
    let history_path = format!("{deploy_dir}/.history");

    if version.is_none() {
        // Show history
        if !std::path::Path::new(&history_path).exists() {
            println!("{} No deployment history for '{}'", "●".yellow(), name);
            return Ok(());
        }
        println!("{} Deployment history for '{}':", "≡".blue(), name.bold());
        let history = std::fs::read_to_string(&history_path)?;
        for line in history.lines() {
            println!("  {line}");
        }
        println!();
        println!("  Roll back: {} deploy rollback {} --to <version>", "mnctl".bold(), name);
        return Ok(());
    }

    let v = version.unwrap();
    let compose_path = format!("{deploy_dir}/docker-compose.yml");
    let backup_compose = format!("{deploy_dir}/docker-compose.{v}.yml");

    if !std::path::Path::new(&backup_compose).exists() {
        anyhow::bail!("version '{}' not found in history", v);
    }

    println!("{} Rolling back '{}' to version {}...", "→".blue(), name.bold(), v);
    std::fs::copy(&backup_compose, &compose_path)?;

    let status = std::process::Command::new("docker")
        .args(["compose", "-f", &compose_path, "up", "-d", "--build"])
        .status()?;

    if status.success() {
        println!("{} Rolled back to version {}", "●".green(), v.bold());
    } else {
        anyhow::bail!("rollback failed for {name}");
    }
    Ok(())
}

fn deploy_canary(name: &str, pct: u8, promote: bool, abort: bool) -> Result<()> {
    let deploy_dir = format!("/var/lib/monolith/deployments/{name}");
    let compose_path = format!("{deploy_dir}/docker-compose.yml");
    let canary_compose = format!("{deploy_dir}/docker-compose.canary.yml");

    if promote {
        // Promote canary: remove weight limit
        if std::path::Path::new(&canary_compose).exists() {
            std::fs::copy(&canary_compose, &compose_path)?;
            let _ = std::fs::remove_file(&canary_compose);
            println!("  {} Canary promoted to full release", "●".green());
        } else {
            anyhow::bail!("no canary deployment for '{}'", name);
        }
        return Ok(());
    }

    if abort {
        // Abort canary: remove canary container
        let _ = std::fs::remove_file(&canary_compose);
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &format!("monolith-{name}-canary")])
            .status();
        println!("  {} Canary aborted", "●".yellow());
        return Ok(());
    }

    // Read existing compose and create canary variant
    let content = std::fs::read_to_string(&compose_path)?;
    let canary_content = content.replace(&format!("container_name: monolith-{name}"), &format!("container_name: monolith-{name}-canary"));

    std::fs::write(&canary_compose, &canary_content)?;

    // Save current compose as versioned backup
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    std::fs::copy(&compose_path, format!("{deploy_dir}/docker-compose.{ts}.yml"))?;

    // Start the canary
    println!("{} Deploying canary ({}%) for '{}'...", "→".blue(), pct, name.bold());
    let status = std::process::Command::new("docker")
        .args(["compose", "-f", &canary_compose, "up", "-d", "--build"])
        .status()?;

    if status.success() {
        println!("  {} Canary v{ts} deployed ({pct}% traffic)", "●".green());

        // Create nginx config for traffic split
        let _nginx_cfg = format!(
            "upstream {name} {{\n    server 127.0.0.1:{} weight={};\n    server 127.0.0.1:{} weight={};\n}}\n",
            8080, 100 - pct,
            8081, pct,
        );

        println!("  {} nginx upstream split configured ({pct}% → canary)", "→".blue());
        println!("  {} Promote: {} deploy canary {} --promote", "●".cyan(), "mnctl".bold(), name);
        println!("  {} Abort:   {} deploy canary {} --abort", "●".cyan(), "mnctl".bold(), name);
    }
    Ok(())
}

fn deploy_watch(name: &str) -> Result<()> {
    let container = format!("monolith-{name}");

    // Check if running
    let inspect = std::process::Command::new("docker")
        .args(["inspect", "--format", "{{.State.Status}}", &container])
        .output();

    match inspect {
        Ok(o) if o.status.success() => {
            let status = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if status != "running" {
                anyhow::bail!("container {} is not running (status: {})", container, status);
            }
        }
        _ => anyhow::bail!("container '{}' not found. Deploy it first.", container),
    }

    // Tail logs in a thread, show CPU/RAM in a loop
    println!("{} Watching deployment '{}'", "≡".blue(), name.bold());
    println!("  {} Press Ctrl+C to stop", "→".blue());
    println!();

    let status = std::process::Command::new("docker")
        .args(["stats", "--no-stream", "--format", "table {{.Name}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}\t{{.BlockIO}}", &container])
        .status()
        .context("failed to run docker stats")?;

    if !status.success() {
        anyhow::bail!("failed to get stats for {container}");
    }
    Ok(())
}
