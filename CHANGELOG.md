# Changelog

All notable changes to Monolith OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] — Unreleased — "Obsidian"

This patch release lands a large feature set as a backwards-compatible
extension of the v1.0 "Obsidian" line. Existing v1.0 deployments can
upgrade in-place — every new component is opt-in. The default install now
also runs comfortably on low-spec hosts (≥1 vCPU / ≥512 MB RAM) for
Discord-bot / single-app workloads.

### Changed — UI redesign

- **mnweb** — full visual overhaul. Glass-morphism cards, an aurora
  gradient backdrop, sidebar navigation with SVG icons, animated CPU
  sparkline (60 samples, smoothly scaled), animated meters that shift
  through emerald → amber → red as load rises, a unified search bar
  that filters the visible table, and a quick-glance panel summarising
  service / container / disk / template counts. Honors
  `prefers-reduced-motion`.
- **mntui** — same emerald + cyan brand palette as mnweb. Rounded
  border panels, branded header (`▮ MONOLITH v1.0.1 · Obsidian`),
  highlighted active tab with bracketed hotkey hints, three-column
  System view (CPU sparkline + RAM/Swap gauges + load average · disks
  + top processes · status pane with live health pill).
- **monolith-installer** — same brand palette, rounded panels, an
  always-visible step progress bar, key-value review screen, and a
  staged install screen with `✓ / ● / ○` glyphs that change colour as
  each stage completes.
- **mnctl info version** — branded title bar, soft separators, and
  truecolor accents.

### Added

- **mnweb** — new workspace crate that ships an embedded single-page web
  management UI. Exposes a JSON API (`/api/overview`, `/api/services`,
  `/api/containers`, `/api/disks`, `/api/cluster`, `/api/templates`,
  `/api/logs`, `/healthz`) with Axum, and bundles the SPA assets directly
  into the binary so the deploy is a single static executable.
- **mnctl web** — launcher for `mnweb`. Provides `run`, `enable`, `disable`,
  `status`, and `url` subcommands. `enable` writes a hardened systemd unit
  to `/etc/systemd/system/monolith-mnweb.service`. The launcher canonicalises
  the `mnweb` path before writing the unit so relative dev paths don't break
  systemd.
- **mnctl plugin** — plugin system for `mnctl`. Discovers executables named
  `mnctl-<plugin>` under `/usr/local/lib/monolith/plugins`,
  `/usr/lib/monolith/plugins`, and `~/.config/monolith/plugins/`. Supports
  `list`, `info`, `path`, `install` (local file or HTTP URL), `remove`, and
  `run` (forwards trailing args).
- **mnctl iso** — ISO builder that wraps `mkarchiso` via the new
  `iso/build-iso.sh` helper. The bundled archiso profile lives under
  `iso/profile/` and includes a Monolith-themed MOTD, first-boot helper,
  and an installer launcher in `iso/airootfs/`. Subcommands: `build`,
  `doctor`, `profile-path`. `build` accepts `--tier lite|full|pro` to
  bake the matching `[system].profile` into the ISO's default
  `monolith.toml`; the `lite` tier also strips the monitoring stack
  out of the package list so the image stays small.
- **CI release workflow** — `.github/workflows/release.yml` now builds
  three ISO variants (`monolith-<version>-{lite,full,pro}-x86_64.iso`)
  in an Arch Linux container on every `v*.*.*` tag, computes SHA-256
  checksums, and attaches all of them plus the `x86_64` and `aarch64`
  binary tarballs to the GitHub Release. `workflow_dispatch` lets
  maintainers cut a one-off ISO without tagging.
- **mnctl kube** — Kubernetes (k3s) integration. Subcommands: `install`
  (server or agent, with channel pinning and Traefik/ServiceLB toggles),
  `uninstall`, `status`, `nodes`, `pods`, `apply`, `token`, `kubeconfig`,
  and a `kubectl` pass-through.
- **mnctl disk** — disk inventory and SMART health. Subcommands: `list`,
  `usage`, `io`, `smart status|attributes|test|log|watch`, and `nvme`.
- **mnctl notify** — notification dispatch. Sends webhook + SMTP messages
  using `msmtp` (preferred) or `curl` as a fallback. Subcommands: `test`,
  `send`, `webhook`, `email`, `show`.
- **mnctl profile** — resource profile manager. `lite`, `full`, and `pro`
  presets that toggle the heavy parts of the stack (Prometheus / Grafana /
  Loki, mnweb, k3s) so Monolith fits on a 512 MB Discord-bot VPS.
