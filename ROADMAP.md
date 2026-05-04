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

## v1.0.1 "Obsidian" (Current)

Backwards-compatible feature drop on top of v1.0. New components are opt-in
and the default install now fits on small Discord-bot / single-app hosts.

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
