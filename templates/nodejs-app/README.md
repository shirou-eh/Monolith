# Node.js Application

Generic Node.js application template with multi-stage Docker build.

## Quick Start

1. Place your Node.js application in this directory
2. Ensure `package.json` and entry point exist
3. Deploy:

```bash
mnctl template deploy nodejs-app --name my-app
```

## Customization

- Edit `Dockerfile` to match your project structure
- Adjust the `CMD` to your entry point
- Set environment variables in `docker-compose.yml`
