# First Steps with Monolith OS

After installation, here's how to get started.

## System Overview

```bash
mnctl info system       # OS, kernel, uptime, hardware
mnctl monitor status    # CPU, RAM, disk usage
mnctl security audit    # Security posture check
```

## Deploy Your First Application

### From Template

```bash
mnctl template list
mnctl template deploy postgresql --name my-database
```

### Zero-Config Deployment

```bash
cd /path/to/your/app
mnctl deploy app --name my-app --port 3000
```

Monolith detects the runtime (Node.js, Python, Rust, Go, Docker) and deploys automatically.

## Firewall Management

```bash
mnctl security firewall status
mnctl security firewall allow 443    # HTTPS
mnctl security firewall allow 8080   # Custom port
```

## VPN Setup

```bash
mnctl vpn create home-vpn
mnctl vpn peer add home-vpn --endpoint <ip>:51820 --pubkey <key>
mnctl vpn connect home-vpn
```

## Backups

```bash
mnctl backup create --tag pre-deploy
mnctl backup list
mnctl backup snapshots    # Btrfs snapshots
```
