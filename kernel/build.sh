#!/usr/bin/env bash
# Monolith OS Kernel Build Script
# Builds a custom server-optimized kernel for Monolith OS
#
# Usage: ./build.sh [--version=VERSION] [--config-only] [--no-install]

set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly LOG_FILE="/var/log/monolith-kernel-build.log"
readonly KERNEL_SRC_DIR="/usr/src/monolith-kernel"
readonly PATCHES_DIR="${SCRIPT_DIR}/patches"
readonly CONFIGS_DIR="${SCRIPT_DIR}/configs"

# Colors
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly NC='\033[0m'

log() {
    local level="$1"
    shift
    local msg="$*"
    local timestamp
    timestamp="$(date '+%Y-%m-%d %H:%M:%S')"
    echo -e "${timestamp} [${level}] ${msg}" | tee -a "${LOG_FILE}"
}

info()  { log "INFO"  "${GREEN}${*}${NC}"; }
warn()  { log "WARN"  "${YELLOW}${*}${NC}"; }
error() { log "ERROR" "${RED}${*}${NC}"; }

die() {
    error "$@"
    exit 1
}

# Parse arguments
KERNEL_VERSION=""
CONFIG_ONLY=false
NO_INSTALL=false

for arg in "$@"; do
    case "${arg}" in
        --version=*)
            KERNEL_VERSION="${arg#*=}"
            ;;
        --config-only)
            CONFIG_ONLY=true
            ;;
        --no-install)
            NO_INSTALL=true
            ;;
        --help|-h)
            echo "Monolith Kernel Build Script"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --version=VERSION  Build specific kernel version (default: latest stable)"
            echo "  --config-only      Only generate config, don't build"
            echo "  --no-install       Build but don't install"
            echo "  --help, -h         Show this help"
            exit 0
            ;;
        *)
            die "Unknown argument: ${arg}"
            ;;
    esac
done

detect_latest_kernel() {
    info "Detecting latest stable kernel version..."
    local version
    version=$(curl -s https://www.kernel.org/releases.json \
        | grep -oP '"version":\s*"\K[0-9]+\.[0-9]+\.[0-9]+' \
        | head -1)

    if [[ -z "${version}" ]]; then
        die "Failed to detect latest kernel version from kernel.org"
    fi

    echo "${version}"
}

download_kernel() {
    local version="$1"
    local major
    major="$(echo "${version}" | cut -d. -f1)"
    local url="https://cdn.kernel.org/pub/linux/kernel/v${major}.x/linux-${version}.tar.xz"
    local sig_url="${url}.sign"
    local tarball="/tmp/linux-${version}.tar.xz"

    if [[ -f "${tarball}" ]]; then
        info "Kernel tarball already downloaded: ${tarball}"
        return
    fi

    info "Downloading kernel ${version}..."
    curl -L -o "${tarball}" "${url}" || die "Failed to download kernel"

    info "Downloading GPG signature..."
    curl -L -o "${tarball}.sign" "${sig_url}" 2>/dev/null || warn "GPG signature not available"

    if [[ -f "${tarball}.sign" ]]; then
        info "Verifying GPG signature..."
        # Import kernel.org keys
        gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys \
            647F28654894E3BD457199BE38DBBDC86092693E 2>/dev/null || true

        if xz -d -k "${tarball}" 2>/dev/null; then
            local uncompressed="${tarball%.xz}"
            if gpg --verify "${tarball}.sign" "${uncompressed}" 2>/dev/null; then
                info "GPG signature verified successfully"
            else
                warn "GPG signature verification failed — proceeding anyway"
            fi
            rm -f "${uncompressed}"
        fi
    fi
}

extract_kernel() {
    local version="$1"
    local tarball="/tmp/linux-${version}.tar.xz"

    info "Extracting kernel source..."
    mkdir -p "${KERNEL_SRC_DIR}"
    tar -xf "${tarball}" -C "${KERNEL_SRC_DIR}" --strip-components=1

    info "Kernel source extracted to ${KERNEL_SRC_DIR}"
}

