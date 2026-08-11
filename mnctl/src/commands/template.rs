use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct TemplateArgs {
    #[command(subcommand)]
    command: TemplateCommand,
}

#[derive(Subcommand)]
enum TemplateCommand {
    /// List available templates
    List,
    /// Deploy from a template
    Deploy {
        /// Template name
        template: String,
        /// Service name
        #[arg(long)]
        name: Option<String>,
    },
    /// Show template details
    Info {
        /// Template name
        template: String,
    },
    /// Update an existing deployed template without recreating
    Update {
        /// Template name
        name: String,
        /// New image tag (if left out, pull latest)
        #[arg(long)]
        tag: Option<String>,
        /// Path to new docker-compose template file
        #[arg(long)]
        file: Option<String>,
    },
}

struct Template {
    name: &'static str,
    description: &'static str,
    category: &'static str,
}

const TEMPLATES: &[Template] = &[
    Template {
        name: "minecraft",
        description: "Minecraft Java Edition server (Paper/Vanilla/Fabric/Forge)",
        category: "Game Servers",
    },
    Template {
        name: "cs2",
        description: "Counter-Strike 2 dedicated server",
        category: "Game Servers",
    },
    Template {
        name: "valheim",
        description: "Valheim dedicated server with auto-updates and backups",
        category: "Game Servers",
    },
    Template {
        name: "palworld",
        description: "Palworld dedicated server with RCON and Steam query",
        category: "Game Servers",
    },
    Template {
        name: "postgresql",
        description: "PostgreSQL 16 with optimized server config",
        category: "Databases",
    },
    Template {
        name: "mariadb",
        description: "MariaDB 11 with utf8mb4 and tuned InnoDB",
        category: "Databases",
    },
    Template {
        name: "mongodb",
        description: "MongoDB 7 with authentication and WiredTiger tuning",
        category: "Databases",
    },
    Template {
        name: "redis",
        description: "Redis 7 with persistence and optimized config",
        category: "Databases",
    },
    Template {
        name: "nodejs-app",
        description: "Node.js application with nginx reverse proxy",
        category: "Web",
    },
    Template {
        name: "discord-bot-python",
        description: "Python Discord bot (disnake/discord.py/nextcord)",
        category: "Bots",
    },
    Template {
        name: "discord-bot-node",
        description: "Node.js Discord bot starter (discord.js v14)",
        category: "Bots",
    },
    Template {
        name: "telegram-bot",
        description: "Async Telegram bot starter (python-telegram-bot v21)",
        category: "Bots",
    },
    Template {
        name: "nginx-reverse-proxy",
        description: "Nginx reverse proxy with automatic TLS via certbot",
        category: "Infrastructure",
    },
    Template {
        name: "factorio",
        description: "Factorio headless server with RCON and auto-backup",
        category: "Game Servers",
    },
    Template {
        name: "terraria",
        description: "Terraria TShock server with REST API and auto-backup",
        category: "Game Servers",
    },
];

impl TemplateArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            TemplateCommand::List => list_templates(),
            TemplateCommand::Deploy { template, name } => {
                deploy_template(&template, name.as_deref())
            }
            TemplateCommand::Info { template } => template_info(&template),
            TemplateCommand::Update { name, tag, file } => {
                template_update(&name, tag.as_deref(), file.as_deref())
            }
        }
    }
}

fn list_templates() -> Result<()> {
    println!("{}", "Available Templates:".bold().underline());
    println!();

    let mut current_category = "";
    for t in TEMPLATES {
        if t.category != current_category {
            println!("  {}", t.category.bold());
            current_category = t.category;
        }
        println!(
            "    {} {:<25} {}",
            "●".green(),
            t.name,
            t.description.dimmed()
        );
    }
    println!();
    println!("Deploy with: {} template deploy <name>", "mnctl".bold());
    Ok(())
}

