use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

#[derive(Args)]
pub struct SecretsArgs {
    #[command(subcommand)]
    command: SecretsCommand,
}

#[derive(Subcommand)]
enum SecretsCommand {
    /// Create a new .env file or add a key
    Init {
        /// Application name (stored in /var/lib/monolith/secrets/<name>)
        name: String,
        /// Environment variable key (creates .env if not exists)
        key: Option<String>,
        /// Value for the key (prompts if not given)
        value: Option<String>,
    },
    /// List all managed secrets
    List,
    /// Show decrypted secret value(s)
    Show {
        /// Application name
        name: String,
        /// Specific key to show (omit for all)
        key: Option<String>,
    },
    /// Encrypt the .env file with age/sops
    Encrypt {
        /// Application name
        name: String,
        /// TPM device path (default: /dev/tpmrm0)
        #[arg(long)]
        tpm: Option<String>,
        /// YubiKey serial number
        #[arg(long)]
        yubikey: Option<String>,
        /// age public key (if not using TPM/YubiKey)
        #[arg(long)]
        age_key: Option<String>,
    },
    /// Decrypt the .env file for editing
    Decrypt {
        /// Application name
        name: String,
    },
    /// Export decrypted .env to stdout (for docker --env-file)
    Export {
        /// Application name
        name: String,
    },
}

impl SecretsArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            SecretsCommand::Init { name, key, value } => secrets_init(&name, key.as_deref(), value.as_deref()),
            SecretsCommand::List => secrets_list(),
            SecretsCommand::Show { name, key } => secrets_show(&name, key.as_deref()),
            SecretsCommand::Encrypt { name, tpm, yubikey, age_key } => {
                secrets_encrypt(&name, tpm.as_deref(), yubikey.as_deref(), age_key.as_deref())
            }
            SecretsCommand::Decrypt { name } => secrets_decrypt(&name),
            SecretsCommand::Export { name } => secrets_export(&name),
        }
    }
}

fn secrets_dir() -> String {
    "/var/lib/monolith/secrets".to_string()
}

fn secrets_init(name: &str, key: Option<&str>, value: Option<&str>) -> Result<()> {
    let dir = format!("{}/{}", secrets_dir(), name);
    let env_path = format!("{dir}/.env");
    let encrypted_path = format!("{dir}/.env.age");

    std::fs::create_dir_all(&dir).context("failed to create secrets directory")?;

    if key.is_none() {
        // Just create the directory structure
        if Path::new(&env_path).exists() || Path::new(&encrypted_path).exists() {
            println!("  {} Secrets for '{}' already exist", "●".yellow(), name);
            return Ok(());
        }
        std::fs::write(&env_path, "# Monolith Secrets — edit with `mnctl secrets decrypt`\n")?;
        println!("  {} Secrets initialized for '{}'", "●".green(), name);
        println!("  {} Add keys: mnctl secrets init {} KEY [value]", "→".blue(), name);
        println!("  {} Encrypt:  mnctl secrets encrypt {}", "→".blue(), name);
        return Ok(());
    }

    let k = key.unwrap();
    let v = match value {
        Some(val) => val.to_string(),
        None => {
            let v: String = dialoguer::Input::new()
                .with_prompt(format!("Value for {k}"))
                .allow_empty(false)
                .interact_text()?;
            v
        }
    };

    let mut content = String::new();
    if Path::new(&env_path).exists() {
        content = std::fs::read_to_string(&env_path)?;
    }

    // Replace existing key or append
    let key_line = format!("{k}=");
    if content.lines().any(|l| l.starts_with(&key_line) || l.trim_start().starts_with(&key_line)) {
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        for line in &mut lines {
            if line.starts_with(&key_line) || line.trim_start().starts_with(&key_line) {
                *line = format!("{k}={v}");
            }
        }
        content = lines.join("\n");
    } else {
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&format!("{k}={v}\n"));
    }

    std::fs::write(&env_path, &content)?;
    println!("  {} {} set in {}/.env", "●".green(), k, name);
    Ok(())
}

