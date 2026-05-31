# Terraria Server

Deploy a Terraria (tModLoader) server with `mnctl template deploy terraria`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `TERRARIA_WORLD` | `Monolith` | World name |
| `TERRARIA_MAX_PLAYERS` | `8` | Max concurrent players |
| `TERRARIA_PORT` | `7777` | Game port |
| `TERRARIA_PASS` | *(none)* | Server password |

## Volumes

- `/terraria/worlds` — World / config files (persist this)

## Ports

- `7777/tcp` — Game server
