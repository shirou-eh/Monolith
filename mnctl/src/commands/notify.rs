//! Notification dispatch — webhook + SMTP + Telegram.
//!
//! Loads `[notifications]`, `[notifications.smtp]`, and
//! `[notifications.telegram]` from `/etc/monolith/monolith.toml` and
//! lets operators send ad-hoc messages or test the configured channels.
//!
//! ## 1.0.2 changes
//! - Added `Telegram` subcommand and `TelegramConfig` struct.
//! - `send_all` / `test_all` now fan out to Telegram when enabled.
//! - Fixed: SMTP transport swapped `subject` and `body` in the `Subject:`
//!   header when using msmtp. The `smtp_via_msmtp` function now writes
//!   headers in the correct order.
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
    /// Send a Telegram message via the configured bot (1.0.2)
    Telegram {
        /// Override the bot token (otherwise uses config)
        #[arg(long)]
        token: Option<String>,
        /// Override the chat ID (otherwise uses config)
        #[arg(long)]
        chat_id: Option<String>,
        /// Message text
        #[arg(long)]
        message: String,
        /// Run the interactive setup wizard
        #[arg(long)]
        setup: bool,
    },
    /// Print the loaded notifications config
    Show,
    /// Manage custom alert rules
    Rule(RuleArgs),
}

#[derive(Args)]
pub struct RuleArgs {
    #[command(subcommand)]
    command: RuleCommand,
}

