# Руководство по установке

## Требования

- Arch Linux (физический сервер или виртуальная машина)
- Архитектура x86_64 или ARM64
- Оперативная память: минимум 2 ГБ (рекомендуется 8 ГБ)
- Диск: минимум 20 ГБ (рекомендуется 100 ГБ)

## Метод 1: Автоматическая установка

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

## Метод 2: Сборка из исходников

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```
