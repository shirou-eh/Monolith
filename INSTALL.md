# Monolith OS - Installation & Day-1 Guide

> *"From a freshly downloaded ISO to a hardened, monitored, production-shaped server in 20 minutes."*

This is the long-form, copy-pasteable companion to the [README](README.md). Pick the section that matches your situation; each path lands you in the same place: a Monolith box with `mnctl`, `mntui`, `mnweb`, snapper, nftables, SSH on port 2222, and a working app-template engine.

```text
┌──────────────────────────────────────────────────────────────────────┐
│  Pick your path                                                      │
├──────────────────────────────────────────────────────────────────────┤
│  A · Boot from one of the official ISOs        →   §1 (recommended)  │
│  B · Convert an existing Arch Linux box        →   §2                 │
│  C · Build & install from source               →   §3                 │
│  D · Run the binaries inside a container       →   §4                 │
│  E · Build your own ISO                        →   §5                 │
└──────────────────────────────────────────────────────────────────────┘
```

After install, jump to **§6 First boot** and **§7 Day-1 cookbook**.

---

## 1. Boot from an official ISO (recommended)

### 1.1 Download

Go to **[Releases v1.0.1 "Obsidian"](https://github.com/shirou-eh/Monolith/releases/tag/v1.0.1)** and grab one of:

| ISO                                                                                                                          | Size      | Best for                        |
|------------------------------------------------------------------------------------------------------------------------------|-----------|---------------------------------|
| [`monolith-1.0.1-lite-x86_64.iso`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-1.0.1-lite-x86_64.iso) | ~1.25 GB | 1 vCPU / 512 MB-1 GB VPS        |
| [`monolith-1.0.1-full-x86_64.iso`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-1.0.1-full-x86_64.iso) | ~1.25 GB | 2-4 cores / 2-8 GB home server  |
| [`monolith-1.0.1-pro-x86_64.iso`](https://github.com/shirou-eh/Monolith/releases/download/v1.0.1/monolith-1.0.1-pro-x86_64.iso)   | ~1.25 GB | 4+ cores / 8+ GB cluster node   |

Each ISO ships the same kernel, the same installer, and the same Monolith binaries - the only difference is the package set on the live medium and the default profile baked into `/etc/monolith/monolith.toml`.

### 1.2 Verify

```bash
sha256sum -c monolith-1.0.1-full-x86_64.iso.sha256
# expected: monolith-1.0.1-full-x86_64.iso: OK
```

If the check fails, **don't write the ISO** - redownload.

### 1.3 Write to USB

**Linux / macOS:**

```bash
lsblk                        # find your USB stick — twice
sudo umount /dev/sdX*
sudo dd if=monolith-1.0.1-full-x86_64.iso \
        of=/dev/sdX bs=4M status=progress conv=fsync
sync
```

**Windows:** [Rufus](https://rufus.ie/) (DD mode) or [Ventoy](https://ventoy.net/).

> `dd` is destructive and silent. Confirm `/dev/sdX` is your stick - not a system disk.

### 1.4 Boot the target

1. Plug the USB into the server.
2. Hit the BIOS one-time-boot key (`F12`, `F11`, `Esc`, `Del` - vendor-specific).
3. Pick the USB device. UEFI is preferred, but legacy BIOS works too.
4. The live system auto-launches `monolith-installer` on `tty1`.

### 1.5 Walk the installer

The TUI installer is keyboard-only (`↑/↓/Tab/Enter/Esc`):

| Step | What to enter | Notes                                                          |
|------|---------------|----------------------------------------------------------------|
| 1    | Welcome / language     | Pick a language; defaults work fine.                            |
| 2    | Disk                   | Whole-disk install. SMART warnings are flagged in red.         |
| 3    | Hostname & timezone    | E.g. `hawk.lan`, `Europe/Moscow`.                              |
| 4    | Root password & user   | Strong root pw + a non-root sudoer; the user joins `monolith`. |
| 5    | Network                | DHCP (default) or static. Live link state is shown.            |
| 6    | Profile                | `lite` / `full` / `pro` - matches the ISO by default.    |
| 7    | Security defaults      | Apply firewall + SSH hardening (recommended).                  |
| 8    | Monitoring             | Opt in to Prometheus + Grafana + Loki (off on `lite`).         |
| 9    | Confirm                | Last chance to back out.                                       |
| 10   | Install                | Live progress gauge + log tail.                                |
| 11   | Reboot                 | Pull the USB on the way down.                                  |

After the reboot you're done with this section - jump to **§6 First boot**.

---

## 2. Convert an existing Arch Linux box

Already running Arch Linux on a server? You can become Monolith without re-imaging.

### 2.1 Inspect first

```bash
uname -a                      # confirm Arch Linux
sudo pacman -Syu              # be on the latest packages first
df -h /                       # need ~500 MB free for binaries + configs
```

### 2.2 Run the converter

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

What it does, in order:

1. Detects your hardware and proposes a profile (`lite` / `full` / `pro`).
2. Installs the five binaries (`mnctl`, `mnpkg`, `mntui`, `mnweb`, `monolith-installer`) into `/usr/local/bin/`.
3. Drops the default `/etc/monolith/monolith.toml`.
4. Installs systemd units for `mnweb`, monolith pacman hooks, snapper, and (optionally) the monitoring stack.
5. If you pass `--secure`, applies firewall, SSH hardening, AppArmor profiles, sysctl tuning, fail2ban.
6. Prints a summary with next steps.

```bash
# Just the binaries
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash

# Full server convert with security defaults
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh \
  | sudo bash -s -- --secure --profile full

# Show what would be done, change nothing
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh \
  | sudo bash -s -- --dry-run
```

> The script is idempotent - re-running it upgrades binaries and touches nothing else.

---

## 3. Build & install from source

For developers, ARM64 users, or anyone who wants to audit before they install.

### 3.1 Toolchain

- Rust **1.95+** stable (`rustup default stable`)
- `git`, `make`, `gcc`, `pkg-config`
- `archiso`, `mkarchiso` if you want to build ISOs (Arch host or container)

### 3.2 Clone & build

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd Monolith

# Build the whole workspace
make build                                        # alias for cargo build --release --workspace

# Tests + lint
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Install binaries + units + default config to /
sudo make install
```

Resulting layout:

```
/usr/local/bin/{mnctl,mnpkg,mntui,mnweb,monolith-installer}
/etc/monolith/monolith.toml
/etc/systemd/system/monolith-mnweb.service
/etc/systemd/system/monolith-mntui.service       (optional)
```

### 3.3 Run individual binaries out-of-tree

You don't have to `make install` to play with Monolith:

```bash
cargo run --release --bin mnctl -- info system
cargo run --release --bin mntui                  # q to quit
cargo run --release --bin mnweb                  # http://127.0.0.1:9911
cargo run --release --bin monolith-installer     # demo / dry-run friendly
```

---

## 4. Run inside a container

Useful for CI smoke tests, demos, or trying templates without touching a real box.

```bash
docker run --rm -it \
  -p 9911:9911 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  archlinux:latest bash -c '
    pacman -Syu --noconfirm
    pacman -S --noconfirm git make rust docker
    git clone https://github.com/shirou-eh/Monolith.git /opt/monolith
    cd /opt/monolith && make build && make install
    mnctl info system
    mnweb &
    sleep 2 && curl -s http://127.0.0.1:9911/api/overview | head
  '
```

`mnctl service`, `mnctl container`, `mnweb`, and `mntui` all work inside a container. Kernel-level steps (sysctl, AppArmor, snapper) are skipped automatically.

---

## 5. Build your own ISO

The same script that produced the official ISOs is in this repo. You can target any tier, version, or output path.

### 5.1 Prerequisites

- Arch Linux host **or** Docker (any host).
- 6 GB free disk during the build.
- ~3 GB output for one ISO.

### 5.2 One-shot Docker build

```bash
docker run --rm --privileged -v "$(pwd):/work" -w /work archlinux:latest bash -c '
  pacman -Syu --noconfirm
  pacman -S --noconfirm --needed archiso grub edk2-shell libisoburn squashfs-tools dosfstools mtools
  ./iso/build-iso.sh --tier full --out out --version 1.0.1
'
```

Output: `out/monolith-1.0.1-full-x86_64.iso` plus `.sha256`.

### 5.3 Native Arch build

```bash
sudo pacman -S --needed archiso grub edk2-shell libisoburn squashfs-tools dosfstools mtools
sudo ./iso/build-iso.sh --tier full --out out --version 1.0.1
```

### 5.4 Build all three tiers (CI-style)

```bash
for tier in lite full pro; do
  ./iso/build-iso.sh --tier "$tier" --out out --version 1.0.1
done
```

Each ISO bakes the matching `[system].profile` into `/etc/monolith/monolith.toml` so the live system reports the right tier on first boot.

---

## 6. First boot

You're freshly booted; you have a `tty` prompt. Log in as the user you created (or `root`).

### 6.1 Sanity check

```bash
mnctl info system          # Distro / kernel / hardware overview
mnctl info version         # Branded banner with build SHA + active profile
mnctl profile show         # Confirms the profile baked in by the installer
mnctl monitor status       # CPU / RAM / disk / network at a glance
mnctl security audit       # Pass/fail summary across all hardening categories
```

If anything is red, fix it before exposing the box. `mnctl security harden` re-applies all defaults.

### 6.2 Open the dashboard

**Locally on the box:**

```bash
mntui                      # ratatui dashboard, q to quit
```

**Web UI from your laptop** (over an SSH tunnel, the only safe way by default):

```bash
ssh -L 9911:127.0.0.1:9911 user@your-server
# now open http://127.0.0.1:9911 in your browser
```

If you'd rather expose it publicly with TLS:

```bash
sudo mnctl proxy add monolith.example.com \
  --service monolith-mnweb.service --tls
```

### 6.3 Update everything

```bash
sudo mnpkg upgrade         # snapper snapshot + pacman -Syu
mnctl update history       # see what changed
```

If anything regresses:

```bash
sudo mnpkg rollback latest
sudo systemctl reboot
```

---

## 7. Day-1 cookbook

A grab-bag of common tasks. Same examples are in the README; they're worth seeing in context.

### 7.1 Deploy your first app

```bash
mnctl template list
mnctl template deploy minecraft --name mc-survival
mnctl service status monolith-app-mc-survival.service
mnctl container logs mc-survival --follow
mnctl proxy add mc.example.com --service mc-survival --tls
```

### 7.2 Set up backups

Edit the `[backup]` block of `/etc/monolith/monolith.toml`:

```toml
[backup]
snapper = { schedule = "hourly", keep_hourly = 24, keep_daily = 7, keep_weekly = 4 }
restic  = { repo = "sftp:backup@nas.example.com:/backups/monolith",
            password_file = "/etc/monolith/restic.pass",
            schedule = "daily" }
```

Then:

```bash
sudo mnctl backup create --tag day-1
sudo mnctl backup verify
```

### 7.3 Stand up a k3s cluster

On the control plane:

```bash
sudo mnctl kube install --role server
sudo mnctl kube token             # save this
```

On each agent node:

```bash
sudo mnctl kube install --role agent \
  --server https://control-plane:6443 \
  --token   <TOKEN-FROM-ABOVE>
```

Then:

```bash
mnctl kube nodes
mnctl kube kubectl get pods -A
```

### 7.4 Add a WireGuard VPN

```bash
sudo mnctl vpn create wg0 --listen-port 51820
sudo mnctl vpn peer add wg0 \
  --pubkey <CLIENT-PUBKEY> --allowed-ips 10.99.0.2/32
mnctl vpn status
```

### 7.5 Hook into a webhook for alerts

```bash
sudo mnctl notify webhook \
  --url https://hooks.example.com/services/ABC/DEF \
  --on warning,error
mnctl notify test
```

### 7.6 Install a custom kernel

```bash
sudo bash kernel/build.sh --tier full
sudo mnctl update kernel --custom
sudo systemctl reboot
mnctl info hardware           # confirms the new kernel
```

---

## 8. Where to next?

- The full **[README](README.md)** - project overview, architecture, command tree, full feature list.
- **[CONTRIBUTING.md](CONTRIBUTING.md)** - PR workflow, commit style, formatting & lint rules.
- **[CHANGELOG.md](CHANGELOG.md)** - what landed in v1.0.1 "Obsidian" and earlier.
- **[ROADMAP.md](ROADMAP.md)** - what's next.
- **[docs/](docs/)** - multi-language docs (en/ru/zh/es).

Got stuck? File an issue - we read every one.

[![Open an issue](https://img.shields.io/badge/Stuck%3F-Open%20an%20issue-35e0a1?style=for-the-badge&labelColor=0b1220)](https://github.com/shirou-eh/Monolith/issues/new)
