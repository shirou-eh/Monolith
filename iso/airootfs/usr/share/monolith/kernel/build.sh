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
    # stderr, not stdout: detect_latest_kernel() below returns its version
    # string via `echo` and gets captured with `$(...)`. If these log lines
    # went to stdout too they'd be captured right along with it, so
    # KERNEL_VERSION ended up as the whole log transcript glued to the
    # version number instead of just the version number.
    echo -e "${timestamp} [${level}] ${msg}" | tee -a "${LOG_FILE}" >&2
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

    # configs/*.config is a curated fragment (the options Monolith actually
    # cares about), not a complete .config — every symbol it doesn't mention
    # is unset. Feeding that straight to `make` would stop at the first
    # unanswered prompt and hang forever under a non-interactive build.
    # olddefconfig fills every gap with the kernel's own upstream default,
    # keeping just the fragment's choices pinned.
    info "Resolving full config from fragment (olddefconfig)..."
    cd "${KERNEL_SRC_DIR}"
    if use_llvm; then
        make LLVM=1 olddefconfig 2>&1 | tee -a "${LOG_FILE}"
    else
        make olddefconfig 2>&1 | tee -a "${LOG_FILE}"
    fi
}

# Whether to build with the LLVM toolchain (clang/lld) instead of GCC.
# `clang` and `ld.lld` alone aren't enough — the kernel build also needs
# llvm-ar/llvm-nm/llvm-strip/etc, which come from a separate `llvm`
# package on Arch and aren't pulled in by `clang`/`lld` alone. Checking
# only for clang (as this script used to) meant a host with clang but not
# the full llvm-* set would get partway into the build and die on the
# first missing tool (llvm-ar, typically) instead of never starting down
# that path at all.
use_llvm() {
    command -v clang &>/dev/null && command -v ld.lld &>/dev/null && command -v llvm-ar &>/dev/null
}

build_kernel() {
    local version="$1"
    local jobs
    jobs="$(nproc)"

    cd "${KERNEL_SRC_DIR}"

    # LLVM=1 is the kernel build system's own switch for "use the full
    # clang/lld/llvm-* toolchain" — one flag instead of pinning CC/LD/AR/
    # NM/STRIP/OBJCOPY/OBJDUMP/READELF/HOSTCC/HOSTCXX/HOSTAR/HOSTLD by
    # hand, which is also how the previous version of this script drifted
    # out of sync with what was actually installed.
    if use_llvm; then
        info "Building kernel with ${jobs} jobs using LLVM (clang/lld)..."
        make -j"${jobs}" LLVM=1 2>&1 | tee -a "${LOG_FILE}"
    else
        info "Building kernel with ${jobs} jobs using GCC (llvm-ar/nm/strip not found — see kernel/build.sh use_llvm())..."
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

    # makepkg refuses to run as root outright (it exits non-zero before
    # doing anything) — and this whole script normally runs as root, via
    # `sudo mnctl update kernel`. Build as the invoking unprivileged user
    # instead ($SUDO_USER is what sudo sets for exactly this), then the
    # caller (install_kernel) installs the resulting package as root the
    # normal way. If there's no such user — run directly as root, no
    # sudo — there's no one to build as; skip packaging and fall back to
    # the direct-install path below instead of failing the whole build.
    local build_user="${SUDO_USER:-}"
    if [[ -z "${build_user}" || "${build_user}" == "root" ]]; then
        warn "No unprivileged build user available (run this via 'sudo', not as root directly) — skipping the .pkg.tar.zst, installing the kernel directly instead"
        return
    fi

    chown -R "${build_user}:${build_user}" "${pkg_dir}"
    cd "${pkg_dir}"
    if su "${build_user}" -c "cd '${pkg_dir}' && makepkg -sf --noconfirm" 2>&1 | tee -a "${LOG_FILE}"; then
        info "Kernel packaged"
    else
        warn "makepkg failed — installing directly instead"
    fi
}

