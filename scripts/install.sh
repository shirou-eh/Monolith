#!/usr/bin/env bash
# Monolith OS Bootstrap Installer
# Installs Monolith OS on an existing Arch Linux system
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/install.sh | bash
#   ./install.sh [OPTIONS]
#
# Options:
#   --non-interactive    Run without prompts
#   --hostname NAME      Set hostname
#   --user NAME          Create admin user
#   --components LIST    Comma-separated: monitoring,security,backup,all
#   --dry-run            Show what would be done without making changes
#   --help               Show this help

set -euo pipefail
IFS=$'\n\t'

readonly VERSION="1.0.0"
readonly CODENAME="Obsidian"
readonly LOG_FILE="/var/log/monolith-install.log"
# Repository URL (used by documentation/instructions)
export REPO_URL="https://github.com/shirou-eh/Monolith"

# Colors
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly BOLD='\033[1m'
readonly NC='\033[0m'

# Options
NON_INTERACTIVE=false
HOSTNAME_SET=""
USER_SET=""
COMPONENTS="all"
DRY_RUN=false

log() { echo -e "[$(date '+%H:%M:%S')] $*" | tee -a "${LOG_FILE}"; }
info()  { log "${GREEN}[INFO]${NC} $*"; }
warn()  { log "${YELLOW}[WARN]${NC} $*"; }
error() { log "${RED}[ERROR]${NC} $*"; }
die()   { error "$@"; exit 1; }

print_banner() {
    echo -e "${GREEN}"
    cat << 'BANNER'
    ███╗   ███╗ ██████╗ ███╗   ██╗ ██████╗ ██╗     ██╗████████╗██╗  ██╗
    ████╗ ████║██╔═══██╗████╗  ██║██╔═══██╗██║     ██║╚══██╔══╝██║  ██║
    ██╔████╔██║██║   ██║██╔██╗ ██║██║   ██║██║     ██║   ██║   ███████║
    ██║╚██╔╝██║██║   ██║██║╚██╗██║██║   ██║██║     ██║   ██║   ██╔══██║
    ██║ ╚═╝ ██║╚██████╔╝██║ ╚████║╚██████╔╝███████╗██║   ██║   ██║  ██║
    ╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═══╝ ╚═════╝ ╚══════╝╚═╝   ╚═╝   ╚═╝  ╚═╝
BANNER
    echo -e "${NC}"
    echo -e "    ${BOLD}Monolith OS v${VERSION} \"${CODENAME}\"${NC}"
    echo -e "    Built for the ones who mean it."
    echo ""
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --non-interactive) NON_INTERACTIVE=true ;;
            --hostname) HOSTNAME_SET="$2"; shift ;;
            --hostname=*) HOSTNAME_SET="${1#*=}" ;;
            --user) USER_SET="$2"; shift ;;
            --user=*) USER_SET="${1#*=}" ;;
            --components) COMPONENTS="$2"; shift ;;
            --components=*) COMPONENTS="${1#*=}" ;;
            --dry-run) DRY_RUN=true ;;
            --help|-h) print_banner; echo "See script header for usage."; exit 0 ;;
            *) die "Unknown option: $1" ;;
        esac
        shift
    done
}

check_requirements() {
    info "Checking requirements..."

    # Must be root
    if [[ $EUID -ne 0 ]]; then
        die "This script must be run as root"
    fi

    # Check architecture
    local arch
    arch="$(uname -m)"
    if [[ "${arch}" != "x86_64" && "${arch}" != "aarch64" ]]; then
        die "Unsupported architecture: ${arch}. Monolith supports x86_64 and ARM64."
    fi
    info "Architecture: ${arch}"

    # Check if Arch Linux
    if [[ -f /etc/arch-release ]]; then
        info "Arch Linux detected"
    else
        warn "This does not appear to be Arch Linux. Monolith is designed for Arch."
        if ! ${NON_INTERACTIVE}; then
            read -rp "Continue anyway? [y/N] " reply
            if [[ ! "${reply}" =~ ^[Yy]$ ]]; then
                exit 1
            fi
        fi
    fi

    # Check disk space
    local avail_gb
    avail_gb=$(df / --output=avail -BG | tail -1 | tr -d ' G')
    if [[ ${avail_gb} -lt 10 ]]; then
        die "Insufficient disk space: ${avail_gb}G available, 10G minimum required"
    fi
    info "Available disk space: ${avail_gb}G"

    # Check RAM
    local total_ram_mb
    total_ram_mb=$(awk '/MemTotal/ {print int($2/1024)}' /proc/meminfo)
    if [[ ${total_ram_mb} -lt 1024 ]]; then
        warn "Low RAM: ${total_ram_mb}MB (2GB recommended)"
    fi
    info "Total RAM: ${total_ram_mb}MB"
}

