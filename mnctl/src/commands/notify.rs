//! Notification dispatch — webhook + SMTP.
//!
//! Loads `[notifications]` and `[notifications.smtp]` from
//! `/etc/monolith/monolith.toml` and lets operators send ad-hoc messages or
//! test the configured channels.
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use std::process::Command;

const CONFIG_PATH: &str = "/etc/monolith/monolith.toml";

#[derive(Args)]
pub struct NotifyArgs {
    #[command(subcommand)]
    command: NotifyCommand,
}

#[derive(Subcommand)]
enum NotifyCommand {
    /// Send a test message through every enabled channel
    Test,
    /// Send a notification to all configured channels
    Send {
        /// Subject / title of the message
        #[arg(long, default_value = "Monolith OS notification")]
        subject: String,
        /// Body of the message
        #[arg(long)]
        body: String,
    },
    /// Send a webhook notification only
    Webhook {
        /// Override the webhook URL (otherwise uses config)
        #[arg(long)]
        url: Option<String>,
        /// JSON body to POST
        #[arg(long)]
        body: String,
    },
    /// Send an email via the configured SMTP relay
    Email {
        /// Recipient (otherwise uses config.email)
        #[arg(long)]
        to: Option<String>,
        /// Subject line
        #[arg(long)]
        subject: String,
        /// Message body
        #[arg(long)]
        body: String,
    },
    /// Print the loaded notifications config
    Show,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct NotificationConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    webhook_url: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    smtp: SmtpConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SmtpConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    host: String,
    #[serde(default = "default_smtp_port")]
    port: u16,
    #[serde(default)]
    user: String,
    /// Either an inline password (discouraged) or path to a file containing it.
    #[serde(default)]
    password: String,
    #[serde(default)]
    password_file: String,
    /// "starttls" (default), "tls", or "plain".
    #[serde(default = "default_security")]
    security: String,
    #[serde(default)]
    from: String,
}

fn default_smtp_port() -> u16 {
    587
}

fn default_security() -> String {
    "starttls".to_string()
}

#[derive(Debug, Deserialize, Default)]
struct WrappedConfig {
    #[serde(default)]
    notifications: NotificationConfig,
}

impl NotifyArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            NotifyCommand::Test => test_all().await,
            NotifyCommand::Send { subject, body } => send_all(&subject, &body).await,
            NotifyCommand::Webhook { url, body } => {
                let cfg = load_config().unwrap_or_default();
                let target = url.unwrap_or_else(|| cfg.webhook_url.clone());
                if target.is_empty() {
                    anyhow::bail!("no webhook URL configured");
                }
                webhook_send(&target, &body).await
            }
            NotifyCommand::Email { to, subject, body } => {
                let cfg = load_config().unwrap_or_default();
                let recipient = to.unwrap_or_else(|| cfg.email.clone());
                if recipient.is_empty() {
                    anyhow::bail!("no recipient configured");
                }
                smtp_send(&cfg.smtp, &recipient, &subject, &body)
            }
            NotifyCommand::Show => show(),
        }
    }
}

fn load_config() -> Result<NotificationConfig> {
    let content = std::fs::read_to_string(CONFIG_PATH)
        .with_context(|| format!("failed to read {CONFIG_PATH}"))?;
    let wrapped: WrappedConfig = toml::from_str(&content).context("failed to parse config")?;
    Ok(wrapped.notifications)
}

fn show() -> Result<()> {
    let cfg = load_config().unwrap_or_default();
    println!("{}", "Notifications:".bold().underline());
    println!("  enabled:     {}", cfg.enabled);
    println!(
        "  webhook_url: {}",
        if cfg.webhook_url.is_empty() {
            "—".to_string()
        } else {
            redact_url(&cfg.webhook_url)
        }
    );
    println!(
        "  email:       {}",
        if cfg.email.is_empty() {
            "—"
        } else {
            cfg.email.as_str()
        }
    );
    println!();
    println!("{}", "  SMTP:".bold());
    println!("    enabled:  {}", cfg.smtp.enabled);
    println!(
        "    host:     {}:{}",
        if cfg.smtp.host.is_empty() {
            "—"
        } else {
            cfg.smtp.host.as_str()
        },
        cfg.smtp.port
    );
    println!(
        "    user:     {}",
        if cfg.smtp.user.is_empty() {
            "—"
        } else {
            cfg.smtp.user.as_str()
        }
    );
    println!("    security: {}", cfg.smtp.security);
    println!(
        "    from:     {}",
        if cfg.smtp.from.is_empty() {
            "—"
        } else {
            cfg.smtp.from.as_str()
        }
    );
    Ok(())
}