install_kernel() {
    local version="$1"
    local krelease="${version}-monolith"

    info "Creating restore point before kernel install..."
    snapper create --description "pre-kernel-${version}" --type pre 2>/dev/null || true

    info "Installing kernel..."

    local pkg_dir="/tmp/monolith-kernel-pkg"
    local pkg_file
    pkg_file=$(find "${pkg_dir}" -name "monolith-kernel-*.pkg.tar.*" -print -quit 2>/dev/null || true)

    if [[ -n "${pkg_file}" ]]; then
        pacman -U --noconfirm "${pkg_file}" 2>&1 | tee -a "${LOG_FILE}"
    else
        warn "No package file — installing modules and vmlinuz directly (no pacman entry, so 'pacman -R monolith-kernel' won't remove it — see the snapper pre-kernel snapshot to roll back instead)"
        cd "${KERNEL_SRC_DIR}"
        make modules_install 2>&1 | tee -a "${LOG_FILE}"

        # `make install` on a hand-built source tree depends on
        # /sbin/installkernel being set up the way Arch's own `linux`
        # package sets it up — a bare kernel.org tree doesn't have that,
        # so `make install` here silently does nothing (exits 0, copies
        # nothing) rather than failing loudly. Copy the file it would
        # have installed, directly.
        local arch_boot_dir
        arch_boot_dir="arch/$(uname -m | sed 's/x86_64/x86/')/boot"
        if [[ ! -f "${arch_boot_dir}/bzImage" ]]; then
            die "build didn't produce ${arch_boot_dir}/bzImage — nothing to install"
        fi
        install -Dm644 "${arch_boot_dir}/bzImage" "/boot/vmlinuz-monolith"
        [[ -f System.map ]] && install -Dm644 System.map "/boot/System.map-monolith"
        info "Installed /boot/vmlinuz-monolith"
    fi

    info "Generating initramfs..."
    # There's no /etc/mkinitcpio.d/monolith.preset on a stock Monolith
    # install, so `-p monolith` always failed here and silently fell
    # back to `-P` — which rebuilds every *existing* preset (i.e. the
    # currently-running kernels) and does nothing for the one just
    # installed. Target the new kernel by version/output path explicitly
    # instead, the same way /etc/mkinitcpio.d/linux.preset does.
    if [[ ! -d "/lib/modules/${krelease}" ]]; then
        die "modules for ${krelease} not found under /lib/modules — modules_install must have failed"
    fi
    mkinitcpio -k "${krelease}" -g "/boot/initramfs-monolith.img" 2>&1 | tee -a "${LOG_FILE}"

    info "Updating bootloader..."
    if command -v grub-mkconfig &>/dev/null; then
        grub-mkconfig -o /boot/grub/grub.cfg 2>&1 | tee -a "${LOG_FILE}"
    elif command -v bootctl &>/dev/null; then
        bootctl update 2>/dev/null || true
    fi

    snapper create --description "post-kernel-${version}" --type post 2>/dev/null || true

    info "Kernel ${version} installed: /boot/vmlinuz-monolith + /boot/initramfs-monolith.img."
    info "Your current kernel is untouched and stays the boot default — select 'monolith' from the bootloader menu to try the new one. Reboot required either way to pick this up."
}

check_prerequisites() {
    # Everything the build needs that isn't part of base-devel and so
    # isn't guaranteed present on a fresh Monolith install. Checked up
    # front so a missing tool is a 1-second failure with a clear fix,
    # not a 10-minute build that dies on whichever Kbuild step happens
    # to need `bc` or `cpio` first.
    local required=(curl gpg tar xz patch make flex bison perl bc cpio)
    local missing=()
    for tool in "${required[@]}"; do
        command -v "${tool}" &>/dev/null || missing+=("${tool}")
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        die "Missing required tool(s): ${missing[*]}. Install with: pacman -S ${missing[*]}"
    fi
}

main() {
    info "Monolith Kernel Build Script starting..."
    check_prerequisites

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
