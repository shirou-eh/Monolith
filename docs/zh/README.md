# Monolith OS — 文档

欢迎阅读 Monolith OS 文档。

## 目录

1. [安装指南](installation.md)
2. [快速开始](first-steps.md)

## 快速开始

### 在现有 Arch Linux 上安装

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

### 从源代码编译

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```

### 基本命令

```bash
mnctl info system          # 系统信息
mnctl monitor status       # 资源监控
mnctl security audit       # 安全审计
mnctl template list        # 应用模板列表
```

## 获取帮助

- GitHub Issues: https://github.com/shirou-eh/Monolith/issues
- 命令帮助: `mnctl --help`
