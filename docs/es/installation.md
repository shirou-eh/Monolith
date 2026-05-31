# Guía de Instalación

## Requisitos

- Arch Linux (servidor físico o máquina virtual)
- Arquitectura x86_64 o ARM64
- RAM: mínimo 2 GB (recomendado 8 GB)
- Disco: mínimo 20 GB (recomendado 100 GB)

## Método 1: Instalación Automática

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

## Método 2: Compilar desde código fuente

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```
