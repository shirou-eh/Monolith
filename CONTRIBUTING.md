# Contributing to Monolith OS

Thank you for your interest in contributing to Monolith OS! This guide will help you get started.

## Development Setup

### Prerequisites

- Rust toolchain (stable, 2021 edition)
- ShellCheck (for shell script linting)
- Docker (for template testing)

### Building from Source

```bash
git clone https://github.com/shirou-eh/Monolith.git
cd monolith
make build          # Debug build
make build-release  # Release build
make lint           # Run clippy and shellcheck
make fmt-check      # Check formatting
make test           # Run tests
```

### Project Structure

- `mnctl/` — Main CLI tool and TUI dashboard (Rust workspace member)
- `mnpkg/` — Package manager wrapper (Rust workspace member)
- `installer/` — TUI installer (Rust workspace member)
- `kernel/` — Kernel configs and build scripts
- `security/` — Security configuration files
- `monitoring/` — Prometheus, Grafana, Loki configs
- `templates/` — Docker Compose application templates

### Code Style

- Rust: Follow standard Rust conventions. `cargo fmt` and `cargo clippy` must pass.
- Shell: All scripts must pass `shellcheck -S warning`.
- Config files: Use consistent indentation (2 spaces for YAML, tabs for Makefile).

## Submitting Changes

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes
4. Ensure all checks pass: `make lint && make build && make test`
5. Commit with a descriptive message
6. Push and open a Pull Request

## Reporting Bugs

Use the [Bug Report](https://github.com/shirou-eh/Monolith/issues/new?template=bug_report.md) template.

## Suggesting Features

Use the [Feature Request](https://github.com/shirou-eh/Monolith/issues/new?template=feature_request.md) template.

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).