fn secrets_list() -> Result<()> {
    let dir = secrets_dir();
    let path = Path::new(&dir);

    if !path.exists() {
        println!("{}", "No secrets configured.".dimmed());
        return Ok(());
    }

    println!("{}", "Managed Secrets:".bold().underline());
    for entry in std::fs::read_dir(path).context("failed to read secrets directory")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            let encrypted = Path::new(&format!("{dir}/{name}/.env.age")).exists();
            let status = if encrypted { "encrypted".green() } else { "plaintext".yellow() };
            println!("  {} {:<30} {}", "●".green(), name, status);
        }
    }
    Ok(())
}

fn secrets_show(name: &str, key: Option<&str>) -> Result<()> {
    let env_path = format!("{}/{}/.env", secrets_dir(), name);
    let encrypted_path = format!("{}/{}/.env.age", secrets_dir(), name);

    let content = if Path::new(&encrypted_path).exists() {
        let output = Command::new("age")
            .args(["--decrypt", &encrypted_path])
            .output()
            .context("failed to decrypt — is age installed?")?;
        if !output.status.success() {
            anyhow::bail!("decryption failed — check your TPM/YubiKey/age key");
        }
        String::from_utf8_lossy(&output.stdout).to_string()
    } else if Path::new(&env_path).exists() {
        std::fs::read_to_string(&env_path)?
    } else {
        anyhow::bail!("secrets for '{}' not found", name);
    };

    match key {
        Some(k) => {
            for line in content.lines() {
                if line.starts_with(&format!("{k}=")) || line.trim_start().starts_with(&format!("{k}=")) {
                    let val = line.splitn(2, '=').nth(1).unwrap_or("");
                    println!("{}={}", k.bold(), val);
                    return Ok(());
                }
            }
            anyhow::bail!("key '{}' not found in secrets for '{}'", k, name);
        }
        None => {
            println!("{} Secrets for '{}':", "≡".blue(), name.bold());
            for line in content.lines() {
                if line.contains('=') && !line.starts_with('#') {
                    let k = line.splitn(2, '=').next().unwrap_or("");
                    println!("  {} = ***", k.bold());
                }
            }
        }
    }
    Ok(())
}

