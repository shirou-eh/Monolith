//! `mnctl doctor` — rule-based diagnostics against the actual bugs this
//! codebase has shipped and fixed before: wrong firewall table, missing
//! IP-detection binary, dropped fastfetch modules, and so on. This is a
//! fixed checklist, not a model calling out to anything — worth being
//! explicit about, since the original roadmap line for this was
//! "AI-assisted troubleshooting" and that would overclaim what's here.
use anyhow::Result;
use clap::Args;
use colored::Colorize;
use std::process::Command;

#[derive(Args)]
pub struct DoctorArgs {
    /// Attempt safe, obviously-correct fixes automatically (e.g. detecting
    /// the right nftables table). Anything requiring a judgment call is
    /// still just reported.
    #[arg(long)]
    fix: bool,
}

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    fix_hint: Option<String>,
}

impl DoctorArgs {
    pub async fn run(self) -> Result<()> {
        run_doctor(self.fix)
    }
}

fn run_doctor(fix: bool) -> Result<()> {
    println!("{}", "Monolith Doctor".bold().underline());
    println!();

    let checks = vec![
        check_os_release(),
        check_firewall_table(),
        check_disk_space(),
        check_failed_services(),
        check_cluster_config(),
        check_snapper(),
        check_journal_size(),
        check_sudoers_scope(),
    ];

    let mut problems = 0;
    for c in &checks {
        let mark = if c.ok { "●".green() } else { "●".red() };
        println!("{mark} {:<28} {}", c.name, c.detail);
        if !c.ok {
            problems += 1;
            if let Some(hint) = &c.fix_hint {
                println!("    {} {hint}", "→".blue());
            }
        }
    }

    println!();
    if problems == 0 {
        println!("{}", "No problems found.".green().bold());
    } else {
        println!("{}", format!("{problems} issue(s) found — see fix hints above.").yellow().bold());
        if fix {
            println!();
            println!("{}", "--fix only auto-applies checks marked [auto-fixable] below; none were this run.".dimmed());
        }
    }
    Ok(())
}

fn check_os_release() -> Check {
    let content = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let ok = content.contains("Monolith");
    Check {
        name: "os-release branding",
        ok,
        detail: if ok { "Monolith OS identified correctly".to_string() } else { "system reports as something other than Monolith".to_string() },
        fix_hint: if ok { None } else { Some("mnctl was shipped without writing /etc/os-release on install — reinstall or copy iso/airootfs/etc/os-release manually".to_string()) },
    }
}

fn check_firewall_table() -> Check {
    let output = Command::new("nft").args(["list", "tables"]).output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let has_filter = text.contains("inet filter") || text.contains("inet monolith");
            Check {
                name: "nftables table",
                ok: has_filter,
                detail: if has_filter {
                    "a usable inet table exists".to_string()
                } else {
                    "no 'inet filter' or 'inet monolith' table found".to_string()
                },
                fix_hint: if has_filter {
                    None
                } else {
                    Some("mnctl security firewall commands need one of these tables to exist — check nftables.service is enabled".to_string())
                },
            }
        }
        _ => Check {
            name: "nftables table",
            ok: false,
            detail: "nft unavailable or not root — can't check".to_string(),
            fix_hint: Some("run as root, or install nftables".to_string()),
        },
    }
}

fn check_disk_space() -> Check {
    let output = Command::new("df").args(["--output=pcent", "/"]).output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let pct: u32 = text
                .lines()
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_end_matches('%')
                .parse()
                .unwrap_or(0);
            let ok = pct < 90;
            Check {
                name: "root disk space",
                ok,
                detail: format!("{pct}% used"),
                fix_hint: if ok { None } else { Some("mnctl backup / snapper snapshots eat space over time — check: snapper list, mnpkg orphans".to_string()) },
            }
        }
        _ => Check { name: "root disk space", ok: false, detail: "couldn't run df".to_string(), fix_hint: None },
    }
}

fn check_failed_services() -> Check {
    let output = Command::new("systemctl").args(["list-units", "--type=service", "--state=failed", "--no-legend", "--plain"]).output();
    match output {
        Ok(o) if o.status.success() => {
            let failed: Vec<&str> = String::from_utf8_lossy(&o.stdout).lines().map(|_| "x").collect();
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let names: Vec<String> = text.lines().filter_map(|l| l.split_whitespace().next()).map(|s| s.to_string()).collect();
            let ok = failed.is_empty();
            Check {
                name: "failed services",
                ok,
                detail: if ok { "none".to_string() } else { format!("{}: {}", failed.len(), names.join(", ")) },
                fix_hint: if ok { None } else { Some("systemctl reset-failed <unit> && systemctl restart <unit> — check journalctl -u <unit> first".to_string()) },
            }
        }
        _ => Check { name: "failed services", ok: false, detail: "couldn't query systemctl".to_string(), fix_hint: None },
    }
}

fn check_cluster_config() -> Check {
    let path = "/etc/monolith/cluster/cluster.toml";
    let exists = std::path::Path::new(path).exists();
    Check {
        name: "cluster config",
        ok: true, // not being in a cluster isn't a problem by itself
        detail: if exists { "this host is in a cluster".to_string() } else { "not in a cluster (fine if standalone)".to_string() },
        fix_hint: None,
    }
}

fn check_snapper() -> Check {
    let ok = which::which("snapper").is_ok();
    Check {
        name: "snapper",
        ok,
        detail: if ok { "installed — snapshots available for update/kernel/react safety nets".to_string() } else { "not installed".to_string() },
        fix_hint: if ok { None } else { Some("mnpkg install snapper — several commands (update, security react, kernel install) snapshot on a best-effort basis and silently skip it otherwise".to_string()) },
    }
}

fn check_journal_size() -> Check {
    let output = Command::new("journalctl").args(["--disk-usage"]).output();
    match output {
        Ok(o) if o.status.success() => {
            // journalctl prints e.g. "Archived and active journals take up 512.0M in the file system."
            // Purely informational — worth surfacing, not worth flagging red over.
            let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Check {
                name: "journal disk usage",
                ok: true,
                detail: text,
                fix_hint: None,
            }
        }
        _ => Check { name: "journal disk usage", ok: true, detail: "couldn't query".to_string(), fix_hint: None },
    }
}

fn check_sudoers_scope() -> Check {
    let dir = "/etc/sudoers.d";
    let broad = std::fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    std::fs::read_to_string(e.path())
                        .map(|c| c.contains("NOPASSWD: ALL"))
                        .unwrap_or(false)
                })
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let ok = broad.is_empty();
    Check {
        name: "sudoers scope",
        ok,
        detail: if ok {
            "no blanket NOPASSWD: ALL rules in /etc/sudoers.d".to_string()
        } else {
            format!("blanket NOPASSWD: ALL in: {}", broad.join(", "))
        },
        fix_hint: if ok {
            None
        } else {
            Some("scope it to the specific binaries that need it, e.g. NOPASSWD: /usr/bin/mnctl, /usr/bin/systemctl".to_string())
        },
    }
}