async fn test_all() -> Result<()> {
    let cfg = load_config().unwrap_or_default();
    if !cfg.enabled {
        println!(
            "{}",
            "Notifications are disabled in config (notifications.enabled = false).".yellow()
        );
    }
    let mut had_channel = false;
    if !cfg.webhook_url.is_empty() {
        had_channel = true;
        match webhook_send(
            &cfg.webhook_url,
            "Monolith test webhook from `mnctl notify test`",
        )
        .await
        {
            Ok(_) => println!("{} webhook OK", "●".green()),
            Err(e) => println!("{} webhook FAILED: {e}", "●".red()),
        }
    }
    if cfg.smtp.enabled && !cfg.email.is_empty() {
        had_channel = true;
        match smtp_send(
            &cfg.smtp,
            &cfg.email,
            "Monolith OS — SMTP test",
            "If you received this, SMTP notifications are working.",
        ) {
            Ok(_) => println!("{} smtp OK ({})", "●".green(), cfg.email),
            Err(e) => println!("{} smtp FAILED: {e}", "●".red()),
        }
    }
    if !had_channel {
        println!(
            "{}",
            "No channels configured. Edit /etc/monolith/monolith.toml to add a webhook or SMTP."
                .yellow()
        );
    }
    Ok(())
}

async fn send_all(subject: &str, body: &str) -> Result<()> {
    let cfg = load_config().unwrap_or_default();
    let mut sent = 0usize;
    if !cfg.webhook_url.is_empty() {
        let payload = serde_json::json!({"subject": subject, "body": body});
        webhook_send_json(&cfg.webhook_url, &payload).await?;
        sent += 1;
    }
    if cfg.smtp.enabled && !cfg.email.is_empty() {
        smtp_send(&cfg.smtp, &cfg.email, subject, body)?;
        sent += 1;
    }
    if sent == 0 {
        anyhow::bail!("no enabled notification channels");
    }
    println!(
        "{} notification dispatched on {sent} channel(s)",
        "●".green()
    );
    Ok(())
}

async fn webhook_send(url: &str, body: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client.post(url).body(body.to_string()).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("webhook returned {}", resp.status());
    }
    Ok(())
}

async fn webhook_send_json(url: &str, body: &serde_json::Value) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client.post(url).json(body).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("webhook returned {}", resp.status());
    }
    Ok(())
}

fn smtp_send(cfg: &SmtpConfig, to: &str, subject: &str, body: &str) -> Result<()> {
    if cfg.host.is_empty() {
        anyhow::bail!("SMTP host not configured");
    }

    // We shell out to msmtp/sendmail/curl to keep the binary small. msmtp is
    // the preferred path because it understands STARTTLS + auth out of the box.
    if which::which("msmtp").is_ok() {
        return smtp_via_msmtp(cfg, to, subject, body);
    }
    if which::which("curl").is_ok() {
        return smtp_via_curl(cfg, to, subject, body);
    }
    anyhow::bail!(
        "no SMTP client available. Install msmtp (pacman -S msmtp) or ensure curl is on PATH"
    );
}

fn smtp_password(cfg: &SmtpConfig) -> Option<String> {
    if !cfg.password.is_empty() {
        return Some(cfg.password.clone());
    }
    if !cfg.password_file.is_empty() {
        if let Ok(content) = std::fs::read_to_string(&cfg.password_file) {
            return Some(content.trim().to_string());
        }
    }
    None
}