fn deploy_template(template: &str, name: Option<&str>) -> Result<()> {
    let tmpl = TEMPLATES
        .iter()
        .find(|t| t.name == template)
        .ok_or_else(|| anyhow::anyhow!("unknown template: {template}"))?;

    let service_name = name.unwrap_or(template);
    let deploy_dir = format!("/var/lib/monolith/deployments/{service_name}");

    // Look for template in multiple locations
    let template_dirs = [
        format!("/usr/share/monolith/templates/{template}"),
        format!("templates/{template}"),
    ];

    let source_dir = template_dirs
        .iter()
        .find(|d| std::path::Path::new(d).exists())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "template files not found. Expected at: {}",
                template_dirs.join(" or ")
            )
        })?;

    std::fs::create_dir_all(&deploy_dir)?;

    // Copy template files
    let status = Command::new("cp")
        .args(["-r", &format!("{source_dir}/."), &deploy_dir])
        .status()
        .context("failed to copy template files")?;

    if !status.success() {
        anyhow::bail!("failed to copy template files");
    }

    // Deploy with docker compose
    let compose_path = format!("{deploy_dir}/docker-compose.yml");
    if std::path::Path::new(&compose_path).exists() {
        // Check Docker socket availability before proceeding
        let docker_sock = std::path::Path::new("/var/run/docker.sock");
        if !docker_sock.exists() {
            anyhow::bail!(
                "✗  Docker socket not reachable: /var/run/docker.sock\n\
                   Is docker.service running? Try: mnctl service start docker"
            );
        }

        println!(
            "{} Deploying {} ({})...",
            "→".blue(),
            service_name.bold(),
            tmpl.description
        );

        let status = Command::new("docker")
            .args(["compose", "-f", &compose_path, "up", "-d"])
            .status()
            .context("failed to start template deployment")?;

        if status.success() {
            println!(
                "{} {} deployed from template '{}'",
                "●".green(),
                service_name.bold(),
                template
            );
        } else {
            anyhow::bail!("deployment failed");
        }
    } else {
        println!(
            "{} Template files copied to {}. Configure and start manually.",
            "●".green(),
            deploy_dir.bold()
        );
    }
    Ok(())
}

fn template_info(template: &str) -> Result<()> {
    let tmpl = TEMPLATES
        .iter()
        .find(|t| t.name == template)
        .ok_or_else(|| anyhow::anyhow!("unknown template: {template}"))?;

    println!("{}", tmpl.name.bold().underline());
    println!("  Category:    {}", tmpl.category);
    println!("  Description: {}", tmpl.description);
    println!();

    // Show README if available
    let readme_paths = [
        format!("/usr/share/monolith/templates/{template}/README.md"),
        format!("templates/{template}/README.md"),
    ];

    for path in &readme_paths {
        if std::path::Path::new(path).exists() {
            let content = std::fs::read_to_string(path)?;
            println!("{content}");
            return Ok(());
        }
    }

    println!(
        "Deploy with: {} template deploy {}",
        "mnctl".bold(),
        template
    );
    Ok(())
}

fn template_update(name: &str, tag: Option<&str>, file: Option<&str>) -> Result<()> {
    let deploy_dir = format!("/var/lib/monolith/deployments/{name}");
    let compose_path = format!("{deploy_dir}/docker-compose.yml");

    if !std::path::Path::new(&compose_path).exists() {
        anyhow::bail!(
            "template '{}' not found at {deploy_dir}. Deploy it first with 'mnctl template apply'",
            name
        );
    }

    println!("{} Updating template '{}'", "→".blue(), name.bold());

    // Backup current compose
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let backup_path = format!("{deploy_dir}/docker-compose.{ts}.bak");
    std::fs::copy(&compose_path, &backup_path)?;
    println!("  {} Backed up to {backup_path}", "●".cyan());

    if let Some(file_path) = file {
        // Replace from new file
        std::fs::copy(file_path, &compose_path)?;
        println!("  {} Replaced from {file_path}", "●".green());
    } else if let Some(tag_val) = tag {
        // Update image tag in compose file
        let content = std::fs::read_to_string(&compose_path)?;
        let updated = content.replace(&format!("{name}:latest"), &format!("{name}:{tag_val}"));
        std::fs::write(&compose_path, &updated)?;
        println!("  {} Updated image tag to {tag_val}", "●".green());
    } else {
        // Just pull latest
        println!("  {} Pulling latest images...", "→".blue());
    }

    // Redeploy
    let status = std::process::Command::new("docker")
        .args(["compose", "-f", &compose_path, "pull"])
        .status()?;

    if !status.success() {
        println!("  {} Pull failed, deploying from cache", "⚠".yellow());
    }

    let status = std::process::Command::new("docker")
        .args([
            "compose",
            "-f",
            &compose_path,
            "up",
            "-d",
            "--remove-orphans",
        ])
        .status()?;

    if status.success() {
        println!(
            "  {} Template '{}' updated successfully",
            "●".green(),
            name.bold()
        );
    } else {
        anyhow::bail!("failed to redeploy '{}' after update", name);
    }

    Ok(())
}