fn secrets_encrypt(name: &str, tpm: Option<&str>, yubikey: Option<&str>, age_key: Option<&str>) -> Result<()> {
    let env_path = format!("{}/{}/.env", secrets_dir(), name);
    let encrypted_path = format!("{}/{}/.env.age", secrets_dir(), name);

    if !Path::new(&env_path).exists() {
        anyhow::bail!(".env not found for '{}'. Run `mnctl secrets init {}` first", name, name);
    }

    let age_bin = which::which("age").ok();
    let age_plugin = if tpm.is_some() {
        which::which("age-plugin-tpm").ok()
    } else if yubikey.is_some() {
        which::which("age-plugin-yubikey").ok()
    } else {
        None
    };

    if age_bin.is_none() {
        anyhow::bail!("age not installed. Install: mnpkg install age");
    }

    let recipient = if let Some(tpm_path) = tpm {
        // TPM: generate an age identity from TPM
        if age_plugin.is_none() {
            anyhow::bail!("age-plugin-tpm not installed");
        }
        println!("  {} Using TPM at {}", "→".blue(), tpm_path);
        let output = Command::new("age-plugin-tpm")
            .args(["--recipient", "--tpm-path", tpm_path])
            .output()
            .context("failed to get TPM recipient key")?;
        if !output.status.success() {
            anyhow::bail!("TPM key generation failed");
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else if let Some(serial) = yubikey {
        if age_plugin.is_none() {
            anyhow::bail!("age-plugin-yubikey not installed");
        }
        println!("  {} Using YubiKey serial {}", "→".blue(), serial);
        let output = Command::new("age-plugin-yubikey")
            .args(["--recipient", "--serial", serial])
            .output()
            .context("failed to get YubiKey recipient key")?;
        if !output.status.success() {
            anyhow::bail!("YubiKey key generation failed");
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else if let Some(key) = age_key {
        format!("age1{key}")
    } else {
        anyhow::bail!("specify --tpm, --yubikey, or --age-key");
    };

    let status = Command::new("age")
        .args(["--encrypt", "--recipient", &recipient, "-o", &encrypted_path, &env_path])
        .status()
        .context("age encryption failed")?;

    if !status.success() {
        anyhow::bail!("encryption failed");
    }

    // Wipe the plaintext .env
    std::fs::write(&env_path, "")?;
    println!("  {} .env.age encrypted", "●".green());
    println!("  {} Plaintext .env wiped", "●".yellow());
    Ok(())
}

fn secrets_decrypt(name: &str) -> Result<()> {
    let env_path = format!("{}/{}/.env", secrets_dir(), name);
    let encrypted_path = format!("{}/{}/.env.age", secrets_dir(), name);

    if !Path::new(&encrypted_path).exists() {
        anyhow::bail!(".env.age not found for '{}'", name);
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());

    // Decrypt to temp, open editor, re-encrypt
    let tmp = format!("/tmp/monolith-{name}-env");
    let decrypt_status = Command::new("age")
        .args(["--decrypt", "-o", &tmp, &encrypted_path])
        .status()
        .context("decryption failed")?;

    if !decrypt_status.success() {
        anyhow::bail!("decryption failed — wrong key?");
    }

    let edit_status = Command::new(&editor).args([&tmp]).status()?;

    if !edit_status.success() {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("editor exited with error");
    }

    // Check if file changed
    let content = std::fs::read_to_string(&tmp)?;
    if content.trim().is_empty() {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("file is empty — aborting");
    }

    // Re-encrypt from temp
    let output = Command::new("age")
        .args(["--encrypt", "-o", &encrypted_path, "-i", &tmp])
        .output()
        .context("re-encryption failed")?;

    // Read the identity used from encrypted header
    let _ = std::fs::remove_file(&tmp);

    if output.status.success() {
        println!("  {} Re-encrypted .env.age", "●".green());
    } else {
        anyhow::bail!("re-encryption failed — .env is at {tmp}");
    }
    Ok(())
}

fn secrets_export(name: &str) -> Result<()> {
    let env_path = format!("{}/{}/.env", secrets_dir(), name);
    let encrypted_path = format!("{}/{}/.env.age", secrets_dir(), name);

    if Path::new(&encrypted_path).exists() {
        let output = Command::new("age")
            .args(["--decrypt", &encrypted_path])
            .output()
            .context("decryption failed")?;
        if !output.status.success() {
            anyhow::bail!("decryption failed");
        }
        print!("{}", String::from_utf8_lossy(&output.stdout));
    } else if Path::new(&env_path).exists() {
        let content = std::fs::read_to_string(&env_path)?;
        print!("{content}");
    } else {
        anyhow::bail!("no secrets for '{}'", name);
    }
    Ok(())
}

/// Resolve one or more --env-secret arguments into env-file paths.
/// Called from deploy.rs when building a deployment.
pub fn resolve_env_secrets(secrets: &[String]) -> Result<Vec<String>> {
    let mut env_files = Vec::new();
    for secret in secrets {
        // Parse "name:KEY" or just "name"
        let parts: Vec<&str> = secret.splitn(2, ':').collect();
        let name = parts[0];
        let key_filter = parts.get(1).copied();

        let encrypted_path = format!("{}/{}/.env.age", secrets_dir(), name);
        let env_path = format!("{}/{}/.env", secrets_dir(), name);

        let decrypted = if std::path::Path::new(&encrypted_path).exists() {
            let output = Command::new("age")
                .args(["--decrypt", &encrypted_path])
                .output()
                .with_context(|| format!("failed to decrypt secrets for '{}'", name))?;
            if !output.status.success() {
                anyhow::bail!("decryption failed for '{}'", name);
            }
            String::from_utf8_lossy(&output.stdout).to_string()
        } else if std::path::Path::new(&env_path).exists() {
            std::fs::read_to_string(&env_path)?
        } else {
            anyhow::bail!("secrets '{}' not found", name);
        };

        let tmp = format!("/tmp/monolith-env-secret-{name}");
        if let Some(kf) = key_filter {
            // Write only the matching key
            let mut out = String::new();
            for line in decrypted.lines() {
                if line.starts_with(&format!("{kf}=")) {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            std::fs::write(&tmp, &out)?;
        } else {
            std::fs::write(&tmp, &decrypted)?;
        }
        env_files.push(tmp);
    }
    Ok(env_files)
}
