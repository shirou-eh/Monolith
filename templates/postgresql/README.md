# PostgreSQL 16 Server

Monolith OS template for PostgreSQL with server-optimized configuration.

## Quick Start

```bash
mnctl template deploy postgresql --name my-postgres
```

## Default Credentials

- **User**: monolith
- **Password**: changeme — **change this immediately!**
- **Database**: monolith

## Configuration

The `postgresql.conf` is pre-tuned for server workloads. Adjust `shared_buffers`
and `effective_cache_size` based on your available RAM:

| RAM    | shared_buffers | effective_cache_size |
|--------|---------------|---------------------|
| 4 GB   | 1 GB          | 3 GB                |
| 8 GB   | 2 GB          | 6 GB                |
| 16 GB  | 4 GB          | 12 GB               |
| 32 GB  | 8 GB          | 24 GB               |
