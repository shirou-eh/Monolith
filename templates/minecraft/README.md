# Minecraft Java Edition Server

Monolith OS template for running a Minecraft Java Edition server.

## Quick Start

```bash
mnctl template deploy minecraft --name my-minecraft
```

## Configuration

Edit `docker-compose.yml` to customize:

- **TYPE**: Server type — `VANILLA`, `PAPER`, `FABRIC`, `FORGE`
- **VERSION**: Minecraft version (default: `LATEST`)
- **MEMORY**: JVM heap size (default: `4G`)
- **MAX_PLAYERS**: Maximum concurrent players
- **VIEW_DISTANCE**: Render distance in chunks

## RCON Access

```bash
docker exec -it monolith-minecraft rcon-cli
```

Default RCON password: `changeme` — **change this immediately!**

## Backups

World data is stored in the `minecraft-data` volume. Back up with:

```bash
mnctl backup create --tag minecraft
```

## Ports

- `25565/tcp` — Game server
- `25575/tcp` — RCON (admin console)
