# Monolith OS — Документация

Добро пожаловать в документацию Monolith OS.

## Содержание

1. [Руководство по установке](installation.md)
2. [Первые шаги](first-steps.md)

## Быстрый старт

### Установка на существующую Arch Linux

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

### Сборка из исходников

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```

### Первые команды

```bash
mnctl info system          # Информация о системе
mnctl monitor status       # Состояние ресурсов
mnctl security audit       # Аудит безопасности
mnctl template list        # Список шаблонов
```

## Получение помощи

- GitHub Issues: https://github.com/shirou-eh/Monolith/issues
- Справка по командам: `mnctl --help`
