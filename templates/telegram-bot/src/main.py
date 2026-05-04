"""Monolith OS — Telegram bot starter.

Minimal async bot built on python-telegram-bot v21. Replace the
`/ping` handler with whatever your bot is supposed to do; the rest of
the file is intentionally tiny.
"""
from __future__ import annotations

import logging
import os
import signal
import sys

from dotenv import load_dotenv
from telegram import Update
from telegram.ext import Application, CommandHandler, ContextTypes


def configure_logging() -> None:
    logging.basicConfig(
        format="%(asctime)s %(levelname)s %(name)s :: %(message)s",
        level=os.environ.get("LOG_LEVEL", "INFO"),
    )


async def ping(update: Update, _ctx: ContextTypes.DEFAULT_TYPE) -> None:
    if update.message is not None:
        await update.message.reply_text("pong")


def main() -> int:
    load_dotenv()
    configure_logging()
    token = os.environ.get("TELEGRAM_TOKEN")
    if not token:
        logging.error("TELEGRAM_TOKEN is not set. See .env.example.")
        return 1

    app = Application.builder().token(token).build()
    app.add_handler(CommandHandler("ping", ping))

    # python-telegram-bot installs SIGINT/SIGTERM handlers automatically
    # via run_polling(), but we still want a clean shutdown if the
    # container receives SIGTERM during start-up.
    def _shutdown(signum, _frame):  # type: ignore[no-untyped-def]
        logging.info("received signal %s, exiting", signum)
        sys.exit(0)

    signal.signal(signal.SIGTERM, _shutdown)
    app.run_polling(close_loop=False)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