install_base_packages() {
    info "Installing base packages..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would install base packages"
        return
    fi

    pacman -Sy --noconfirm --needed \
        base-devel \
        git \
        curl \
        wget \
        vim \
        tmux \
        htop \
        tree \
        jq \
        unzip \
        rsync \
        nftables \
        fail2ban \
        apparmor \
        docker \
        docker-compose \
        nginx \
        certbot \
        certbot-nginx \
        prometheus \
        grafana \
        wireguard-tools \
        restic \
        snapper \
        btrfs-progs \
        rust \
        cargo \
        shellcheck \
        openssh \
        2>&1 | tee -a "${LOG_FILE}"

    info "Base packages installed"
}

configure_system() {
    info "Configuring system..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would configure system"
        return
    fi

    # Create Monolith directory structure
    mkdir -p /etc/monolith/services
    mkdir -p /var/log/monolith
    mkdir -p /var/lib/monolith/{deployments,etcd}
    mkdir -p /usr/share/monolith/templates

    # Copy configurations
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

    if [[ -d "${script_dir}/config" ]]; then
        cp "${script_dir}/config/pacman/pacman.conf" /etc/pacman.conf 2>/dev/null || true
        cp "${script_dir}/config/pacman/makepkg.conf" /etc/makepkg.conf 2>/dev/null || true
        cp "${script_dir}/config/monolith/monolith.toml" /etc/monolith/monolith.toml 2>/dev/null || true
        cp "${script_dir}/config/monolith/ssh-banner.txt" /etc/monolith/ssh-banner.txt 2>/dev/null || true
    fi

    # Set hostname
    if [[ -n "${HOSTNAME_SET}" ]]; then
        hostnamectl set-hostname "${HOSTNAME_SET}"
        info "Hostname set to: ${HOSTNAME_SET}"
    fi

    # Create admin user
    if [[ -n "${USER_SET}" ]]; then
        useradd -m -G wheel -s /bin/bash "${USER_SET}" 2>/dev/null || true
        info "User created: ${USER_SET}"
    fi

    info "System configured"
}

configure_security() {
    info "Configuring security..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would configure security"
        return
    fi

    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

    # SSH hardening
    if [[ -f "${script_dir}/security/ssh/sshd_config" ]]; then
        cp "${script_dir}/security/ssh/sshd_config" /etc/ssh/sshd_config
        systemctl restart sshd 2>/dev/null || true
    fi

    # Firewall
    if [[ -f "${script_dir}/security/nftables/monolith.nft" ]]; then
        cp "${script_dir}/security/nftables/monolith.nft" /etc/nftables.conf
        systemctl enable --now nftables 2>/dev/null || true
    fi

    # sysctl hardening
    if [[ -d "${script_dir}/security/sysctl" ]]; then
        cp "${script_dir}/security/sysctl/"*.conf /etc/sysctl.d/ 2>/dev/null || true
        sysctl --system 2>/dev/null || true
    fi

    # AppArmor profiles
    if [[ -d "${script_dir}/security/apparmor" ]]; then
        cp "${script_dir}/security/apparmor/"* /etc/apparmor.d/ 2>/dev/null || true
        systemctl enable --now apparmor 2>/dev/null || true
    fi

    # Fail2ban
    systemctl enable --now fail2ban 2>/dev/null || true

    # Limits
    if [[ -f "${script_dir}/security/limits/monolith.conf" ]]; then
        cp "${script_dir}/security/limits/monolith.conf" /etc/security/limits.d/ 2>/dev/null || true
    fi

    info "Security configured"
}

