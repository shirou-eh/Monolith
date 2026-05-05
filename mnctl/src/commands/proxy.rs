use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct ProxyArgs {
    #[command(subcommand)]
    command: ProxyCommand,
}

#[derive(Subcommand)]
enum ProxyCommand {
    /// List all proxy rules
    List,
    /// Add a reverse proxy rule with automatic TLS
    Add {
        /// Domain name
        domain: String,
        /// Upstream server (e.g., http://127.0.0.1:3000)
        upstream: String,
        /// Email for Let's Encrypt certificate registration
        #[arg(long)]
        email: Option<String>,
    },
    /// Remove a proxy rule
    Remove {
        /// Domain name
        domain: String,
    },
    /// TLS certificate management
    Ssl(SslArgs),
    /// Reload nginx configuration
    Reload,
}

#[derive(Args)]
struct SslArgs {
    #[command(subcommand)]
    command: SslCommand,
}

#[derive(Subcommand)]
enum SslCommand {
    /// Renew TLS certificates
    Renew {
        /// Specific domain to renew (renews all if omitted)
        domain: Option<String>,
    },
    /// Show certificate status and expiry dates
    Status,
}

impl ProxyArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            ProxyCommand::List => list_proxies(),
            ProxyCommand::Add {
                domain,
                upstream,
                email,
            } => add_proxy_rule(&domain, &upstream, email.as_deref()),
            ProxyCommand::Remove { domain } => remove_proxy_rule(&domain),
            ProxyCommand::Ssl(args) => match args.command {
                SslCommand::Renew { domain } => ssl_renew(domain.as_deref()),
                SslCommand::Status => ssl_status(),
            },
            ProxyCommand::Reload => reload_nginx(),
        }
    }
}

fn list_proxies() -> Result<()> {
    let sites_dir = "/etc/nginx/sites-enabled";
    let path = std::path::Path::new(sites_dir);

    if !path.exists() {
        println!("{}", "No proxy rules configured.".dimmed());
        return Ok(());
    }

    println!("{}", "Proxy Rules:".bold().underline());
    for entry in std::fs::read_dir(path).context("failed to read nginx sites")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();

        let content = std::fs::read_to_string(entry.path()).unwrap_or_default();

        let server_name = content
            .lines()
            .find(|l| l.contains("server_name"))
            .map(|l| l.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let proxy_pass = content
            .lines()
            .find(|l| l.contains("proxy_pass"))
            .map(|l| l.trim().to_string())
            .unwrap_or_else(|| "none".to_string());

        println!("  {} {:<30} → {}", "●".green(), server_name, proxy_pass);
        let _ = name; // used above via entry
    }
    Ok(())
}

