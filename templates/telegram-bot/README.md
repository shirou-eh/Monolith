# Telegram Bot (python-telegram-bot v21)

Minimal Telegram bot starter for Monolith OS. Sized for the `lite`
profile — uses ~64 MB RAM at idle.

## Deploy

```bash
mnctl template deploy telegram-bot --name my-tg-bot
cd /var/lib/monolith/deployments/my-tg-bot
cp .env.example .env
$EDITOR .env             # paste TELEGRAM_TOKEN from @BotFather
docker compose up -d --build
```

## What's inside

| Path                  | Purpose                                  |
|-----------------------|------------------------------------------|
| `docker-compose.yml`  | Hardened, read-only, non-root container  |
| `Dockerfile`          | Single-stage Alpine + Python 3.12        |
| `requirements.txt`    | python-telegram-bot + dotenv             |
| `src/main.py`         | `/ping` handler — replace with your bot  |

## Logs

```bash
mnctl container logs my-tg-bot         # last 100 lines
mnctl container logs my-tg-bot -f      # follow
```
