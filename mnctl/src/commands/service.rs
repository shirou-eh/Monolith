use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

#[derive(Subcommand)]
enum ServiceCommand {
    /// List all services with status
    List,
    /// Start a service
    Start {
        /// Service name
        name: String,
    },
    /// Stop a service
    Stop {
        /// Service name
        name: String,
    },
    /// Restart a service
    Restart {
        /// Service name
        name: String,
    },
    /// Enable service at boot
    Enable {
        /// Service name
        name: String,
    },
    /// Disable service at boot
    Disable {
        /// Service name
        name: String,
    },
    /// Show detailed service status with recent log lines
    Status {
        /// Service name
        name: String,
    },
    /// View service logs
    Logs {
        /// Service name
        name: String,
        /// Number of log lines to show
        #[arg(short = 'n', long, default_value = "50")]
        lines: u32,
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
        /// Show logs since timestamp
        #[arg(long)]
        since: Option<String>,
    },
    /// Edit service unit file in $EDITOR
    Edit {
        /// Service name
        name: String,
    },
    /// Interactive service creation wizard
    Create {
        /// Service name
        name: String,
    },
}

impl ServiceArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ServiceCommand::List => list_services(),
            ServiceCommand::Start { name } => systemctl("start", &name),
            ServiceCommand::Stop { name } => systemctl("stop", &name),
            ServiceCommand::Restart { name } => systemctl("restart", &name),
            ServiceCommand::Enable { name } => systemctl("enable", &name),
            ServiceCommand::Disable { name } => systemctl("disable", &name),
            ServiceCommand::Status { name } => service_status(&name),
            ServiceCommand::Logs {
                name,
                lines,
                follow,
                since,
            } => service_logs(&name, lines, follow, since.as_deref()),
            ServiceCommand::Edit { name } => edit_service(&name),
            ServiceCommand::Create { name } => create_service(&name),
        }
    }
}

fn systemctl(action: &str, name: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .args([action, name])
        .output()
        .with_context(|| format!("failed to {action} service {name}"))?;

    if output.status.success() {
        println!(
            "{} Service {} {}",
            "●".green(),
            name.bold(),
            format!("{action}ed successfully").green()
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to {action} {name}: {stderr}");
    }
    Ok(())
}

fn list_services() -> Result<()> {
    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .output()
        .context("failed to list services")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("{}", "Services:".bold().underline());
    println!();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let name = parts[0];
            let active = parts[2];
            let sub = parts[3];
            let indicator = match active {
                "active" => "●".green(),
                "inactive" => "●".dimmed(),
                "failed" => "●".red(),
                _ => "●".yellow(),
            };
            println!("  {indicator} {:<40} {:<10} {}", name, active, sub);
        }
    }
    Ok(())
}

fn service_status(name: &str) -> Result<()> {
    let output = Command::new("systemctl")
        .args(["status", name, "--no-pager", "-l"])
        .output()
        .with_context(|| format!("failed to get status of {name}"))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn service_logs(name: &str, lines: u32, follow: bool, since: Option<&str>) -> Result<()> {
    let lines_str = lines.to_string();
    let mut cmd_args = vec!["-u", name, "-n", &lines_str, "--no-pager"];
    if let Some(s) = since {
        cmd_args.push("--since");
        cmd_args.push(s);
    }
    if follow {
        cmd_args.push("-f");
    }

    let output = Command::new("journalctl")
        .args(&cmd_args)
        .output()
        .with_context(|| format!("failed to get logs for {name}"))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn edit_service(name: &str) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let unit_path = format!("/etc/systemd/system/{name}.service");

    let status = Command::new(&editor)
        .arg(&unit_path)
        .status()
        .with_context(|| format!("failed to open {unit_path} in {editor}"))?;

    if status.success() {
        println!("{}", "Reloading systemd daemon...".dimmed());
        Command::new("systemctl")
            .arg("daemon-reload")
            .status()
            .context("failed to reload systemd")?;
        println!("{} Unit file updated and daemon reloaded.", "●".green());
    }
    Ok(())
}

fn create_service(name: &str) -> Result<()> {
    use dialoguer::{Input, Select};

    println!("{}", "Service Creation Wizard".bold().underline());
    println!();

    let description: String = Input::new()
        .with_prompt("Service description")
        .interact_text()?;

    let exec_start: String = Input::new()
        .with_prompt("Command to run (ExecStart)")
        .interact_text()?;

    let user: String = Input::new()
        .with_prompt("Run as user")
        .default("root".to_string())
        .interact_text()?;

    let restart_options = &["always", "on-failure", "no"];
    let restart_idx = Select::new()
        .with_prompt("Restart policy")
        .items(restart_options)
        .default(1)
        .interact()?;

    let working_dir: String = Input::new()
        .with_prompt("Working directory (leave empty for none)")
        .default(String::new())
        .interact_text()?;

    let unit_content = format!(
        "[Unit]\n\
         Description={description}\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         User={user}\n\
         ExecStart={exec_start}\n\
         Restart={restart}\n\
         RestartSec=5\n\
         {working_dir_line}\
         StandardOutput=journal\n\
         StandardError=journal\n\
         SyslogIdentifier={name}\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        restart = restart_options[restart_idx],
        working_dir_line = if working_dir.is_empty() {
            String::new()
        } else {
            format!("WorkingDirectory={working_dir}\n")
        },
    );

    let unit_path = format!("/etc/systemd/system/{name}.service");
    std::fs::write(&unit_path, &unit_content)
        .with_context(|| format!("failed to write {unit_path}"))?;

    Command::new("systemctl")
        .arg("daemon-reload")
        .status()
        .context("failed to reload systemd")?;

    println!();
    println!(
        "{} Service {} created at {}",
        "●".green(),
        name.bold(),
        unit_path
    );
    println!("  Start with: {} service start {}", "mnctl".bold(), name);
    println!(
        "  Enable at boot: {} service enable {}",
        "mnctl".bold(),
        name
    );

    Ok(())
}
