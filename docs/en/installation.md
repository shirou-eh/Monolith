# Installation Guide

## Prerequisites

- Arch Linux (bare metal or VM)
- x86_64 or ARM64 architecture
- 2 GB RAM minimum (8 GB recommended)
- 20 GB disk minimum (100 GB recommended)
- Internet connection

## Method 1: Bootstrap Installer (Recommended)

For existing Arch Linux systems:

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

### Options

```bash
sudo ./install.sh --hostname myserver --components all
sudo ./install.sh --non-interactive --hostname prod-01
sudo ./install.sh --dry-run  # Preview without changes
```

## Method 2: Build from Source

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```

## Method 3: TUI Installer

Boot from the Monolith ISO and follow the interactive installer.

## Post-Installation

1. Verify installation:
   ```bash
   mnctl info version
   mnctl info system
   ```

2. Run security audit:
   ```bash
   mnctl security audit
   ```

3. Configure monitoring:
   ```bash
   mnctl monitor status
   ```

4. Set up backups:
   ```bash
   mnctl backup create --tag initial
   ```