apply_patches() {
    info "Applying kernel patches..."

    if [[ ! -d "${PATCHES_DIR}" ]]; then
        warn "No patches directory found at ${PATCHES_DIR}"
        return
    fi

    local patch_count=0
    for patch in "${PATCHES_DIR}"/*.patch; do
        if [[ -f "${patch}" ]]; then
            local patch_name
            patch_name="$(basename "${patch}")"
            info "Applying patch: ${patch_name}"

            if ! patch -p1 -d "${KERNEL_SRC_DIR}" < "${patch}" 2>>"${LOG_FILE}"; then
                warn "Patch ${patch_name} failed to apply cleanly — skipping"
            else
                ((patch_count++))
            fi
        fi
    done

    info "Applied ${patch_count} patches"
}

select_config() {
    local arch
    arch="$(uname -m)"

    case "${arch}" in
        x86_64)
            info "Using x86_64 kernel config"
            cp "${CONFIGS_DIR}/x86_64.config" "${KERNEL_SRC_DIR}/.config"
            ;;
        aarch64)
            info "Using ARM64 kernel config"
            cp "${CONFIGS_DIR}/arm64.config" "${KERNEL_SRC_DIR}/.config"
            ;;
        *)
            die "Unsupported architecture: ${arch}"
            ;;
    esac
}

build_kernel() {
    local version="$1"
    local jobs
    jobs="$(nproc)"

    info "Building kernel with ${jobs} jobs using clang..."
    cd "${KERNEL_SRC_DIR}"

    # Use LLVM/Clang if available, otherwise GCC
    if command -v clang &>/dev/null; then
        make -j"${jobs}" \
            CC=clang \
            LD=ld.lld \
            AR=llvm-ar \
            NM=llvm-nm \
            STRIP=llvm-strip \
            OBJCOPY=llvm-objcopy \
            OBJDUMP=llvm-objdump \
            READELF=llvm-readelf \
            HOSTCC=clang \
            HOSTCXX=clang++ \
            HOSTAR=llvm-ar \
            HOSTLD=ld.lld \
            2>&1 | tee -a "${LOG_FILE}"
    else
        make -j"${jobs}" 2>&1 | tee -a "${LOG_FILE}"
    fi

    info "Kernel build complete"
}

package_kernel() {
    local version="$1"

    info "Packaging kernel as pacman package..."

    local pkg_dir="/tmp/monolith-kernel-pkg"
    rm -rf "${pkg_dir}"
    mkdir -p "${pkg_dir}"

    cat > "${pkg_dir}/PKGBUILD" << EOF
pkgname=monolith-kernel
pkgver=${version//-/.}
pkgrel=1
pkgdesc="Monolith OS custom server-optimized kernel"
arch=('x86_64' 'aarch64')
license=('GPL2')
depends=('coreutils' 'kmod' 'initramfs')
provides=('linux')

package() {
    cd "${KERNEL_SRC_DIR}"
    make INSTALL_MOD_PATH="\${pkgdir}/usr" modules_install
    install -Dm644 arch/\$(uname -m | sed 's/x86_64/x86/')/boot/bzImage "\${pkgdir}/boot/vmlinuz-monolith"
    install -Dm644 .config "\${pkgdir}/boot/monolith-kernel.config"
}
EOF

    cd "${pkg_dir}"
    makepkg -sf --noconfirm 2>&1 | tee -a "${LOG_FILE}" || warn "makepkg failed"

    info "Kernel packaged"
}

install_kernel() {
    local version="$1"

    info "Creating restore point before kernel install..."
    snapper create --description "pre-kernel-${version}" --type pre 2>/dev/null || true

    info "Installing kernel..."

    local pkg_dir="/tmp/monolith-kernel-pkg"
    local pkg_file
    pkg_file=$(find "${pkg_dir}" -name "monolith-kernel-*.pkg.tar.*" -print -quit)

    if [[ -n "${pkg_file}" ]]; then
        pacman -U --noconfirm "${pkg_file}" 2>&1 | tee -a "${LOG_FILE}"
    else
        warn "No package file found, installing directly..."
        cd "${KERNEL_SRC_DIR}"
        make modules_install 2>&1 | tee -a "${LOG_FILE}"
        make install 2>&1 | tee -a "${LOG_FILE}"
    fi

    info "Updating bootloader..."
    if command -v grub-mkconfig &>/dev/null; then
        grub-mkconfig -o /boot/grub/grub.cfg 2>&1 | tee -a "${LOG_FILE}"
    elif command -v bootctl &>/dev/null; then
        bootctl update 2>/dev/null || true
    fi

    info "Generating initramfs..."
    mkinitcpio -p monolith 2>&1 | tee -a "${LOG_FILE}" || \
    mkinitcpio -P 2>&1 | tee -a "${LOG_FILE}" || true

    snapper create --description "post-kernel-${version}" --type post 2>/dev/null || true

    info "Kernel ${version} installed successfully. Reboot required."
}

main() {
    info "Monolith Kernel Build Script starting..."

    # Detect or use specified version
    if [[ -z "${KERNEL_VERSION}" ]]; then
        KERNEL_VERSION="$(detect_latest_kernel)"
    fi
    info "Target kernel version: ${KERNEL_VERSION}"

    if ${CONFIG_ONLY}; then
        select_config
        info "Config generated at ${KERNEL_SRC_DIR}/.config"
        exit 0
    fi

    download_kernel "${KERNEL_VERSION}"
    extract_kernel "${KERNEL_VERSION}"
    apply_patches
    select_config
    build_kernel "${KERNEL_VERSION}"
    package_kernel "${KERNEL_VERSION}"

    if ! ${NO_INSTALL}; then
        install_kernel "${KERNEL_VERSION}"
    fi

    info "Monolith kernel build complete!"
}

main "$@"
