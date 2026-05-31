# MongoDB 7 Server

Monolith OS template for MongoDB 7 with authentication enabled, WiredTiger
tuning, and slow operation profiling.

## Quick Start

```bash
mnctl template deploy mongodb --name my-mongo
```

## Default Credentials

- **Root user**: `monolith` / `changeme` — **change this immediately!**
- **Database**: `monolith`

Authentication is enabled by default. Update the credentials in
`docker-compose.yml` before exposing the server publicly.

## Tuning

Edit `mongod.conf` to adjust the WiredTiger cache (`cacheSizeGB`). A common
guideline is 50% of host RAM, capped at the working set size.

| RAM    | wiredTiger.engineConfig.cacheSizeGB |
|--------|------------------------------------|
| 4 GB   | 1 GB                               |
| 8 GB   | 2 GB                               |
| 16 GB  | 6 GB                               |
| 32 GB  | 16 GB                              |

## Connecting

```bash
docker exec -it monolith-mongodb mongosh -u monolith -p
```

Connection URI:

```
mongodb://monolith:changeme@localhost:27017/monolith?authSource=admin
```

## Backups

```bash
docker exec monolith-mongodb mongodump --uri="mongodb://monolith:changeme@localhost:27017" --archive=/tmp/dump.archive
```

Pair with `mnctl backup` for off-host scheduled snapshots.

## Optional: Mongo Express

Uncomment the `mongo-express` service in `docker-compose.yml` to enable a
web admin UI on <http://localhost:8081>.