- **Templates** — Valheim, Palworld, MariaDB 11, MongoDB 7, **discord-bot
  (Node.js / discord.js)**, **discord-bot-py (Python / discord.py)**, and
  **telegram-bot (python-telegram-bot)** with matching docker-compose,
  README, and AppArmor profiles where relevant.
- **Config** — new sections `notifications.smtp`, `webui`, `kubernetes`,
  and `disks` in `monolith.toml` and `config_default.toml`. New
  `[system].profile` key for the resource profile.
- **systemd** — new `monolith-mnweb.service` unit for running the web UI as
  a hardened service.
- Workspace bumped to `1.0.1`. `mnctl info version` lists all five
  components (`mnctl`, `mnpkg`, `mntui`, `mnweb`, `monolith-installer`).

### Changed

- Cargo `release` profile is now size-optimised (`opt-level = "z"`,
  `lto = true`, `codegen-units = 1`, `strip = "symbols"`,
  `panic = "abort"`). Resulting binaries are 30-50% smaller, which matters
  on tiny VPSes and embedded boards. A new `release-fast` profile keeps
  the old fast-iteration behaviour.
- `mnctl template list/info` now surfaces the new templates and their
  categories.
- `make install` and the release CI now package `mnweb` and the
  `monolith-installer` binary, plus the `iso/` profile under
  `/usr/share/monolith/iso/`.

### Fixed

- `mnctl notify`: `MSMTP_PASSWORD` is now passed via `Command::env()`
  before spawning instead of being set on the parent process after
  spawn — the previous code would never authenticate. `--ssl-reqd` is
  also now only attached for `starttls` / `tls` modes, so
  `security = "plain"` SMTP relays work. `redact_url` uses character
  iteration so URLs with multi-byte UTF-8 don't panic.
- `mnctl disk nvme`: NVMe namespace devices like `/dev/nvme0n1` are no
  longer mangled into `/dev/nvme0n` before being passed to
  `nvme smart-log`. Partition paths like `/dev/nvme0n1p1` still get
  trimmed back to the namespace.
- `mnctl kube install`: the `--disable-traefik` flag now actually accepts
  a value (`true`/`false`) instead of being permanently true. The same
  fix is applied to `--sudo` in `mnctl iso build`.

## [1.0.0] — 2024-01-01 — "Obsidian"

### Added

- **mnctl** — Unified server management CLI with 15 command groups
  - service: Full systemd service management
  - container: Docker/Podman unified interface
  - deploy: Zero-config application deployment with runtime detection
  - monitor: System monitoring (CPU, RAM, disk, network, alerts, PromQL)
  - security: Audit, firewall, AppArmor, fail2ban, CVE scanning, integrity
  - update: Package updates with snapshot safety and kernel management
  - backup: Two-tier backup (snapper + restic)
  - network: Interface, DNS, route management, connectivity testing
  - vpn: WireGuard tunnel management
  - proxy: nginx reverse proxy with automatic TLS
  - cluster: Multi-node management with etcd
  - bench: CPU, memory, disk, network benchmarking
  - template: One-command application deployment
  - info: System, hardware, version information
  - config: Configuration management and validation
- **mnpkg** — Enhanced package manager wrapper
  - Snapshot safety (auto-creates restore points)
  - AUR support (auto-detects paru/yay)
  - Package pinning and CVE auditing
- **mntui** — Terminal dashboard with real-time system monitoring
- **Installer** — TUI-based multi-step installation wizard
- **Custom kernel** — Server-optimized with BORE scheduler, BBR3, WireGuard
  - x86_64 and ARM64 configurations
  - Automated build script with GPG verification
- **Security hardening**
  - nftables firewall with default-deny policy
  - Hardened SSH (port 2222, key-only, modern ciphers)
  - AppArmor profiles for nginx, PostgreSQL, Redis, Node.js, game servers
  - Kernel sysctl hardening (ASLR, ptrace, dmesg restriction)
  - fail2ban integration
- **Monitoring stack**
  - Prometheus with node exporter, cAdvisor, custom targets
  - Grafana dashboard with system overview
  - Loki + Promtail for log aggregation
  - Alert rules for CPU, memory, disk, security, containers
- **Backup system**
  - Btrfs snapshots via snapper (hourly/daily/weekly/monthly)
  - restic remote backups with configurable destinations
  - Systemd timers for scheduled backups
- **Application templates**
  - Minecraft Java Edition (Paper/Vanilla/Fabric/Forge)
  - Counter-Strike 2 dedicated server
  - PostgreSQL 16 with optimized config
  - Redis 7 with persistence
  - Node.js application
  - Python Discord bot
  - nginx reverse proxy with TLS
- **Documentation**
  - Multi-language: English, Russian, Chinese, Spanish
  - Complete command reference
  - Installation guide and first-steps walkthrough
