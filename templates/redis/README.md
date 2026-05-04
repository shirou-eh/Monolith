# Redis 7 Server

Monolith OS template for Redis with persistence and optimized configuration.

## Quick Start

```bash
mnctl template deploy redis --name my-redis
```

## Default Password

`changeme` — **change this in redis.conf!**

## Features

- AOF persistence enabled (everysec)
- RDB snapshots configured
- Memory limit: 1 GB with LRU eviction
- Health checks configured