fn smtp_via_msmtp(cfg: &SmtpConfig, to: &str, subject: &str, body: &str) -> Result<()> {
    // Resolve the password before spawning so we can pass it through
    // Command::env(). Setting std::env::set_var after spawn() has no effect on
    // the child since the environment is captured at fork time.
    let password = smtp_password(cfg);

    let mut command = Command::new("msmtp");
    command
        .args([
            "--host",
            &cfg.host,
            "--port",
            &cfg.port.to_string(),
            match cfg.security.as_str() {
                "tls" => "--tls=on",
                "plain" => "--tls=off",
                _ => "--tls=on",
            },
            if cfg.security == "starttls" {
                "--tls-starttls=on"
            } else {
                "--tls-starttls=off"
            },
            "--auth=on",
            "--user",
            if cfg.user.is_empty() {
                cfg.from.as_str()
            } else {
                cfg.user.as_str()
            },
            "--from",
            if cfg.from.is_empty() {
                cfg.user.as_str()
            } else {
                cfg.from.as_str()
            },
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    if let Some(ref pw) = password {
        // msmtp reads $MSMTP_PASSWORD when --passwordeval="echo $MSMTP_PASSWORD"
        // is set, but the simpler path is to instruct msmtp to evaluate the
        // variable directly. Setting it on the child env (not the parent's)
        // keeps the secret scoped to this single invocation.
        command
            .env("MSMTP_PASSWORD", pw)
            .arg("--passwordeval=printenv MSMTP_PASSWORD");
    }

    command.arg("--").arg(to);

    let mut child = command.spawn().context("failed to spawn msmtp")?;

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin");
        let from = if cfg.from.is_empty() {
            &cfg.user
        } else {
            &cfg.from
        };
        writeln!(stdin, "From: {from}")?;
        writeln!(stdin, "To: {to}")?;
        writeln!(stdin, "Subject: {subject}")?;
        writeln!(stdin, "Content-Type: text/plain; charset=UTF-8")?;
        writeln!(stdin)?;
        write!(stdin, "{body}")?;
    }

    let status = child.wait().context("msmtp exited unexpectedly")?;
    if !status.success() {
        anyhow::bail!("msmtp exited {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

fn smtp_via_curl(cfg: &SmtpConfig, to: &str, subject: &str, body: &str) -> Result<()> {
    let scheme = match cfg.security.as_str() {
        "tls" => "smtps",
        "plain" => "smtp",
        _ => "smtp",
    };
    let url = format!("{scheme}://{}:{}", cfg.host, cfg.port);
    let from = if cfg.from.is_empty() {
        &cfg.user
    } else {
        &cfg.from
    };
    let user = if cfg.user.is_empty() { from } else { &cfg.user };

    let body_with_headers = format!("From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\n\r\n{body}");

    let pw = smtp_password(cfg).unwrap_or_default();

    let mut cmd = Command::new("curl");
    cmd.args(["--silent", "--show-error", "--url", &url]);
    // Only require TLS when STARTTLS or implicit TLS is configured. With
    // `security = "plain"`, --ssl-reqd would force the connection to upgrade
    // and fail against plain SMTP relays.
    match cfg.security.as_str() {
        "plain" => {}
        _ => {
            cmd.arg("--ssl-reqd");
        }
    }
    if !user.is_empty() && !pw.is_empty() {
        cmd.arg("--user").arg(format!("{user}:{pw}"));
    }
    cmd.arg("--mail-from")
        .arg(from)
        .arg("--mail-rcpt")
        .arg(to)
        .arg("-T")
        .arg("-")
        .stdin(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn curl")?;
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(body_with_headers.as_bytes())?;
    }
    let status = child.wait().context("curl exited unexpectedly")?;
    if !status.success() {
        anyhow::bail!("curl exited {}", status.code().unwrap_or(-1));
    }
    Ok(())
}

fn redact_url(url: &str) -> String {
    if let Some(idx) = url.find('@') {
        // Slicing at byte index returned by `find` is safe because '@' is
        // ASCII and therefore always sits on a char boundary.
        return format!("***{}", &url[idx..]);
    }
    let chars: Vec<char> = url.chars().collect();
    if chars.len() > 12 {
        let head: String = chars.iter().take(8).collect();
        let tail: String = chars
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{head}…{tail}")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_with_credentials_in_url() {
        assert_eq!(
            redact_url("https://user:secret@example.com/hook"),
            "***@example.com/hook"
        );
    }

    #[test]
    fn redact_long_url_uses_char_boundaries() {
        // The URL contains 4-byte UTF-8 emoji glyphs in the path. Naive
        // byte-indexed slicing would panic; we must operate on chars.
        let url = "https://hooks.example.com/services/🎉🎉🎉/secret";
        let redacted = redact_url(url);
        assert!(redacted.starts_with("https://"));
        assert!(redacted.contains("…"));
    }

    #[test]
    fn short_url_returned_verbatim() {
        assert_eq!(redact_url("a/b"), "a/b");
    }
}
