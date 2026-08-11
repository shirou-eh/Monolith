# Changelog

All notable changes to Monolith OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] — Unreleased — "Obsidian"

## [1.4.0] — Unreleased — "Diorite"

Merges the planned v1.5 "Granite" and v2.0 "Basalt" releases into one.
See ROADMAP.md for the full per-item breakdown, including what was
rescoped and what was dropped and why.

### Added

- `mnctl cluster drain` / `uncordon` — real cordon state, consulted by
  `schedule`/`autobalance`/`rolling-update`
- `mnctl security anomaly` — log-based anomaly detection against a
  learned per-host baseline (EWMA), separate from `security ids`'s
  fixed-threshold SSH check and `monitor anomaly`'s metric-based one
- `mnctl monitor exporter` — Prometheus exporter for Monolith-specific
  metrics
- `mnctl kube autoscale` — per-Deployment HPA generation/apply
- `mnpkg repo init/add/build/serve` — local pacman repository + a
  minimal static file server to pull it from another node
- `mnctl declare apply/init` — declarative reconciliation (profile,
  hardening, services, firewall) from a spec file
- `mnctl system immutable enable/disable/status` — read-only root
  option, `/etc`+`/var`+`/opt` writable via a separate subvolume
- `mnctl cloud template` — Terraform + cloud-init scaffolding for
  Hetzner/DigitalOcean/AWS (generates files only, no credentials, no
  API calls)
- `mnctl doctor` — rule-based diagnostics against known footguns

### Fixed

- `mnctl cluster rolling-update` read a cluster config path
  (`/etc/monolith/cluster.toml`) that `cluster init`/`join` never
  actually wrote, so it always failed to find any nodes on a real
  cluster. Also restarted a hardcoded `monolith-node` unit that doesn't
  exist and shelled out to `drain`/`uncordon` subcommands that didn't
  exist yet. Now reads real peer config, restarts a configurable
  service, and aborts the rest of the rollout the moment a node fails
  its post-restart health check.
- `mnctl cluster deploy` printed success unconditionally without
  contacting any node. Now actually restarts the target service per
  node and reports real per-node pass/fail.
- `mnctl cluster nodes` only ever printed the local hostname as
  "master (this node)" regardless of cluster size. Now lists real
  peers with reachability and cordon state.
- `kernel/build.sh`: `detect_latest_kernel()` returned its version
  string via `$(...)`, but the script's own log function wrote to
  stdout too — so the captured "version" was the whole log transcript
  glued to the real version number, and every download URL built from
  it was garbage. Logging now goes to stderr.
- `kernel/build.sh`: `configs/*.config` is a curated fragment, not a
  complete `.config` — copying it straight in and running `make` left
  thousands of symbols unresolved with no `olddefconfig` step to fill
  them from upstream defaults. Added that step.
- `kernel/build.sh`: toolchain selection checked only for `clang`, then
  unconditionally pinned `AR=llvm-ar`/`NM=llvm-nm`/etc — on a host with
  clang+lld but not the separate `llvm` package (no `llvm-ar`), the
  build got partway in and died on the first missing tool. Now checks
  for the full LLVM toolchain and uses the kernel build system's own
  `LLVM=1` switch; falls back to GCC otherwise.
- `kernel/build.sh`: no upfront check for build prerequisites (`bc` in
  particular) meant a missing tool surfaced as a build failure ~5
  minutes in instead of an immediate, actionable error.
- `kernel/build.sh`: `make install` on a hand-built source tree
  (without `/sbin/installkernel` set up the way Arch's own `linux`
  package does) silently installs nothing rather than failing — so the
  "installed directly" fallback path was doing nothing at all. Now
  copies the built image directly. Also, `mkinitcpio -p monolith`
  always failed (no such preset ships anywhere) and silently fell back
  to `-P`, rebuilding every *existing* preset instead of the kernel
  that was just installed. Now targets the new kernel's version/output
  path explicitly. `makepkg` also refuses to run as root outright,
  which this script does by default under `sudo mnctl update kernel`
  — now builds the package as the invoking unprivileged user
  (`$SUDO_USER`) and falls back to the direct-install path if there
  isn't one.
- `mnctl update kernel` looked for `kernel/build.sh` at
  `/usr/share/monolith/kernel/build.sh`, a path nothing in the
  installer ever actually populated — so every install silently fell
  back to `pacman -S monolith-kernel` regardless of intent. Installer
  now copies `kernel/{build.sh,configs,patches}` into place.
