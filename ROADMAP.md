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

## v1.3.0 "Slate"

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

## v1.4 "Diorite" (Current)

Merges what were planned as two separate releases (v1.5 "Granite" and
v2.0 "Basalt") into one. A few v2.0 items were rescoped on the way in —
see the notes below each; nothing here is aspirational copy, everything
shipped is wired up and was actually run.

**Cluster operations** (from "Granite")
- [x] `mnctl cluster rolling-update` — was reading a config path
      (`/etc/monolith/cluster.toml`) that `cluster init`/`join` never
      wrote to, and restarting a hardcoded `monolith-node` unit that
      doesn't exist. Fixed to read real peer config, restart a
      configurable service, and gate each node on a post-restart health
      check — a chunk that fails aborts the rest of the rollout instead
      of cascading a bad build to the whole cluster.
- [x] `mnctl cluster drain` / `uncordon` — real cordon state now, not the
      no-op calls rolling-update used to shell out to
- [x] `mnctl cluster deploy` — used to just print success without doing
      anything; now actually restarts the given service on each target
- [x] `mnctl cluster nodes` — now lists real peers with reachability and
      cordon state instead of only ever printing "this node"
- [x] `mnctl security anomaly` — log-based anomaly detection: learns a
      per-host EWMA baseline of journal warning+ lines per window and
      flags spikes against it, instead of `security ids`'s fixed
      SSH-failure threshold
- [x] `mnctl monitor exporter` — custom Prometheus exporter for
      Monolith-specific metrics (cluster peers, anomaly baseline,
      autobalance job outcomes) — runs alongside node_exporter, not
      instead of it
- [x] `mnctl kube autoscale` — per-Deployment HPA generation/apply, the
      per-template autoscaling hook (needs metrics-server in-cluster)

**Platform** (from "Basalt", rescoped where noted)
- [x] `mnpkg repo init/add/build/serve` — a real local pacman repository
      (wraps `repo-add`), plus a minimal static file server so another
      Monolith node can pull from it over the LAN
- [x] Declarative system configuration — `mnctl declare apply -f spec.toml`
      reconciles profile, hardening level, service enable/disable, and
      firewall rules against a spec file. Smaller in scope than
      NixOS — a reconciler over existing mnctl commands, not a from-
      scratch config language.
- [x] `mnctl system immutable enable/disable/status` — read-only `@`
      root with `/etc`, `/var`, `/opt` writable via a separate
      subvolume; edits fstab and requires a reboot rather than
      remounting live
- [x] Built-in secrets management (Vault-like) — turned out to already
      ship (`mnctl secrets`, age-encrypted with TPM/YubiKey/age-key
      recipients) since v1.0.1. No rebuild needed; listed here only
      because it was on the original v2.0 list.
- [x] Multi-cloud deployment support — rescoped to
      `mnctl cloud template --provider hetzner|digitalocean|aws`:
      generates Terraform + cloud-init scaffolding. Deliberately does
      **not** hold cloud credentials or call provider APIs — that's a
      different, much bigger trust boundary than "generate the files
      you'd write by hand anyway."
- [x] GUI installer (Wayland) — rescoped/closed as already covered: the
      installer (`monolith-installer`) is already a full interactive
      ratatui TUI wizard (keyboard layout, disk selection, encryption,
      timezone). A from-scratch Wayland GUI on top of a TUI-first,
      server-focused distro would be disproportionate scope for what
      it'd add.
- [x] AI-assisted troubleshooting — rescoped to `mnctl doctor`: a fixed
      checklist against bugs this project has actually shipped and
      fixed (wrong nftables table, missing IP-detection binary, dropped
      fastfetch modules, unscoped sudoers, disk/service health). Named
      honestly — it's a rule-based checklist, not a model in the loop.
- [ ] ARM64 optimized kernel with big.LITTLE scheduling — dropped for
      this release: no ARM hardware available to build against or test
      on. `kernel/build.sh` itself got real fixes this cycle (missing
      `bc`/toolchain prerequisite checks, `olddefconfig` so a config
      fragment doesn't hang the build, `LLVM=1` toolchain detection that
      actually checks for `llvm-ar` instead of just `clang`, and a
      direct-install fallback that no longer silently no-ops when
      `make install` can't find `/sbin/installkernel`) — ARM64
      big.LITTLE tuning specifically is the part left for whenever
      there's a board to validate it on.
- [ ] Custom init system integration — dropped as underspecified; no
      concrete init system or use case attached to the original line.
