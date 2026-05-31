# MariaDB 11 Server

Monolith OS template for MariaDB 11 with server-tuned defaults (utf8mb4,
2 GB InnoDB buffer pool, slow query log).

## Quick Start

```bash
mnctl template deploy mariadb --name my-mariadb
```

## Default Credentials

- **Root password**: `changeme` — **change this immediately!**
- **Application user**: `monolith` / `changeme`
- **Database**: `monolith`

## Configuration

Tune `--innodb-buffer-pool-size` based on available RAM:

| RAM    | innodb-buffer-pool-size |
|--------|-------------------------|
| 4 GB   | 1 GB                    |
| 8 GB   | 2 GB                    |
| 16 GB  | 6 GB                    |
| 32 GB  | 16 GB                   |

The bundled `my.cnf` enables the slow query log (>1 s) and `performance_schema`.
Override via `docker compose` environment variables or by editing `my.cnf`.

## Connecting

```bash
docker exec -it monolith-mariadb mariadb -u monolith -p monolith
```

Or from the host:

```bash
mariadb -h 127.0.0.1 -P 3306 -u monolith -p monolith
```

## Backups

```bash
docker exec monolith-mariadb mariadb-dump -u root -p --all-databases > dump.sql
```

Combine with `mnctl backup` for scheduled, encrypted, off-host snapshots.
