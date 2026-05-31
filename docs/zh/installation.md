# 安装指南

## 系统要求

- Arch Linux（物理服务器或虚拟机）
- x86_64 或 ARM64 架构
- 内存：最低 2 GB（推荐 8 GB）
- 磁盘：最低 20 GB（推荐 100 GB）

## 方法一：自动安装

```bash
curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | sudo bash
```

## 方法二：从源代码编译

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build-release
sudo make install
```
