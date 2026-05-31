# Monolith OS 快速开始

## 系统概览

```bash
mnctl info system       # 操作系统、内核、运行时间、硬件
mnctl monitor status    # CPU、内存、磁盘使用率
mnctl security audit    # 安全检查
```

## 部署应用

```bash
mnctl template list
mnctl template deploy postgresql --name my-database
```

## 防火墙管理

```bash
mnctl security firewall status
mnctl security firewall allow 443
```

## 备份

```bash
mnctl backup create --tag initial
mnctl backup list
```
