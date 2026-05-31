# Discord bot — Node.js / discord.js v14
#
# Two-stage build: install prod deps in the builder, then copy them into
# a slim runtime image. Final image is ~80 MB and runs as a non-root user.
FROM node:20-alpine AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci --omit=dev --no-audit --no-fund

FROM node:20-alpine
RUN addgroup -S bot && adduser -S bot -G bot
WORKDIR /app
COPY --from=builder /app/node_modules ./node_modules
COPY --chown=bot:bot . .
USER bot
ENV NODE_ENV=production
CMD ["node", "src/index.js"]