pub fn add_proxy_rule(domain: &str, upstream: &str, email: Option<&str>) -> Result<()> {
    let sites_dir = "/etc/nginx/sites-available";
    let enabled_dir = "/etc/nginx/sites-enabled";
    std::fs::create_dir_all(sites_dir).context("failed to create nginx sites directory")?;
    std::fs::create_dir_all(enabled_dir)
        .context("failed to create nginx sites-enabled directory")?;

    let config = format!(
        r#"# Managed by mnctl — do not edit manually
server {{
    listen 80;
    listen [::]:80;
    server_name {domain};

    location /.well-known/acme-challenge/ {{
        root /var/www/certbot;
    }}

    location / {{
        return 301 https://$host$request_uri;
    }}
}}

server {{
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name {domain};

    ssl_certificate /etc/letsencrypt/live/{domain}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{domain}/privkey.pem;

    # Security headers
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;
    add_header Strict-Transport-Security "max-age=63072000; includeSubDomains; preload" always;

    # Proxy settings
    location / {{
        proxy_pass {upstream};
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_cache_bypass $http_upgrade;
        proxy_read_timeout 86400s;
        proxy_send_timeout 86400s;
    }}
}}
"#
    );

    let config_path = format!("{sites_dir}/{domain}");
    let enabled_path = format!("{enabled_dir}/{domain}");

    std::fs::write(&config_path, &config)
        .with_context(|| format!("failed to write nginx config for {domain}"))?;

    // Create symlink in sites-enabled
    if std::path::Path::new(&enabled_path).exists() {
        std::fs::remove_file(&enabled_path)?;
    }
    std::os::unix::fs::symlink(&config_path, &enabled_path).context("failed to create symlink")?;

    // Resolve certbot email: --email flag > config > error
    let certbot_email = match email {
        Some(e) => e.to_string(),
        None => {
            let config_path = "/etc/monolith/monolith.toml";
            std::fs::read_to_string(config_path)
                .ok()
                .and_then(|content| content.parse::<toml::Value>().ok())
                .and_then(|v| {
                    v.get("notifications")
                        .and_then(|n| n.get("email"))
                        .and_then(|e| e.as_str())
                        .filter(|e| !e.is_empty())
                        .map(|e| e.to_string())
                })
                .unwrap_or_default()
        }
    };

    if certbot_email.is_empty() {
        println!(
            "  {} No email configured for Let's Encrypt. Set [notifications].email in /etc/monolith/monolith.toml or pass --email",
            "●".yellow(),
        );
        println!(
            "  {} Skipping TLS certificate — configure email and run: certbot certonly --webroot -w /var/www/certbot -d {domain}",
            "●".yellow()
        );
    } else {
        println!("{} Obtaining TLS certificate for {}...", "→".blue(), domain);
        let certbot_result = Command::new("certbot")
            .args([
                "certonly",
                "--webroot",
                "-w",
                "/var/www/certbot",
                "-d",
                domain,
                "--non-interactive",
                "--agree-tos",
                "--email",
                &certbot_email,
            ])
            .status();

        match certbot_result {
            Ok(s) if s.success() => {
                println!("  {} TLS certificate obtained", "●".green());
            }
            _ => {
                println!(
                    "  {} Certificate not obtained — configure manually or run certbot",
                    "●".yellow()
                );
            }
        }
    }

    reload_nginx()?;
    println!(
        "{} Proxy rule added: {} → {}",
        "●".green(),
        domain.bold(),
        upstream
    );
    Ok(())
}

fn remove_proxy_rule(domain: &str) -> Result<()> {
    let available = format!("/etc/nginx/sites-available/{domain}");
    let enabled = format!("/etc/nginx/sites-enabled/{domain}");

    if std::path::Path::new(&enabled).exists() {
        std::fs::remove_file(&enabled).context("failed to remove symlink")?;
    }
    if std::path::Path::new(&available).exists() {
        std::fs::remove_file(&available).context("failed to remove config")?;
    }

    reload_nginx()?;
    println!("{} Proxy rule for {} removed", "●".green(), domain.bold());
    Ok(())
}

fn ssl_renew(domain: Option<&str>) -> Result<()> {
    let mut args = vec!["renew"];
    if let Some(d) = domain {
        args.push("--cert-name");
        args.push(d);
    }

    let status = Command::new("certbot")
        .args(&args)
        .status()
        .context("failed to renew certificates")?;

    if status.success() {
        reload_nginx()?;
        println!("{} Certificates renewed", "●".green());
    } else {
        anyhow::bail!("certificate renewal failed");
    }
    Ok(())
}

fn ssl_status() -> Result<()> {
    let output = Command::new("certbot")
        .args(["certificates"])
        .output()
        .context("failed to get certificate status")?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn reload_nginx() -> Result<()> {
    // Test config first
    let test = Command::new("nginx")
        .args(["-t"])
        .output()
        .context("failed to test nginx config")?;

    if !test.status.success() {
        let stderr = String::from_utf8_lossy(&test.stderr);
        anyhow::bail!("nginx config test failed: {stderr}");
    }

    let status = Command::new("systemctl")
        .args(["reload", "nginx"])
        .status()
        .context("failed to reload nginx")?;

    if status.success() {
        println!("{} nginx reloaded", "●".green());
    }
    Ok(())
}