- Local git identity for 3 commits landed on `main` under the wrong
  author again after the v1.3.0 history rewrite (local repo config
  wasn't updated post-rewrite) — corrected and re-pushed.

## [1.3.0] — Unreleased — "Slate"

A general-purpose release: Monolith stops being server-only, gains a
reactive security layer instead of purely static rules, and identifies
itself correctly instead of reporting as bare Arch Linux.

### Added

- **`desktop` resource profile** (`mnctl profile set desktop`) — a
  fourth profile alongside `lite`/`full`/`pro` for a regular PC rather
  than a headless box. Monitoring stack off, `mnweb` on as a local
  dashboard, SSH back on port 22, and its own `desktop` hardening
  level (debugger/profiler-friendly instead of server-locked-down).
- `mnctl security ids` — behavioural pass over recent sshd activity,
  flags source IPs past a failed-login threshold.
- `mnctl security honeypot` — decoy ports; any connection is treated
  as hostile and reacted to automatically.
- `mnctl security react <ip>` — immediate nftables ban + forensic
  snapper snapshot for a confirmed-hostile source.
- `mnctl security harden --level desktop` — new hardening level with
  its own sysctl set (ptrace/BPF/perf stay usable for local dev tools).
- `mnctl tune auto` — continuous CPU/IO re-tuning against live load
  average instead of a one-shot pass at install time.
- `mnctl cluster fs mount` / `umount` / `sync-status` — shared
  filesystem across cluster nodes (sshfs-backed) layered over the
  existing btrfs/snapper stack.
- `mnctl cluster schedule <command>` — runs a command on whichever
  cluster node currently has the most free memory.
- `mnpkg update --self` — fetches the latest Monolith release from
  `github.com/shirou-eh/Monolith`, snapshots first, installs it.
- Monolith-branded `/etc/os-release` (`fastfetch`, `neofetch`,
  `lsb_release`, desktop "About" panels now correctly report
  "Monolith OS" instead of "Arch Linux"), plus a distro logo
  (`/usr/share/pixmaps/monolith.{svg,png}`) and a matching fastfetch
  config/ASCII logo shipped under `/etc/fastfetch/`.

### Fixed

- Installer never wrote `/etc/os-release` — every install reported
  itself as plain Arch Linux regardless of everything else Monolith
  had configured. The installer now writes a proper Monolith
  `os-release` as part of the base install steps.

## [1.2.0] — Unreleased — "Onyx"

Part 1 + Part 2 combined release — combines all v1.0.2 patches with new
commands, UI improvements, kernel updates, and self-update support. No
breaking changes; all v1.0 deployments can upgrade in-place.

Targeted patch release. No breaking changes; all v1.0.1 deployments can
upgrade in-place with `mnctl update system`.

### Fixed

- **`mnctl kube install`** — kubeconfig was written with mode `0644`
  (world-readable), leaking cluster credentials to all local users.
  Now written `0640` and chowned to `root:wheel` so only members of the
  `wheel` / `sudo` group can read it. Existing installs: run
  `chmod 640 /etc/rancher/k3s/k3s.yaml && chgrp wheel /etc/rancher/k3s/k3s.yaml`
  after upgrading.
- **`mnctl proxy ssl renew`** — certificates were renewed by certbot but
  nginx was never signalled to reload them, so the old certs stayed active
  until the next manual `mnctl proxy reload`. The renew path now calls
  `nginx -s reload` after each successful renewal and reports the old/new
  expiry dates.
- **`mnctl backup list`** — crashed with a Rust `unwrap` panic when the
  restic repository had never been initialised, printing an unhelpful
  `thread 'main' panicked` message. Now emits a clean error:
  `✗ Backup repository not initialised — run: mnctl backup create`.
- **`mnctl deploy app`** — `--env` values containing `=` (e.g.
  `DATABASE_URL=postgres://user:p@ss@host/db?ssl=require&pool=5`)
  were split on the first `=` only; everything after the first `=` was
  silently dropped. Fixed by using `splitn(2, '=')` when parsing each
  key-value pair.
- **`mnweb /api/logs`** — returned HTTP 500 with an empty body when the
  journald socket was unavailable (container environments, minimal installs
  without systemd-journald). Now returns HTTP 200 with
  `{"logs": [], "warning": "journald unavailable"}` so the web UI shows an
  empty log pane instead of a broken error state.
- **`mnctl notify send`** — subject and body were swapped in the SMTP
  `Subject:` header when `msmtp` was used as the transport. The email
  landed with the body text as the subject line.
- **`mnctl monitor logs --since`** — relative time strings like `"1h"` or
  `"30m"` were passed verbatim to `journalctl --since` without the leading
  `-` required for relative offsets. Fixed; `"1h"` is now translated to
  `"-1h"` automatically so `mnctl monitor logs --since 1h` works as
  expected.

### Added

- **`mnctl notify telegram`** — new notification channel that dispatches
  messages through the Telegram Bot API (`api.telegram.org/bot<token>/sendMessage`).
  Configure via two new keys in `monolith.toml`:

  ```toml
  [notifications.telegram]
  enabled  = true
  bot_token = "123456:ABC-..."
  chat_id   = "-1001234567890"
  ```

  `mnctl notify test` and `mnctl notify send` automatically include
  Telegram when the channel is enabled. `mnctl notify telegram --message`
  sends a one-off message without touching SMTP or webhooks. Pairs
  naturally with the `telegram-bot` application template added in v1.0.1.

- **`mnctl monitor export`** — snapshot the current system state to a
  structured file for scripting, alerting pipelines, or quick reporting.

  ```
  mnctl monitor export --format json   # default: /tmp/monolith-export.json
  mnctl monitor export --format csv --out /var/log/monolith/$(date +%F).csv
  ```

  Captures: hostname, timestamp, CPU (per-core %), RAM/Swap (used/total),
  disk usage per mount, load average (1/5/15 min), and active service
  count. The JSON schema is stable across patch releases.

- **`mnctl info version --json`** — machine-readable version output for
  scripts and CI health checks. Returns a JSON object with `version`,
  `codename`, `build_date`, `git_sha`, and per-component versions
  (`mnctl`, `mnpkg`, `mntui`, `mnweb`, `installer`). The plain-text
  `mnctl info version` output is unchanged.

- **`mnctl security audit --json`** — same treatment as `info version`:
  add `--json` flag so audit results can be piped into SIEM tooling or
  processed with `jq`. Includes CVE IDs and CVSS scores for flagged
  packages (sourced from the Arch Linux security tracker).

- **mnweb — `/api/overview` response cache** — the overview endpoint
  called `sysinfo` on every HTTP request, causing measurable CPU spikes on
  low-spec hosts when the dashboard auto-refreshed at 2-second intervals.
  Responses are now cached for **2 seconds** with `tokio::time::Instant`
  and served from an `Arc<RwLock<CachedOverview>>`. Wall-clock accuracy is
  preserved for the dashboard; CPU load drops to near zero between refreshes.

### Changed

- Workspace bumped to `1.0.2`. `monolith.toml` `version` field updated
  accordingly; `mnctl info version` and the mnweb footer both reflect the
  new version automatically.
- `mnctl proxy ssl renew` output now shows a before/after expiry table:

  ```
  ✓ example.com  renewed  2025-05-31 → 2025-08-29  nginx reloaded
  ✓ api.example.com  renewed  2025-06-01 → 2025-08-30  nginx reloaded
  ```

- `[notifications.telegram]` section added to the default
  `config/monolith/monolith.toml` and `mnctl/config_default.toml` with
  all keys present but `enabled = false` so existing configs don't need
  to be touched.


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

- **mnctl tune** — performance tuning command set that spreads CPU load
  across every available core and hardware thread. Subcommands:
  `cpu` (governor + EPP + min-freq + THP + SMT + irqbalance),
  `io` (per-device elevator + nr_requests + read_ahead_kb),
  `all`, `status`, and `reset`. Presets: `performance` (default),
  `balanced` (`schedutil`), and `powersave`. Idempotent and
  `--dry-run` friendly.
- **monolith-tune.service** — oneshot systemd unit that runs
  `mnctl tune all` early at boot so workloads spread across all
  cores from the first second of uptime. Enabled automatically by
  `scripts/install.sh`.
- **`/etc/sysctl.d/99-monolith-cpu.conf`** — scheduler / NUMA / vmstat
  tunables that bias the kernel toward server-style throughput
  (autogroup, NUMA balancing on, watchdog cpumask, vm.stat_interval=1,
  zone_reclaim_mode=0, larger fs.aio-max-nr).
- **`[performance]` section in `monolith.toml`** — declarative
  defaults for cpu_governor, energy_performance_preference,
  apply_on_boot, transparent_hugepages, and per-class I/O elevators.
- **Cargo `release-perf` profile** — opt-in profile (`cargo build
  --profile release-perf`) for hosts that prefer absolute runtime
  speed over the default size-optimised binaries.
- **Multi-threaded CPU benchmark** — `mnctl bench cpu` now spawns one
  worker per logical CPU, reports parallel speedup and efficiency,
  and replaces the previous single-threaded `dd` proxy.
- **Parallel cargo install** — `scripts/install.sh` now passes
  `CARGO_BUILD_JOBS=$(nproc)` and `--jobs $(nproc)` explicitly so
  the local build always saturates every available core.
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
