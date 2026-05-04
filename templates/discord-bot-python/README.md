# Python Discord Bot

Template for Python Discord bots using disnake, discord.py, or nextcord.

## Quick Start

1. Copy `.env.template` to `.env` and set your bot token
2. Place your bot code in this directory with `main.py` as entry point
3. Add dependencies to `requirements.txt`
4. Deploy:

```bash
cp .env.template .env
# Edit .env with your bot token
mnctl template deploy discord-bot-python --name my-bot
```

## Data Persistence

Bot data is stored in the `bot-data` volume at `/app/data` inside the container.
