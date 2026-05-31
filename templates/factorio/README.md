# Factorio Server

Deploy a headless Factorio server with `mnctl template deploy factorio`.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `FACTORIO_SAVE` | `_autosave1` | Save name to load/create |
| `FACTORIO_SAVE_INTERVAL` | `300` | Autosave interval (seconds) |
| `FACTORIO_GAME_SPEED` | `1` | Game speed multiplier |

## Volumes

- `/factorio/saves` — Save files (persist this)

## Ports

- `34197/udp` — Game server
