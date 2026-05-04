# Nginx Reverse Proxy with Automatic TLS

Production nginx reverse proxy with Let's Encrypt certificate automation.

## Quick Start

```bash
mnctl template deploy nginx-reverse-proxy --name proxy
```

## Adding Sites

Use mnctl to add reverse proxy rules:

```bash
mnctl proxy add example.com http://127.0.0.1:3000
```

Or manually add `.conf` files to `conf.d/`.

## Certificate Management

```bash
# Initial certificate
docker exec monolith-certbot certbot certonly --webroot -w /var/www/certbot -d example.com

# Renewal
docker exec monolith-certbot certbot renew
docker exec monolith-nginx nginx -s reload
```