configure_monitoring() {
    info "Configuring monitoring stack..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would configure monitoring"
        return
    fi

    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

    if [[ -d "${script_dir}/monitoring" ]]; then
        mkdir -p /etc/prometheus/rules
        cp "${script_dir}/monitoring/prometheus/prometheus.yml" /etc/prometheus/ 2>/dev/null || true
        cp "${script_dir}/monitoring/prometheus/rules/"*.yml /etc/prometheus/rules/ 2>/dev/null || true
        cp -r "${script_dir}/monitoring/grafana" /etc/monolith/services/ 2>/dev/null || true
        cp "${script_dir}/monitoring/loki/"*.yml /etc/monolith/services/ 2>/dev/null || true
    fi

    systemctl enable --now prometheus 2>/dev/null || true
    systemctl enable --now grafana 2>/dev/null || true

    info "Monitoring configured"
}

configure_backup() {
    info "Configuring backup system..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would configure backup"
        return
    fi

    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

    # Snapper
    if command -v snapper &>/dev/null && findmnt -n -o FSTYPE / | grep -q btrfs; then
        snapper -c root create-config / 2>/dev/null || true
        if [[ -f "${script_dir}/backup/snapper/root-config" ]]; then
            cp "${script_dir}/backup/snapper/root-config" /etc/snapper/configs/root 2>/dev/null || true
        fi
        systemctl enable --now snapper-timeline.timer 2>/dev/null || true
        systemctl enable --now snapper-cleanup.timer 2>/dev/null || true
    fi

    # Systemd timer for backups
    if [[ -d "${script_dir}/config/systemd" ]]; then
        cp "${script_dir}/config/systemd/monolith-backup."* /etc/systemd/system/ 2>/dev/null || true
        systemctl daemon-reload
        systemctl enable monolith-backup.timer 2>/dev/null || true
    fi

    info "Backup configured"
}

install_monolith_tools() {
    info "Building and installing Monolith tools..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would build and install mnctl, mnpkg, mntui"
        return
    fi

    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

    if [[ -f "${script_dir}/Cargo.toml" ]]; then
        cd "${script_dir}"
        cargo build --release 2>&1 | tee -a "${LOG_FILE}"

        install -Dm755 target/release/mnctl /usr/bin/mnctl
        install -Dm755 target/release/mnpkg /usr/bin/mnpkg
        install -Dm755 target/release/mntui /usr/bin/mntui 2>/dev/null || true
        install -Dm755 target/release/monolith-installer /usr/bin/monolith-installer 2>/dev/null || true

        info "Monolith tools installed"
    else
        warn "Cargo.toml not found — skipping Rust tool build"
    fi
}

enable_services() {
    info "Enabling services..."

    if ${DRY_RUN}; then
        info "[DRY RUN] Would enable services"
        return
    fi

    systemctl enable --now docker 2>/dev/null || true
    systemctl enable --now sshd 2>/dev/null || true
    systemctl enable --now nftables 2>/dev/null || true

    info "Services enabled"
}

print_summary() {
    echo ""
    echo -e "${GREEN}════════════════════════════════════════════════════════════${NC}"
    echo -e "${BOLD}  Monolith OS v${VERSION} \"${CODENAME}\" — Installation Complete${NC}"
    echo -e "${GREEN}════════════════════════════════════════════════════════════${NC}"
    echo ""
    echo "  Next steps:"
    echo "    1. mnctl info system          — Check system info"
    echo "    2. mnctl monitor status       — View system status"
    echo "    3. mnctl security audit       — Run security audit"
    echo "    4. mnctl template list        — Browse app templates"
    echo ""
    echo "  SSH access (port 2222):"
    echo "    ssh admin@$(hostname -I | awk '{print $1}') -p 2222"
    echo ""
    echo "  Documentation: https://github.com/shirou-eh/Monolith"
    echo ""
}

main() {
    print_banner
    parse_args "$@"

    mkdir -p "$(dirname "${LOG_FILE}")"
    touch "${LOG_FILE}"

    info "Monolith OS installer v${VERSION} starting..."

    check_requirements
    install_base_packages
    configure_system
    install_monolith_tools

    if [[ "${COMPONENTS}" == "all" || "${COMPONENTS}" == *"security"* ]]; then
        configure_security
    fi
    if [[ "${COMPONENTS}" == "all" || "${COMPONENTS}" == *"monitoring"* ]]; then
        configure_monitoring
    fi
    if [[ "${COMPONENTS}" == "all" || "${COMPONENTS}" == *"backup"* ]]; then
        configure_backup
    fi

    enable_services
    print_summary

    info "Installation complete! Log: ${LOG_FILE}"
}

main "$@"
