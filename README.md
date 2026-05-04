<div align="center">

```
███╗   ███╗ ██████╗ ███╗   ██╗ ██████╗ ██╗     ██╗████████╗██╗  ██╗
████╗ ████║██╔═══██╗████╗  ██║██╔═══██╗██║     ██║╚══██╔══╝██║  ██║
██╔████╔██║██║   ██║██╔██╗ ██║██║   ██║██║     ██║   ██║   ███████║
██║╚██╔╝██║██║   ██║██║╚██╗██║██║   ██║██║     ██║   ██║   ██╔══██║
██║ ╚═╝ ██║╚██████╔╝██║ ╚████║╚██████╔╝███████╗██║   ██║   ██║  ██║
╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝ ╚═════╝ ╚══════╝╚═╝   ╚═╝   ╚═╝  ╚═╝
```

# **Monolith OS** &middot; *Built for the ones who mean it.*

A lean, server-focused Linux distribution built on Arch Linux with a custom kernel, unified CLI, web management UI, integrated monitoring, security hardening, Kubernetes (k3s), a custom ISO builder, plugin system, and zero-config application deployment.

[![License: MIT](https://img.shields.io/badge/License-MIT-1f2937?style=flat-square&labelColor=0b1220)](LICENSE)
[![Release: v1.0.1](https://img.shields.io/badge/Release-v1.0.1%20Obsidian-35e0a1?style=flat-square&labelColor=0b1220)](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1)
[![Built with Rust](https://img.shields.io/badge/Rust-1.95+-5cc9ff?style=flat-square&labelColor=0b1220&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Based on Arch](https://img.shields.io/badge/Base-Arch%20Linux-1793d1?style=flat-square&labelColor=0b1220&logo=archlinux&logoColor=white)](https://archlinux.org)
[![Profiles: lite · full · pro](https://img.shields.io/badge/Profiles-lite%20%C2%B7%20full%20%C2%B7%20pro-a78bfa?style=flat-square&labelColor=0b1220)](#system-profiles)

[Download&nbsp;ISOs](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1) &middot;
[Quick&nbsp;Start](#quick-start) &middot;
[Command&nbsp;Reference](#command-reference) &middot;
[Architecture](#architecture) &middot;
[Docs](docs/en/README.md) &middot;
[Contribute](CONTRIBUTING.md)

</div>

---

## Why Monolith OS?

Most distros are general-purpose. Monolith is single-purpose — **run servers, run them well, and stop fighting the box.** Everything you need on a typical server (firewall, monitoring, containers, VPN, Kubernetes, backups, reverse proxy, app templates, hardware health) is wired in from boot. One CLI, one config file, one web UI. No 30-package install dance, no half-configured systemd timers, no sprawl.

- **One tool for the whole box** &mdash; `mnctl` is a 50-subcommand multi-call CLI. Services, containers, deploys, k3s, VPN, ISO builds — same verbs, same flags.
- **Three surfaces, one brand** &mdash; the web dashboard (`mnweb`), the TUI (`mntui`), and the installer all share the same emerald + cyan aurora palette and the same data model.
- **Snapshot-safe upgrades** &mdash; every `mnpkg upgrade` takes a Btrfs snapshot via snapper *before* touching the system. One command rolls it back.
- **Tier-aware** &mdash; ship `lite` to a 512 MB VPS, `pro` to a k3s node, same image family.
- **Zero-config app templates** &mdash; `mnctl template deploy minecraft --name pvp` and you have a hardened container with a reverse proxy, TLS, backups, and Prometheus targets attached. Same flow for CS2, Postgres, Redis, Discord/Telegram bots, static sites.
- **Built locally, downloadable today** &mdash; three TTY-only x86_64 ISOs are attached to [v1.0.1 Obsidian](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1), SHA-256 verified.

---

## Table of Contents

1. [Highlights of v1.0.1 "Obsidian"](#highlights-of-v101-obsidian)
2. [Download an ISO](#download-an-iso)
3. [Quick Start](#quick-start)
   - [Boot from the ISO](#-boot-from-the-iso-recommended)
   - [Convert an existing Arch install](#-convert-an-existing-arch-install)
   - [Build from source](#-build-from-source)
4. [First Steps After Install](#first-steps-after-install)
5. [System Profiles](#system-profiles)
6. [The Three Surfaces](#the-three-surfaces)
   - [`mnctl` &mdash; the CLI](#mnctl--the-cli)
   - [`mntui` &mdash; the dashboard TUI](#mntui--the-dashboard-tui)
   - [`mnweb` &mdash; the web UI](#mnweb--the-web-ui)
   - [`monolith-installer` &mdash; the TUI installer](#monolith-installer--the-tui-installer)
   - [`mnpkg` &mdash; the package manager wrapper](#mnpkg--the-package-manager-wrapper)
7. [Cookbook](#cookbook)
8. [Application Templates](#application-templates)
9. [Monitoring Stack](#monitoring-stack)
10. [Security Hardening](#security-hardening)
11. [Backups](#backups)
12. [Custom Kernel](#custom-kernel)
13. [Plugin System](#plugin-system)
14. [Architecture](#architecture)
15. [Command Reference](#command-reference)
16. [Building Your Own ISO](#building-your-own-iso)
17. [Configuration File](#configuration-file)
18. [Troubleshooting & FAQ](#troubleshooting--faq)
19. [Contributing](#contributing)
20. [Roadmap](#roadmap)
21. [License & Credits](#license--credits)

---

## Highlights of v1.0.1 "Obsidian"

- **Brand-wide UI redesign** across `mnweb`, `mntui`, the installer, and `mnctl info version`. Aurora gradients, glass-morphism cards, sparkline graphs, animated meters, rounded panels, accent-coloured gauges &mdash; one cohesive visual identity.
- **Three TTY-only ISO variants** (lite / full / pro) attached to the [v1.0.1 release](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1). No Xorg, no Wayland, no DE &mdash; this is a server distro.
- **Tier-aware installer** that bakes the matching `[system].profile` into `/etc/monolith/monolith.toml` so `mnctl profile show` on first boot reports the correct tier.
- **Snapshot-safe `mnpkg`** &mdash; integrated AUR support, snapper rollback, and CVE auditing.
- **Quality-of-life fixes** &mdash; correct CPU colour bands in the TUI, install gauge always reaches 100% on the Complete screen, throttled web-UI polling (~80 % less API chatter).
- **Hardened ISO build pipeline** &mdash; the ISO builder now overlays the upstream `archiso` releng baseline so `mkarchiso` always has the boot-loader configs it needs.

See the [full changelog](CHANGELOG.md) for everything else.

---

## Download an ISO

Three pre-built x86_64 ISOs are attached to [v1.0.1 Obsidian](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1). Pick the one that matches your hardware. **All three boot to a TTY &mdash; there is no graphical session shipped on disk.**

| ISO | Size | Best for | Default profile |
|-----|------|----------|-----------------|
| [`monolith-1.0.1-lite-x86_64.iso`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-1.0.1-lite-x86_64.iso) | ~1.25 GB | Discord-bot VPS, microservers, edge boxes | `lite` |
| [`monolith-1.0.1-full-x86_64.iso`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-1.0.1-full-x86_64.iso) | ~1.25 GB | Home servers, default install | `full` |
| [`monolith-1.0.1-pro-x86_64.iso`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-1.0.1-pro-x86_64.iso) | ~1.25 GB | Production / k3s clusters | `pro` |

Each ISO ships `mnctl`, `mnpkg`, `mntui`, `mnweb`, and `monolith-installer` pre-vendored under `/usr/local/bin/` so the live medium is fully usable.

### Verify the download

```bash
sha256sum -c monolith-1.0.1-full-x86_64.iso.sha256
```

Pre-computed checksums live next to each ISO on the [release page](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1).

### Write to USB

```bash
sudo dd if=monolith-1.0.1-full-x86_64.iso of=/dev/sdX bs=4M status=progress conv=fsync
```

> **Heads-up:** `dd` is destructive. Triple-check `/dev/sdX` &mdash; ideally with `lsblk` immediately before and after plugging the stick.

You can also flash with [balenaEtcher](https://www.balena.io/etcher/) or [Ventoy](https://ventoy.net/) on Windows / macOS.

### Don't want to use the ISO?

The release also ships [`monolith-x86_64-unknown-linux-gnu.tar.gz`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-x86_64-unknown-linux-gnu.tar.gz) &mdash; a 3.6 MB tarball with all five Monolith binaries (`mnctl`, `mnpkg`, `mntui`, `mnweb`, `monolith-installer`) for users who want to upgrade an existing install without re-imaging.

---

## Quick Start

### &#9654; Boot from the ISO (recommended)

1. **Download** the ISO that matches your hardware (see [above](#download-an-iso)).
2. **Verify** the checksum.
3. **Write** to a USB stick.
4. **Boot** from the stick. The live system auto-launches `monolith-installer` on `tty1`.
5. The installer walks you through:
   - Disk selection & partitioning
   - Hostname & timezone
   - Root password & primary user (sudo + Monolith group)
   - Network config (DHCP or static)
   - Profile pick (`lite` / `full` / `pro`)
   - Packages & first-boot setup (firewall, SSH, monitoring opt-in, etc.)
6. **Reboot** &mdash; on first login, `monolith-firstboot` runs once to finalise systemd units and seed defaults.

### &#9654; Convert an existing Arch install

If you already run Arch Linux on a server and don't want to re-image:

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

This script:

- Installs the five Monolith binaries into `/usr/local/bin/`
- Drops the default config at `/etc/monolith/monolith.toml` (auto-detects a sensible profile)
- Enables `mnweb` and `mntui` systemd units
- Adds the Monolith pacman hooks (snapshot-before-upgrade, AUR helper)
- Optionally rolls out the security defaults (firewall, SSH, AppArmor) when you pass `--secure`

You can run it on a VPS, a home server, or in a container &mdash; it does not assume any particular hardware.

### &#9654; Build from source

Requires Rust **1.95+** (stable) and Arch Linux for the ISO bits.

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd Monolith
make build           # cargo build --release across the workspace
sudo make install    # copies binaries + units + config defaults
```

Or run any individual binary out-of-tree:

```bash
cargo run --release --bin mnctl -- info system
cargo run --release --bin mntui
cargo run --release --bin mnweb         # listens on 127.0.0.1:9911
cargo run --release --bin monolith-installer  # demo / dry-run friendly
```

---

## First Steps After Install

After your first boot, run these in order:

```bash
mnctl info system          # Distro / kernel / hardware overview
mnctl profile show         # Confirms the active resource profile
mnctl monitor status       # CPU / RAM / disk / network at a glance
mnctl security audit       # Quick security posture check
mnctl template list        # See the available app templates
mntui                      # Open the TUI dashboard (q to quit)
```

To open the **web UI** from your laptop (the dashboard lives at `127.0.0.1:9911` by default):

```bash
ssh -L 9911:127.0.0.1:9911 user@your-server   # SSH tunnel from your laptop
# then open http://127.0.0.1:9911 in your browser
```

To deploy your first app:

```bash
mnctl template deploy minecraft --name mc-survival
mnctl service status monolith-app-mc-survival.service
mnctl proxy add mc-survival.example.com --service mc-survival --tls
```

---

## System Profiles

Monolith ships three resource profiles. Each ISO defaults to one, but you can switch at any time after install with `mnctl profile set <tier>`.

| Profile | CPU      | RAM     | Disk   | Default-on services                                       | Use case                                |
|---------|----------|---------|--------|-----------------------------------------------------------|------------------------------------------|
| `lite`  | 1 vCPU   | 512 MB  | 8 GB   | `mnweb` (off), monitoring (off)                            | Discord bots, microservers, edge boxes   |
| `full`  | 2 cores  | 2 GB    | 20 GB  | `mnweb`, monitoring, container runtime                     | Single-node home server, side projects   |
| `pro`   | 4+ cores | 8+ GB   | 100 GB | Everything `full` has + `k3s`, `etcd`-backed cluster mode  | Production cluster, k3s nodes            |

Both `x86_64` and `ARM64` are supported in source builds. The `lite` ISO trims the monitoring stack and other heavy packages from the live medium so the download stays under ~1.3 GB.

```bash
mnctl profile show          # Print the current profile + thresholds
mnctl profile list          # List all available profiles
mnctl profile set pro       # Switch to pro (writes /etc/monolith/monolith.toml)
```

---

## The Three Surfaces

Monolith exposes the same data through a CLI, a TUI, and a web UI. They all read the same `/etc/monolith/monolith.toml`, talk to the same JSON API on `mnweb`, and share the same emerald-and-cyan brand palette so the product feels coherent end-to-end.

### `mnctl` &mdash; the CLI

The everything-CLI. 25 top-level commands, ~100 subcommands. Same structure as Git: `mnctl <noun> <verb> [args]`.

```bash
mnctl service list                 # All systemd units, color-coded by state
mnctl container stats              # Live Docker/Podman resource usage
mnctl deploy app https://...       # Deploy from a Docker Compose URL
mnctl monitor top                  # ratatui top, with hotkeys
mnctl backup create --tag pre-upgrade
mnctl kube install --role server
mnctl iso build --tier lite
```

`mnctl info version` prints a branded ANSI banner with the build SHA, target triple, and active profile &mdash; nice for support tickets.

### `mntui` &mdash; the dashboard TUI

A ratatui dashboard for SSH sessions where opening a browser is overkill. Real-time:

- CPU sparkline + per-core gauges
- RAM / swap meters (emerald → amber → red as load rises)
- Disk usage with I/O rates
- Top processes with tier-aware CPU colouring (yellow > 50 %, red > 80 %)
- Network throughput per interface
- Service health pills (`active` / `failed` / `inactive`)
- Hotkeys: `q` quit, `1..6` switch tabs, `↑/↓` scroll process list

Launch with `mntui` (or `mnctl monitor dashboard`).

### `mnweb` &mdash; the web UI

A single-binary embedded SPA + JSON API. Hosted by Axum on `127.0.0.1:9911` by default, locked down to localhost. Tunnel over SSH or put it behind `mnctl proxy` with TLS.

- **Overview** &mdash; large metric cards, animated 60-sample CPU sparkline, RAM/swap gauges, load average, system-wide quick-glance counters (services, containers, disks, templates).
- **Services / Containers / Disks / Templates** tabs with searchable, sortable tables.
- **Logs** tab with live tail and a filter bar.
- Hash-based tab routing (`#overview`, `#services`, …), skeleton loaders, `prefers-reduced-motion` honored.
- API surface: `GET /api/overview`, `/api/services`, `/api/containers`, `/api/disks`, `/api/templates`, `/api/logs/<unit>` &mdash; all return JSON.

Enable on demand:

```bash
mnctl web enable      # systemctl enable --now monolith-mnweb.service
mnctl web url         # Print the URL (and an SSH-tunnel hint)
```

### `monolith-installer` &mdash; the TUI installer

A ratatui installer with per-step glyphs, a brand bar, a live install gauge, and a final summary panel. It runs automatically on `tty1` of every live ISO. Steps:

1. Welcome / language
2. Disk selection (with current partition table and SMART warning if applicable)
3. Hostname & timezone
4. Root password & primary user
5. Network (DHCP or static, with live link-state preview)
6. Profile pick (`lite` / `full` / `pro`)
7. Optional security hardening
8. Optional monitoring stack opt-in
9. Confirm
10. Install (live progress gauge, package counter, log tail)
11. Complete (summary, reboot prompt)

You can also run it on a converted Arch system to re-bootstrap configs without touching `/`.

### `mnpkg` &mdash; the package manager wrapper

A pacman-compatible CLI that adds:

- **Snapshot-before-upgrade** &mdash; every `mnpkg upgrade` calls `snapper create --description 'pre-upgrade'` first; rollback is one command.
- **AUR support** &mdash; transparent fallback to an AUR helper when a package isn't in the official repos.
- **CVE audit** &mdash; `mnpkg audit` queries arch-audit and surfaces affected installed packages.
- **Familiar verbs** &mdash; `install`, `remove`, `search`, `info`, `list-installed`, `clean`, plus `rollback <snapshot>`.

```bash
mnpkg install nginx
mnpkg upgrade            # Snapshots, then -Syu
mnpkg audit              # CVE check
mnpkg rollback latest    # Roll back the last upgrade
```

---

## Cookbook

Common operations, copy-pasteable. Each recipe is a one-liner; `mnctl --help` and `mnctl <verb> --help` cover the rest.

```bash
# --- Day 1 ---
mnctl info system && mnctl profile show
mnctl security audit
mnctl monitor status

# --- Containers / app deploys ---
mnctl template list
mnctl template deploy minecraft --name mc-pvp
mnctl deploy app /opt/my-compose-repo --name api
mnctl container logs api --follow
mnctl container stats

# --- Reverse proxy with automatic TLS ---
mnctl proxy add api.example.com --service monolith-app-api.service --tls
mnctl proxy ssl --renew

# --- Kubernetes (k3s) ---
mnctl kube install --role server
mnctl kube token
mnctl kube install --role agent --server https://control:6443 --token <TOKEN>
mnctl kube nodes
mnctl kube kubectl get pods -A

# --- Backups ---
mnctl backup create --tag weekly
mnctl backup list
mnctl backup restore --snapshot <ID>
mnctl backup verify

# --- Disk health ---
mnctl disk list
mnctl disk smart status
mnctl disk smart attributes /dev/sda
mnctl disk smart test --type short /dev/sda

# --- VPN ---
mnctl vpn create wg0
mnctl vpn peer add wg0 --pubkey <K> --allowed-ips 10.0.0.2/32
mnctl vpn status

# --- Updates ---
mnctl update check
mnctl update apply --reboot-if-needed
mnctl update rollback   # snapper-backed

# --- Notifications ---
mnctl notify webhook --url https://hooks.example.com/abc --on warning,error
mnctl notify test
```

---

## Application Templates

Every template lives under `templates/<name>/` and ships a Docker Compose file plus a `template.yaml` manifest with environment, ports, and proxy hints. Deploy any of them with one command:

```bash
mnctl template deploy <name> --name <instance> [--var KEY=VALUE ...]
```

Currently shipping:

| Category    | Templates                                                                  |
|-------------|----------------------------------------------------------------------------|
| Game servers | `minecraft`, `cs2`, `valheim`, `palworld`                                  |
| Databases   | `postgresql`, `mariadb`, `mongodb`, `redis`                                |
| Bots        | `discord-bot-js` (discord.js), `discord-bot-python` (discord.py), `telegram-bot` |
| Web         | `nodejs-app`, `static-site`, `nginx-reverse-proxy`                         |
| Misc        | (more on the [roadmap](ROADMAP.md))                                        |

A deploy automatically:

1. Pulls the image and starts the container in a `monolith-app-<name>` systemd unit.
2. Wires it into Prometheus targets if monitoring is on.
3. Adds backup hooks if the template declares persistent volumes.
4. (Optionally) registers a reverse-proxy entry with TLS via `mnctl proxy add`.

---

## Monitoring Stack

When `[monitoring].enabled = true` (default on `full` and `pro`), Monolith brings up:

- **Prometheus** &mdash; scrapes `mnweb` for system metrics and any deployed templates that publish `/metrics`. Alert rules under `monitoring/prometheus/alerts.yml` cover CPU saturation, memory pressure, disk fill rate, container restarts, and SMART warnings.
- **Grafana** &mdash; pre-configured datasources and dashboards (System Overview, Containers, Disks, Network).
- **Loki** &mdash; aggregates `journald` and container logs; queryable from Grafana.

Quick commands:

```bash
mnctl monitor status              # All three pieces at a glance
mnctl monitor metrics --top 10    # Top series by churn
mnctl monitor alerts              # Active alert pings
mnctl monitor dashboard           # Opens mntui pinned to monitoring tab
```

The `lite` profile leaves all three off &mdash; turn them on with `mnctl monitor enable` if/when you have headroom.

---

## Security Hardening

Monolith ships defense-in-depth defaults:

- **Firewall** &mdash; `nftables` with default-deny, only SSH (port `2222`) open. Inbound rules opened per-service when you `mnctl proxy add` or `mnctl service create --port`.
- **SSH** &mdash; key-only auth, no root login, modern ciphers, alternate port `2222`.
- **AppArmor** &mdash; mandatory access-control profiles for nginx, Docker, redis, postgres, etc. (under `security/apparmor/`).
- **Fail2ban** &mdash; auto-bans IPs after failed SSH or proxy auth attempts.
- **Kernel hardening** &mdash; sysctl tuning for ASLR, ptrace restriction, kernel.dmesg_restrict, network spoofing protections.
- **CVE auditing** &mdash; `mnctl security cve-check` and `mnpkg audit` use arch-audit on demand and weekly via a systemd timer.
- **TLS everywhere** &mdash; `mnctl proxy` integrates with Let's Encrypt out of the box.

```bash
mnctl security audit              # Pass/fail summary across all categories
mnctl security firewall list
mnctl security harden             # Apply all defaults to a converted Arch box
mnctl security cve-check
```

The full security configuration lives under `security/` and is fully transparent &mdash; nothing is patched into closed binaries.

---

## Backups

Two-tier, by default:

- **Local** &mdash; Btrfs snapshots via `snapper`. Used by `mnpkg upgrade` for instant rollback. Hourly / daily / weekly retention.
- **Remote** &mdash; `restic` backed by the SSH or S3 backend of your choice. Encrypts everything client-side.

```bash
mnctl backup snapshots                    # Local snapper view
mnctl backup create --tag pre-migration   # Snapshot + restic push
mnctl backup verify                       # restic check
mnctl backup restore --snapshot <ID>
mnctl backup export --path /path/to/file  # One-off file pull from a snapshot
```

Backup config lives in `/etc/monolith/monolith.toml` under `[backup]`.

---

## Custom Kernel

Monolith optionally ships a custom kernel build (`kernel/build.sh`) tuned for servers:

- **BORE scheduler** &mdash; better latency for interactive workloads under load.
- **BBR3** congestion control.
- **WireGuard** built in (no module load).
- **Strict KASLR**, hardened slab allocator, init-on-alloc.
- **Both x86_64 and ARM64 configs** under `kernel/configs/`.

You can install the custom kernel post-install:

```bash
sudo bash kernel/build.sh --tier full
sudo mnctl update kernel --custom
```

The default ISO ships the upstream `linux` kernel for compatibility &mdash; the custom kernel is opt-in.

---

## Plugin System

`mnctl plugin` lets you extend the CLI without rebuilding. Drop any executable named `mnctl-<name>` into `~/.local/share/mnctl/plugins/` (user) or `/usr/local/lib/mnctl/plugins/` (system) and it shows up as `mnctl <name>`.

```bash
mnctl plugin list
mnctl plugin install /path/to/mnctl-myplug
mnctl plugin run myplug -- --help
```

Stdin / stdout / argv are forwarded transparently. Plugins inherit `MNCTL_CONFIG` and `MNCTL_PROFILE` env vars so they can read the same config the host CLI uses.

---

## Architecture

```
Monolith/
├── mnctl/              # Main CLI tool (Rust)
│   ├── src/
│   │   ├── commands/   # 25 command modules
│   │   └── tui/        # mntui dashboard
│   └── config_default.toml
├── mnpkg/              # Package manager wrapper (Rust)
├── mnweb/              # Web UI server (Rust + Axum)
│   ├── src/
│   └── static/         # SPA: index.html · app.css · app.js
├── installer/          # TUI installer (Rust + ratatui)
├── kernel/             # Custom kernel configs & build script
│   ├── configs/        # x86_64 and ARM64 configs
│   ├── patches/        # BORE, BBR3, etc.
│   └── build.sh
├── iso/                # ISO builder
│   ├── airootfs/       # Files merged into the live medium
│   ├── profile/        # archiso profile (packages, profiledef)
│   └── build-iso.sh    # Wraps mkarchiso, supports --tier lite|full|pro
├── security/           # Security configurations
│   ├── nftables/
│   ├── apparmor/
│   ├── ssh/
│   ├── sysctl/
│   └── limits/
├── monitoring/         # Monitoring stack
│   ├── prometheus/
│   ├── grafana/
│   └── loki/
├── backup/             # Snapper + restic configs
├── templates/          # Docker Compose app templates
│   ├── minecraft/  cs2/  valheim/  palworld/
│   ├── postgresql/ mariadb/ mongodb/ redis/
│   ├── nodejs-app/ static-site/ nginx-reverse-proxy/
│   ├── discord-bot-js/  discord-bot-python/  telegram-bot/
│   └── …
├── config/             # System configs (pacman, monolith.toml, systemd units)
├── scripts/            # install.sh, conversion helpers
├── docs/               # Multi-language documentation (en/ru/zh/es)
└── .github/            # CI/CD workflows (rust.yml, release.yml)
```

The four Rust crates form a Cargo workspace at the repo root. `cargo build --workspace` builds every binary; `make build` is just a thin alias.

---

## Command Reference

```text
mnctl
├── service              List, start, stop, restart, enable, disable,
│                         status, logs, edit, create
├── container            List, start, stop, restart, logs, exec,
│                         stats, inspect, pull, images, prune, compose
├── deploy               app, list, status, update, remove
├── monitor              status, top, services, network, disk, logs,
│                         alerts, metrics, dashboard
├── security             audit, firewall, apparmor, fail2ban,
│                         cve-check, integrity, harden
├── update               check, apply, kernel, rollback, history, schedule
├── backup               create, list, restore, verify, delete,
│                         snapshots, export
├── network              status, interfaces, dns, routes, test
├── vpn                  create, list, connect, disconnect, status, peer
├── proxy                list, add, remove, ssl, reload
├── cluster              init, join, leave, nodes, status, sync, deploy
├── bench                cpu, memory, disk, network, all, compare
├── template             list, deploy, info
├── info                 system, hardware, version
├── config               show, set, edit, validate
├── disk                 list, usage, io, smart {status|attributes|test|log|watch}, nvme
├── kube                 install, uninstall, status, nodes, pods,
│                         apply, token, kubeconfig, kubectl
├── plugin               list, info, path, install, remove, run
├── profile              show, list, set
├── notify               test, send, webhook, email, show
├── iso                  build, doctor, profile-path
└── web                  run, enable, disable, status, url
```

`mnctl --help` and `mnctl <verb> --help` are exhaustive.

---

## Building Your Own ISO

You don't need a CI runner &mdash; the same builder used to ship the official ISOs is in the repo.

```bash
# Build all three tiers locally (requires Docker or an Arch host)
make iso TIER=lite
make iso TIER=full
make iso TIER=pro

# Or call the script directly
sudo ./iso/build-iso.sh --tier full --out ./out --version 1.0.1
```

The script:

1. Lays down the upstream `archiso` releng baseline so `mkarchiso` always has its boot-loader configs.
2. Overlays the Monolith profile (`iso/profile/packages.x86_64`, `profiledef.sh`).
3. Bakes the chosen `[system].profile` into the ISO's `/etc/monolith/monolith.toml`.
4. Trims heavy packages on `lite`.
5. Vendors the release tarball into `airootfs/usr/local/bin/` so the live system has the binaries ready to run.
6. Calls `mkarchiso` to produce the final ISO.

Inside an Arch container:

```bash
docker run --rm --privileged -v "$(pwd):/work" -w /work archlinux:latest bash -c '
  pacman -Syu --noconfirm
  pacman -S --noconfirm --needed archiso grub edk2-shell libisoburn squashfs-tools dosfstools mtools
  ./iso/build-iso.sh --tier full --out out --version 1.0.1
'
```

Output lands at `./out/monolith-1.0.1-full-x86_64.iso`.

---

## Configuration File

Everything lives in `/etc/monolith/monolith.toml`. The default file (`mnctl/config_default.toml`) is heavily commented; the snippet below is a representative excerpt:

```toml
# Monolith OS Configuration
# https://github.com/shirou-eh/Monolith
# Version: 1.0.1 "Obsidian"

[system]
profile  = "full"         # lite | full | pro
hostname = "monolith"
timezone = "UTC"

[mnweb]
enabled = true
bind    = "127.0.0.1:9911"
auth    = "token"          # token | none — token is read from $MNWEB_TOKEN_FILE

[monitoring]
enabled    = true
prometheus = { port = 9090, retention = "30d" }
grafana    = { port = 3000, admin_user = "admin" }
loki       = { port = 3100 }

[security]
firewall_default_policy = "drop"
ssh_port                = 2222
fail2ban                = true

[backup]
snapper = { schedule = "hourly", keep_hourly = 24, keep_daily = 7, keep_weekly = 4 }
restic  = { repo = "sftp:backup@nas:/backups/monolith", schedule = "daily" }

[notify]
webhook = { url = "", min_severity = "warning" }
email   = { smtp = "smtp.example.com:587", from = "monolith@example.com" }
```

Run `mnctl config validate` after editing &mdash; it parses the file and surfaces missing keys or type mismatches before you reload the units.

---

## Troubleshooting & FAQ

<details>
<summary><b>The installer can't see my disk.</b></summary>

The live ISO loads `nvme`, `ahci`, `virtio_blk`, and `mpt3sas` by default. If you boot on exotic storage (some HBAs, RAID controllers), drop to a shell with **Ctrl+Alt+F2**, `modprobe <driver>`, then re-run `monolith-installer`.
</details>

<details>
<summary><b>I picked the wrong profile.</b></summary>

```bash
sudo mnctl profile set <tier>
sudo systemctl restart monolith-mnweb.service
```

Your data is untouched &mdash; only `/etc/monolith/monolith.toml` and unit defaults change.
</details>

<details>
<summary><b>How do I open <code>mnweb</code> from another machine?</b></summary>

`mnweb` only binds to localhost by design. Two safe options:

```bash
# Option A: SSH tunnel (preferred)
ssh -L 9911:127.0.0.1:9911 user@your-server

# Option B: put it behind mnctl proxy with TLS
sudo mnctl proxy add monolith.example.com --service monolith-mnweb.service --tls
```

Don't bind 0.0.0.0 directly &mdash; the API exposes systemd unit control.
</details>

<details>
<summary><b><code>mnpkg upgrade</code> failed half-way.</b></summary>

```bash
sudo mnpkg rollback latest
sudo systemctl reboot
```

The pre-upgrade snapper snapshot rolls the system back to the last known-good state &mdash; no manual `pacman` surgery required.
</details>

<details>
<summary><b>Where do logs live?</b></summary>

- Per-unit: `journalctl -u <unit>`
- Container: `mnctl container logs <name>` (with `--follow`)
- Aggregated (if Loki is on): Grafana → Explore
- Installer: `/var/log/monolith-install.log`
</details>

<details>
<summary><b>Can I run Monolith inside a container?</b></summary>

Yes &mdash; the install script (`scripts/install.sh`) detects containerised environments and skips kernel-level steps (sysctl tuning, AppArmor, snapper). `mnctl service`, `mnctl container`, `mnweb`, and `mntui` all work. Useful for CI smoke tests of templates.
</details>

<details>
<summary><b>How big is each ISO and what's the difference between them?</b></summary>

All three are ~1.25 GB. Same kernel, same installer, same Monolith binaries. The differences:

- `lite` &mdash; **trimmed package list** (no Prometheus / Grafana / Loki / smartmontools / nvme-cli) and the `lite` profile baked into `/etc/monolith/monolith.toml`.
- `full` &mdash; full package list, `full` profile baked in.
- `pro` &mdash; same package list as `full`, `pro` profile baked in (which enables k3s and the cluster mode).
</details>

---

## Contributing

Pull requests welcome. The bar is:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build --workspace --release`

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the full flow (branch naming, commit messages, the "Conventional Commits" we use, the PR template).

For larger features, open a discussion or an issue first &mdash; especially if it touches the kernel, the security defaults, or the on-disk config layout.

---

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the running list. Highlights of what's queued:

- ARM64 ISOs (the source already builds; we just don't publish a binary yet).
- A `mnctl ai` namespace for embedding small local LLMs / vector stores as templates.
- A web-UI dark-mode toggle (currently aurora is dark-only).
- More game-server templates (Factorio, Terraria, RimWorld dedicated).
- Per-user web auth with WebAuthn.

---

## License & Credits

[MIT License](LICENSE) &mdash; do whatever you want, just don't blame us.

Monolith OS stands on the shoulders of giants:

[Arch Linux](https://archlinux.org/) · [Linux kernel](https://kernel.org/) · [Rust](https://www.rust-lang.org/) · [Cargo](https://doc.rust-lang.org/cargo/) · [Tokio](https://tokio.rs/) · [Axum](https://github.com/tokio-rs/axum) · [ratatui](https://ratatui.rs/) · [crossterm](https://github.com/crossterm-rs/crossterm) · [clap](https://github.com/clap-rs/clap) · [Docker](https://www.docker.com/) · [Podman](https://podman.io/) · [k3s](https://k3s.io/) · [Prometheus](https://prometheus.io/) · [Grafana](https://grafana.com/) · [Loki](https://grafana.com/oss/loki/) · [WireGuard](https://www.wireguard.com/) · [nftables](https://nftables.org/) · [AppArmor](https://apparmor.net/) · [nginx](https://nginx.org/) · [snapper](http://snapper.io/) · [restic](https://restic.net/) · [archiso](https://wiki.archlinux.org/title/archiso) · [Btrfs](https://btrfs.readthedocs.io/) · [Let's Encrypt](https://letsencrypt.org/)

---

<div align="center">

**Monolith OS** &middot; *Built for the ones who mean it.*

[Download&nbsp;v1.0.1](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1) · [Issues](https://github.com/shirou-eh/Monolith/issues) · [Contributing](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

</div>
