# Discord Bot (Node.js / discord.js v14)

Production-ready Discord bot starter for Monolith OS. Built around
[discord.js](https://discord.js.org) v14, ships as a hardened container,
and runs comfortably on a `lite`-profile Monolith host (≤256 MB RAM).

## Deploy

```bash
# Copy template into your services directory
mnctl template deploy discord-bot-node --name my-bot

# Cd into the deployment and edit secrets
cd /var/lib/monolith/deployments/my-bot
cp .env.example .env
$EDITOR .env

# Bring it up
docker compose up -d --build
```

## Files

| Path                  | Purpose                                          |
|-----------------------|--------------------------------------------------|
| `docker-compose.yml`  | Hardened service definition (read-only, non-root)|
| `Dockerfile`          | Two-stage Node 20 Alpine build                   |
| `package.json`        | discord.js + dotenv only — keep deps tiny        |
| `src/index.js`        | Minimal `!ping` handler. Replace with your bot.  |
| `.env.example`        | Required `DISCORD_TOKEN` placeholder             |

## Resource limits

The compose file caps memory at 256 MB and CPU at 0.5 cores by default.
Adjust the `deploy.resources.limits` block if your bot does heavier
work (e.g. voice, large caches). On the `lite` Monolith profile this
fits alongside the OS itself on a 512 MB VPS.

## Logs

The bot writes structured JSON to stdout. View them via:

```bash
mnctl container logs my-bot          # last 100 lines, follow with -f
mnctl monitor logs --service my-bot  # via Loki when monitoring is on
```

## Updating

```bash
cd /var/lib/monolith/deployments/my-bot
git pull            # if you put src/ under VCS
docker compose up -d --build
```
