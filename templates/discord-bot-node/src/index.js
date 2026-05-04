// Monolith OS — Discord bot starter (discord.js v14)
//
// Replace the `!ping` handler with whatever you actually want the bot to
// do. The shape is intentionally tiny — that's the point of this
// template. Production niceties (gateway intent flags, graceful
// shutdown, structured logging) are wired up so you don't have to.
import 'dotenv/config';
import { Client, Events, GatewayIntentBits, Partials } from 'discord.js';

const token = process.env.DISCORD_TOKEN;
if (!token) {
  console.error('DISCORD_TOKEN is not set. See .env.example.');
  process.exit(1);
}

const client = new Client({
  intents: [
    GatewayIntentBits.Guilds,
    GatewayIntentBits.GuildMessages,
    GatewayIntentBits.MessageContent,
  ],
  partials: [Partials.Channel],
});

client.once(Events.ClientReady, (c) => {
  console.log(JSON.stringify({ level: 'info', msg: 'bot ready', user: c.user.tag }));
});

client.on(Events.MessageCreate, async (message) => {
  if (message.author.bot) return;
  if (message.content === '!ping') {
    await message.reply({ content: 'pong', allowedMentions: { repliedUser: false } });
  }
});

const shutdown = (signal) => {
  console.log(JSON.stringify({ level: 'info', msg: 'shutting down', signal }));
  client
    .destroy()
    .catch(() => {})
    .finally(() => process.exit(0));
};
process.on('SIGINT', () => shutdown('SIGINT'));
process.on('SIGTERM', () => shutdown('SIGTERM'));

client.login(token).catch((err) => {
  console.error(JSON.stringify({ level: 'error', msg: 'login failed', err: String(err) }));
  process.exit(1);
});
