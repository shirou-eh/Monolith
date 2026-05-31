# Palworld Dedicated Server

Monolith OS template for [Palworld](https://www.pocketpair.jp/palworld) dedicated
server with automatic backups, RCON, and Steam query support.

## Quick Start

```bash
mnctl template deploy palworld --name my-palworld
```

## Default Credentials

- **Server password**: `changeme` — **change this immediately!**
- **Admin password**: `admin-changeme` — required for RCON.

Edit `docker-compose.yml` and update `SERVER_PASSWORD` and `ADMIN_PASSWORD`
before exposing the server publicly.

## Ports

| Port    | Proto | Purpose         |
|---------|-------|-----------------|
| 8211    | UDP   | Game traffic    |
| 27015   | UDP   | Steam query     |
| 25575   | TCP   | RCON            |

Open these in `nftables` or via `mnctl security firewall` to expose the server.

## System Requirements

Palworld's dedicated server is heavyweight. The defaults here reserve 8 GB of
RAM and cap at 16 GB; do not run it on a host with fewer than 12 GB total
RAM. Multithreading is enabled by default.

## Backups

Backups run every 6 hours and are kept for 30 days under the `palworld-data`
Docker volume. Combine with `mnctl backup` for off-host snapshots.

## RCON

Once the server is up:

```bash
docker exec -it monolith-palworld rcon-cli
```

## References

- Image: <https://github.com/thijsvanloef/palworld-server-docker>
- Game: <https://www.pocketpair.jp/palworld>
