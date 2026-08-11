# Monolith OS Roadmap

## v1.0.0 "Obsidian"

- [x] Core CLI (mnctl) with all command groups
- [x] Custom kernel configuration (x86_64 + ARM64)
- [x] Security hardening (nftables, AppArmor, SSH, sysctl)
- [x] Monitoring stack (Prometheus + Grafana + Loki)
- [x] Backup system (snapper + restic)
- [x] TUI dashboard (mntui)
- [x] Application templates
- [x] Bootstrap installer
- [x] Multi-language documentation

## v1.0.1 "Obsidian"

- [x] Web management UI (mnweb) — embedded SPA + JSON API
- [x] Plugin system for mnctl (`mnctl plugin install/list/run`)
- [x] Custom ISO builder with archiso (`mnctl iso build`)
- [x] Kubernetes integration (k3s) via `mnctl kube`
- [x] SMTP notification support (`mnctl notify`)
- [x] Disk health monitoring (SMART) via `mnctl disk smart`
- [x] Additional templates: Valheim, Palworld, MariaDB, MongoDB
- [x] Bot templates: discord.js, discord.py, python-telegram-bot
- [x] Resource profiles (`lite` / `full` / `pro`) via `mnctl profile`
- [x] Size-optimised cargo release profile (`opt-level=z`, `lto`, `strip`)

## v1.2.0 "Onyx"

Targeted bug-fix and quality-of-life patch. No breaking changes.

- [x] Fix kubeconfig written world-readable (`0644` → `0640`, chown `root:wheel`)
- [x] Fix `mnctl proxy ssl renew` not reloading nginx after cert renewal
- [x] Fix `mnctl backup list` panicking when repository is uninitialised
- [x] Fix `mnctl deploy app --env` dropping values that contain `=`
- [x] Fix `mnweb /api/logs` returning HTTP 500 when journald is unavailable
- [x] Fix `mnctl notify send` swapping subject and body in SMTP transport
- [x] Fix `mnctl monitor logs --since` rejecting relative time strings
- [x] Telegram notification channel (`mnctl notify telegram`)
- [x] `mnctl monitor export` — snapshot metrics to JSON/CSV
- [x] `mnctl info version --json` and `mnctl security audit --json`
- [x] mnweb `/api/overview` 2-second response cache (CPU relief on small VPS)

## v1.3.0 "Slate" (Current)

Not server-only anymore, and a reactive security layer instead of
purely static rules. See CHANGELOG.md for the full list.

- [x] `desktop` resource profile (4th profile alongside lite/full/pro)
- [x] `desktop` hardening level in `mnctl security harden`
- [x] `mnctl security ids` / `honeypot` / `react`
- [x] `mnctl tune auto` — continuous re-tuning against live load
- [x] `mnctl cluster fs mount/umount/sync-status`
- [x] `mnctl cluster schedule`
- [x] `mnpkg update --self`
- [x] Monolith-branded `/etc/os-release` + distro logo + fastfetch config
- [x] Fixed: installer never wrote `/etc/os-release` (reported as Arch)

## v1.5 "Granite" (Future)

- [ ] Advanced cluster operations (rolling updates)
- [ ] Log-based anomaly detection
- [ ] Custom Prometheus exporters
- [ ] Per-template auto-scaling hooks for k3s

## v2.0 "Basalt" (Future)

- [ ] Full custom package repository
- [ ] GUI installer (Wayland)
- [ ] Declarative system configuration (NixOS-inspired)
- [ ] Immutable root filesystem option
- [ ] Built-in secrets management (Vault-like)
- [ ] Multi-cloud deployment support
- [ ] AI-assisted troubleshooting
- [ ] ARM64 optimized kernel with big.LITTLE scheduling
- [ ] Custom init system integration

## v2.5 "Quartzite" (Proposed)

Not server-only anymore — hardening applies to any Monolith install (desktop/edge included). Three pillars: security, performance, and inter-node clustering.

**Security**
- [ ] `mnctl security ids` — behavioural intrusion detection layered on existing nftables/AppArmor/fail2ban
- [ ] `mnctl security honeypot` — decoy services; any hit triggers alert + automatic ban
- [ ] `mnctl security react` — automatic response to suspicious activity: instant ban + forensic snapshot, no human in the loop
- [ ] `mnctl security audit --deep` — extends existing audit with config drift, permission, and unexpected-open-port detection

**Performance**
- [ ] `mnctl tune auto` — continuous sysctl/scheduler auto-tuning based on live load, not a one-time install-time pass
- [ ] `mnctl gpu` — GPU passthrough for containers/k3s, with driver + utilisation metrics in mnweb
- [ ] `mnctl profile auto` — automatic switching between `lite`/`full`/`pro` based on observed load instead of manual `mnctl profile set`
- [ ] `mnctl bench --continuous` — historical benchmark logging to feed the above

**Inter-node clustering (shared files + shared load)**
- [ ] `mnctl cluster join` — auto-discovers other Monolith nodes on the local network and joins a cluster, zero manual config
- [ ] `mnctl cluster fs mount` — shared filesystem across nodes (distributed layer over the existing btrfs/snapper stack); a file created on one node is visible on the rest
- [ ] `mnctl cluster fs sync status` — replication/consistency state between nodes
- [ ] `mnctl cluster schedule` — workloads land on whichever node has free capacity right now (zero-config layer on top of existing `mnctl kube`/k3s scheduling)
- [ ] `mnctl cluster status` — fleet-wide view: nodes, free CPU/RAM/disk per node, what's running where
