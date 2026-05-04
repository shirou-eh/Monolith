# Первые шаги с Monolith OS

## Обзор системы

```bash
mnctl info system       # ОС, ядро, время работы, оборудование
mnctl monitor status    # Загрузка CPU, RAM, диска
mnctl security audit    # Проверка безопасности
```

## Развёртывание приложений

```bash
mnctl template list
mnctl template deploy postgresql --name my-database
```

## Управление файрволом

```bash
mnctl security firewall status
mnctl security firewall allow 443
```

## Резервные копии

```bash
mnctl backup create --tag initial
mnctl backup list
```
