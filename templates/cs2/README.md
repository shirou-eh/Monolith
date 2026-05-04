# Counter-Strike 2 Dedicated Server

## Prerequisites

Get a Steam Game Server Login Token (GSLT) from:
https://steamcommunity.com/dev/managegameservers

Set `SRCDS_TOKEN` in the docker-compose.yml.

## Quick Start

```bash
mnctl template deploy cs2 --name my-cs2
```

## Disk Requirements

CS2 requires ~40 GB for game files. Ensure sufficient disk space.

## Ports

- `27015/tcp+udp` — Game server
- `27020/udp` — GOTV
