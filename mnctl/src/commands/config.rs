use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

const CONFIG_PATH: &str = "/etc/monolith/monolith.toml";

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show current configuration
    Show,
    /// Set a configuration value
    Set {
        /// Configuration key (dot-separated path)
        key: String,
        /// Value to set
        value: String,
    },
    /// Open configuration in $EDITOR
    Edit,
    /// Validate configuration file
    Validate,
}

impl ConfigArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ConfigCommand::Show => show_config(),
            ConfigCommand::Set { key, value } => set_config(&key, &value),
            ConfigCommand::Edit => edit_config(),
            ConfigCommand::Validate => validate_config(),
        }
    }
}

fn show_config() -> Result<()> {
    if !std::path::Path::new(CONFIG_PATH).exists() {
        println!("{}", "No configuration file found.".yellow());
        println!("  Create one at: {}", CONFIG_PATH.bold());
        println!("  Or run: {} config edit", "mnctl".bold());
        return Ok(());
    }

    let content = std::fs::read_to_string(CONFIG_PATH).context("failed to read configuration")?;
    println!("{}", "Monolith Configuration:".bold().underline());
    println!();
    println!("{content}");
    Ok(())
}

fn set_config(key: &str, value: &str) -> Result<()> {
    let config_dir = std::path::Path::new(CONFIG_PATH)
        .parent()
        .unwrap_or(std::path::Path::new("/etc/monolith"));
    std::fs::create_dir_all(config_dir).context("failed to create config directory")?;

    let mut content = if std::path::Path::new(CONFIG_PATH).exists() {
        std::fs::read_to_string(CONFIG_PATH).context("failed to read config")?
    } else {
        String::from("# Monolith OS Configuration\n# https://github.com/shirou-eh/Monolith\n\n")
    };

    // Parse key path and update or add the value
    let parts: Vec<&str> = key.split('.').collect();
    let toml_key = parts.last().unwrap_or(&key);
    let section = if parts.len() > 1 {
        Some(parts[..parts.len() - 1].join("."))
    } else {
        None
    };

    // Simple key-value update/add
    let line = format!("{toml_key} = \"{value}\"\n");

    if let Some(ref sect) = section {
        let section_header = format!("[{sect}]");
        if content.contains(&section_header) {
            // Find existing key in section and replace, or add to section
            let key_pattern = format!("{toml_key} = ");
            if content.contains(&key_pattern) {
                let mut new_content = String::new();
                for l in content.lines() {
                    if l.trim_start().starts_with(&key_pattern) {
                        new_content.push_str(&line);
                    } else {
                        new_content.push_str(l);
                        new_content.push('\n');
                    }
                }
                content = new_content;
            } else {
                content = content.replace(&section_header, &format!("{section_header}\n{line}"));
            }
        } else {
            content.push_str(&format!("\n{section_header}\n{line}"));
        }
    } else {
        content.push_str(&line);
    }

    std::fs::write(CONFIG_PATH, &content).context("failed to write config")?;
    println!("{} Set {} = {}", "●".green(), key.bold(), value);
    Ok(())
}

fn edit_config() -> Result<()> {
    let config_dir = std::path::Path::new(CONFIG_PATH)
        .parent()
        .unwrap_or(std::path::Path::new("/etc/monolith"));
    std::fs::create_dir_all(config_dir)?;

    if !std::path::Path::new(CONFIG_PATH).exists() {
        let default_config = include_str!("../../config_default.toml");
        std::fs::write(CONFIG_PATH, default_config).context("failed to create default config")?;
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor)
        .arg(CONFIG_PATH)
        .status()
        .with_context(|| format!("failed to open {CONFIG_PATH} in {editor}"))?;

    if status.success() {
        validate_config()?;
    }
    Ok(())
}

fn validate_config() -> Result<()> {
    if !std::path::Path::new(CONFIG_PATH).exists() {
        println!("{}", "No configuration file to validate.".yellow());
        return Ok(());
    }

    let content = std::fs::read_to_string(CONFIG_PATH).context("failed to read config")?;

    match content.parse::<toml::Value>() {
        Ok(_) => {
            println!("{} Configuration is valid TOML", "●".green());
        }
        Err(e) => {
            println!("{} Configuration is invalid: {}", "●".red(), e);
            anyhow::bail!("invalid configuration");
        }
    }
    Ok(())
}