#[derive(Subcommand)]
enum RuleCommand {
    /// Add a new alert rule
    Add {
        /// Alert name
        name: String,
        /// Condition: "cpu > 90% for 5m"
        #[arg(long)]
        if_: String,
        /// Notification channel: telegram, webhook, smtp
        #[arg(long)]
        send: String,
    },
    /// Remove an alert rule
    Remove {
        /// Rule name
        name: String,
    },
    /// List all alert rules
    List,
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
    #[serde(default)]
    telegram: TelegramConfig,
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

/// Telegram Bot API notification channel (added 1.0.2).
///
/// Sends messages to a chat or channel via
/// `https://api.telegram.org/bot<token>/sendMessage`. Requires a bot token
/// obtained from @BotFather and the numeric chat ID of the target conversation.
#[derive(Debug, Clone, Default, Deserialize)]
struct TelegramConfig {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    bot_token: String,
    /// Numeric chat ID, e.g. "-1001234567890" for a supergroup/channel.
    #[serde(default)]
    chat_id: String,
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
            NotifyCommand::Telegram {
                token,
                chat_id,
                message,
                setup,
            } => {
                if setup {
                    return telegram_setup().await;
                }
                let cfg = load_config().unwrap_or_default();
                let t = token.unwrap_or_else(|| cfg.telegram.bot_token.clone());
                let c = chat_id.unwrap_or_else(|| cfg.telegram.chat_id.clone());
                if t.is_empty() || c.is_empty() {
                    anyhow::bail!(
                        "Telegram bot_token and chat_id must be set in config or provided via flags"
                    );
                }
                telegram_send(&t, &c, &message).await
            }
            NotifyCommand::Show => show(),
            NotifyCommand::Rule(args) => args.run(),
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
    println!();
    println!("{}", "  Telegram:".bold());
    println!("    enabled:   {}", cfg.telegram.enabled);
    println!(
        "    bot_token: {}",
        if cfg.telegram.bot_token.is_empty() {
            "—".to_string()
        } else {
            redact_url(&cfg.telegram.bot_token)
        }
    );
    println!(
        "    chat_id:   {}",
        if cfg.telegram.chat_id.is_empty() {
            "—"
        } else {
            cfg.telegram.chat_id.as_str()
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
    if cfg.telegram.enabled && !cfg.telegram.bot_token.is_empty() {
        had_channel = true;
        match telegram_send(
            &cfg.telegram.bot_token,
            &cfg.telegram.chat_id,
            "🔔 *Monolith OS* — Telegram notification test\\. If you received this, the Telegram channel is working\\.",
        )
        .await
        {
            Ok(_) => println!("{} telegram OK (chat {})", "●".green(), cfg.telegram.chat_id),
            Err(e) => println!("{} telegram FAILED: {e}", "●".red()),
        }
    }
    if !had_channel {
        println!(
            "{}",
            "No channels configured. Edit /etc/monolith/monolith.toml to add a webhook, SMTP, or Telegram."
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
    if cfg.telegram.enabled && !cfg.telegram.bot_token.is_empty() {
        let msg = format!("*{subject}*\n{body}");
        telegram_send(&cfg.telegram.bot_token, &cfg.telegram.chat_id, &msg).await?;
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

/// Interactive Telegram setup wizard (1.0.2-p2).
///
/// Walks the user through creating a bot, fetching the chat_id, and
/// writing the config to /etc/monolith/monolith.toml.
async fn telegram_setup() -> Result<()> {
    println!("{}", "Telegram Notification Setup".bold().underline());
    println!();
    println!(
        "  {} Open https://t.me/BotFather and create a new bot.",
        "1.".bold()
    );
    println!(
        "  {} Paste the bot token you received from BotFather:",
        "2.".bold()
    );
    println!();

    let token: String = dialoguer::Input::new()
        .with_prompt("Bot token")
        .interact_text()?;

    if token.is_empty() {
        anyhow::bail!("no token provided");
    }

    println!();
    println!(
        "  {} Send any message to your bot, then press Enter.",
        "3.".bold()
    );

    let _: String = dialoguer::Input::new()
        .with_prompt("Press Enter after sending a message")
        .allow_empty(true)
        .interact_text()?;

    // Fetch updates to get the chat_id
    let url = format!("https://api.telegram.org/bot{token}/getUpdates");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .context("failed to reach Telegram API")?;

    let body: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse Telegram response")?;

    let chat_id = body["result"]
        .as_array()
        .and_then(|arr| arr.last())
        .and_then(|u| u["message"]["chat"]["id"].as_i64())
        .map(|id| id.to_string())
        .ok_or_else(|| {
            anyhow::anyhow!("could not detect chat_id. Did you send a message to the bot?")
        })?;

    println!("  {} Detected chat_id: {}", "✓".green(), chat_id.bold());

    // Test the configuration
    println!("  {} Sending test message...", "→".blue());
    let test_text = "🔔 *Monolith OS* — Telegram channel is working\\!";
    telegram_send(&token, &chat_id, test_text).await?;
    println!("  {} Test message sent — check your Telegram.", "✓".green());

    // Write to config
    let config_path = "/etc/monolith/monolith.toml";
    let config_dir = std::path::Path::new(config_path).parent().unwrap();
    std::fs::create_dir_all(config_dir)?;

    let content = if std::path::Path::new(config_path).exists() {
        std::fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut doc: toml::Value = content
        .parse::<toml::Value>()
        .unwrap_or(toml::Value::Table(toml::Table::new()));

    let telegram = serde_json::json!({
        "enabled": true,
        "bot_token": token,
        "chat_id": chat_id,
    });
    let telegram_toml: toml::Value =
        toml::from_str(&serde_json::to_string_pretty(&telegram).unwrap()).unwrap();

    if let Some(notifications) = doc
        .as_table_mut()
        .and_then(|t| t.get_mut("notifications"))
        .and_then(|n| n.as_table_mut())
    {
        notifications.insert("telegram".to_string(), telegram_toml);
    } else {
        let mut notifications = toml::Table::new();
        notifications.insert("telegram".to_string(), telegram_toml);
        doc.as_table_mut().unwrap().insert(
            "notifications".to_string(),
            toml::Value::Table(notifications),
        );
    }

    let serialized = toml::to_string_pretty(&doc)?;
    std::fs::write(config_path, &serialized)?;

    println!();
    println!(
        "{} Config written to {} [notifications.telegram]",
        "✓".green(),
        config_path
    );
    println!("  {} enabled = true", "●".green());
    println!("  {} bot_token = <redacted>", "●".green());
    println!("  {} chat_id = {}", "●".green(), chat_id);

    Ok(())
}

/// Send a message through the Telegram Bot API (added 1.0.2).
///
/// Uses `sendMessage` with `parse_mode=MarkdownV2`. The caller is responsible
/// for escaping any user-supplied text with `escape_md` if needed.
async fn telegram_send(token: &str, chat_id: &str, text: &str) -> Result<()> {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let payload = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "MarkdownV2",
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .context("failed to reach Telegram API")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Telegram API returned {status}: {body}");
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
        // FIX (1.0.2): headers were written Subject then From/To, but
        // the RFC 5322 writer accidentally emitted `body` in the Subject
        // field and `subject` in the body. Correct order below.
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

impl RuleArgs {
    pub fn run(self) -> Result<()> {
        match self.command {
            RuleCommand::Add { name, if_, send } => rule_add(&name, &if_, &send),
            RuleCommand::Remove { name } => rule_remove(&name),
            RuleCommand::List => rule_list(),
        }
    }
}

fn rule_add(name: &str, condition: &str, channel: &str) -> Result<()> {
    let rules_dir = "/etc/monolith/alert-rules";
    std::fs::create_dir_all(rules_dir)?;

    let rule = format!(
        "# Alert rule: {name}\n# Condition: {condition}\n# Channel: {channel}\n[rule]\nname = \"{name}\"\ncondition = \"{condition}\"\nchannel = \"{channel}\"\nenabled = true\n"
    );

    let path = format!("{rules_dir}/{name}.toml");
    std::fs::write(&path, &rule)?;

    println!("  {} Alert rule '{}' added", "●".green(), name.bold());
    println!("     If: {condition}");
    println!("     Send via: {channel}");
    Ok(())
}

fn rule_remove(name: &str) -> Result<()> {
    let path = format!("/etc/monolith/alert-rules/{name}.toml");
    if std::path::Path::new(&path).exists() {
        std::fs::remove_file(&path)?;
        println!("  {} Rule '{}' removed", "●".green(), name);
    } else {
        anyhow::bail!("rule '{}' not found", name);
    }
    Ok(())
}

fn rule_list() -> Result<()> {
    let rules_dir = "/etc/monolith/alert-rules";
    let path = std::path::Path::new(rules_dir);

    if !path.exists() {
        println!("{}", "No alert rules configured.".dimmed());
        return Ok(());
    }

    println!("{}", "Alert Rules:".bold().underline());
    for entry in std::fs::read_dir(path).context("failed to read rules directory")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".toml") {
            let content = std::fs::read_to_string(entry.path()).unwrap_or_default();
            let condition = content
                .lines()
                .find(|l| l.contains("condition ="))
                .map(|l| l.trim_start_matches("condition = ").trim_matches('"'))
                .unwrap_or("?");
            let channel = content
                .lines()
                .find(|l| l.contains("channel ="))
                .map(|l| l.trim_start_matches("channel = ").trim_matches('"'))
                .unwrap_or("?");
            let enabled = content.contains("enabled = true");
            let indicator = if enabled {
                "●".green()
            } else {
                "●".dimmed()
            };
            println!(
                "  {indicator} {:<20} if {:<30} → {}",
                name.trim_end_matches(".toml").bold(),
                condition,
                channel
            );
        }
    }
    Ok(())
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
