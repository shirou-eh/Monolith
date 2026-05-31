# Monolith OS — Documentación

Bienvenido a la documentación de Monolith OS.

## Contenido

1. [Guía de Instalación](installation.md)
2. [Primeros Pasos](first-steps.md)

## Inicio Rápido

### Instalación en Arch Linux existente

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

### Compilar desde código fuente

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```

### Comandos básicos

```bash
mnctl info system          # Información del sistema
mnctl monitor status       # Estado de recursos
mnctl security audit       # Auditoría de seguridad
mnctl template list        # Plantillas de aplicaciones
```

## Obtener ayuda

- GitHub Issues: https://github.com/shirou-eh/Monolith/issues
- Ayuda de comandos: `mnctl --help`
