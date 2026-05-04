# Monolith OS — Makefile
# v1.0.0 "Obsidian"

.PHONY: build build-release install uninstall test lint fmt fmt-check clean help

PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
SHAREDIR ?= $(PREFIX)/share/monolith
ETCDIR ?= /etc/monolith

# Build targets
build:
	cargo build --workspace

build-release:
	cargo build --workspace --release

# Install
install: build-release
	install -Dm755 target/release/mnctl $(DESTDIR)$(BINDIR)/mnctl
	install -Dm755 target/release/mnpkg $(DESTDIR)$(BINDIR)/mnpkg
	install -Dm755 target/release/mntui $(DESTDIR)$(BINDIR)/mntui || true
	install -Dm755 target/release/mnweb $(DESTDIR)$(BINDIR)/mnweb || true
	install -Dm755 target/release/monolith-installer $(DESTDIR)$(BINDIR)/monolith-installer || true
	install -d $(DESTDIR)$(SHAREDIR)/templates
	cp -r templates/* $(DESTDIR)$(SHAREDIR)/templates/ 2>/dev/null || true
	install -d $(DESTDIR)$(SHAREDIR)/kernel
	cp -r kernel/* $(DESTDIR)$(SHAREDIR)/kernel/ 2>/dev/null || true
	install -d $(DESTDIR)$(SHAREDIR)/iso
	cp -r iso/* $(DESTDIR)$(SHAREDIR)/iso/ 2>/dev/null || true
	install -d $(DESTDIR)$(ETCDIR)
	install -Dm644 config/monolith/monolith.toml $(DESTDIR)$(ETCDIR)/monolith.toml 2>/dev/null || true
	install -Dm644 config/monolith/ssh-banner.txt $(DESTDIR)$(ETCDIR)/ssh-banner.txt 2>/dev/null || true

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/mnctl
	rm -f $(DESTDIR)$(BINDIR)/mnpkg
	rm -f $(DESTDIR)$(BINDIR)/mntui
	rm -f $(DESTDIR)$(BINDIR)/mnweb
	rm -f $(DESTDIR)$(BINDIR)/monolith-installer
	rm -rf $(DESTDIR)$(SHAREDIR)

# Testing
test:
	cargo test --workspace

# Linting
lint: fmt-check
	cargo clippy --workspace --all-targets -- -D warnings
	@echo "Checking shell scripts with ShellCheck..."
	@find . -name '*.sh' -type f -exec shellcheck -S warning {} + 2>/dev/null || echo "shellcheck not installed"

# Formatting
fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

# Cleanup
clean:
	cargo clean

# Help
help:
	@echo "Monolith OS Build System"
	@echo ""
	@echo "Targets:"
	@echo "  build          Build all crates (debug)"
	@echo "  build-release  Build all crates (release)"
	@echo "  install        Install binaries and configs"
	@echo "  uninstall      Remove installed files"
	@echo "  test           Run tests"
	@echo "  lint           Run clippy and shellcheck"
	@echo "  fmt            Format code"
	@echo "  fmt-check      Check formatting"
	@echo "  clean          Remove build artifacts"
	@echo "  help           Show this help"
