# Valheim Dedicated Server

Monolith OS template for Valheim dedicated server with automatic updates and
periodic backups.

## Quick Start

```bash
mnctl template deploy valheim --name my-valheim
```

## Default Credentials

- **Server password**: `changeme` — **change this immediately!**
- **World name**: `Monolith`

Edit `docker-compose.yml` and update `SERVER_PASS`, `SERVER_NAME`, and
`WORLD_NAME` before deploying to a public network.

## Ports

| Port    | Proto | Purpose             |
|---------|-------|---------------------|
| 2456-2458 | UDP | Game traffic       |
| 9001    | TCP   | Status HTTP server |

Open these in `nftables` (or via `mnctl security firewall`) to expose the
server publicly.

## Backups

Backups run automatically every 2 hours and are kept for 14 days under
`/config/backups` inside the container (Docker volume `valheim-config`).

You can also pair this with `mnctl backup` for off-host snapshots:

```bash
mnctl backup create valheim --include /var/lib/docker/volumes/valheim-config
```

## Resource Tuning

Valheim's dedicated server is memory-hungry as worlds grow. The default
container limits to 4 GB RAM; bump this for larger groups by editing the
`deploy.resources.limits.memory` field.

## References

- Image: <https://github.com/lloesche/valheim-server-docker>
- Game: <https://www.valheimgame.com/>
