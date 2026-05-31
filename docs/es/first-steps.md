# Primeros Pasos con Monolith OS

## Visión General del Sistema

```bash
mnctl info system       # SO, kernel, tiempo activo, hardware
mnctl monitor status    # Uso de CPU, RAM, disco
mnctl security audit    # Verificación de seguridad
```

## Desplegar Aplicaciones

```bash
mnctl template list
mnctl template deploy postgresql --name my-database
```

## Gestión del Firewall

```bash
mnctl security firewall status
mnctl security firewall allow 443
```

## Copias de Seguridad

```bash
mnctl backup create --tag initial
mnctl backup list
```
