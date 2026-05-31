#!/usr/bin/env bash
# Monolith OS — ISO builder
#
# Wraps `mkarchiso` to produce a bootable Monolith OS install medium. The
# resulting ISO image lands in $OUT_DIR (default: ./out).
#
# Requirements (host):
#   * Arch Linux (or container with archiso installed)
#   * archiso          (provides mkarchiso)
#   * grub-mkrescue + edk2-shell  (UEFI bootmodes)
#   * Root privileges (mkarchiso uses chroots)
#
# Usage:
#   ./iso/build-iso.sh [--profile PATH] [--out DIR] [--release-tar PATH]
#                      [--tier lite|full|pro] [--label NAME] [--version STR]
#
# --tier picks the resource profile baked into the ISO's default
# /etc/monolith/monolith.toml and trims the package list for the lite
# tier (no monitoring stack on disk). The default tier is "full".
#
# Most users should invoke this via `mnctl iso build` instead.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE_DIR_DEFAULT="${REPO_ROOT}/iso/profile"
AIROOTFS_TEMPLATE="${REPO_ROOT}/iso/airootfs"
CONFIG_TEMPLATE="${REPO_ROOT}/config/monolith/monolith.toml"
OUT_DIR_DEFAULT="${REPO_ROOT}/out"

PROFILE_DIR="${PROFILE_DIR_DEFAULT}"
OUT_DIR="${OUT_DIR_DEFAULT}"
RELEASE_TAR=""
TIER="full"
ISO_LABEL_OVERRIDE=""
ISO_VERSION_OVERRIDE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile) PROFILE_DIR="$2"; shift 2 ;;
        --out) OUT_DIR="$2"; shift 2 ;;
        --release-tar) RELEASE_TAR="$2"; shift 2 ;;
        --tier) TIER="$2"; shift 2 ;;
        --label) ISO_LABEL_OVERRIDE="$2"; shift 2 ;;
        --version) ISO_VERSION_OVERRIDE="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,25p' "$0"
            exit 0 ;;
        *) echo "Unknown argument: $1" >&2; exit 2 ;;
    esac
done

case "${TIER}" in
    lite|full|pro) ;;
    *) echo "Unknown --tier '${TIER}'. Use one of: lite, full, pro." >&2; exit 2 ;;
esac

if [[ $EUID -ne 0 ]]; then
    echo "build-iso.sh must run as root (mkarchiso uses chroots)." >&2
    exit 1
fi

if ! command -v mkarchiso >/dev/null 2>&1; then
    echo "mkarchiso not found. On Arch: pacman -S archiso" >&2
    exit 1
fi

WORK_DIR="$(mktemp -d -t monolith-iso-XXXXXXXX)"
PROFILE_WORK="${WORK_DIR}/profile"
trap 'rm -rf "${WORK_DIR}"' EXIT

echo "[*] Preparing profile at ${PROFILE_WORK} (tier=${TIER})"

# The Monolith profile only customises package list, profiledef and a
# handful of airootfs files. mkarchiso requires the rest of the archiso
# scaffolding (boot loaders, automated_script, pacman keyring config,
# etc.). Lay down the upstream `releng` profile as a baseline first and
# then overlay the Monolith profile on top — that way we always get a
# bootable, validatable image without having to vendor copies of the
# upstream config that just shadow it.
RELENG_DIR="/usr/share/archiso/configs/releng"
if [[ -d "${RELENG_DIR}" ]]; then
    echo "[*] Laying down archiso releng baseline"
    cp -a "${RELENG_DIR}/." "${PROFILE_WORK}/"
fi

# Now overlay the Monolith profile (packages.x86_64, profiledef.sh,
# pacman.conf, plus anything else the maintainer adds later).
cp -a "${PROFILE_DIR}/." "${PROFILE_WORK}/"

# Merge airootfs template into the profile so users can customize the
# profile dir without having to also copy airootfs.
mkdir -p "${PROFILE_WORK}/airootfs"
if [[ -d "${AIROOTFS_TEMPLATE}" ]]; then
    cp -a "${AIROOTFS_TEMPLATE}/." "${PROFILE_WORK}/airootfs/"
fi

# Bake /etc/monolith/monolith.toml with the right profile preset so
# `mnctl profile show` on first boot reports the same tier the user
# downloaded.
if [[ -f "${CONFIG_TEMPLATE}" ]]; then
    install -d "${PROFILE_WORK}/airootfs/etc/monolith"
    sed "s/^profile = \".*\"$/profile = \"${TIER}\"/" "${CONFIG_TEMPLATE}" \
        > "${PROFILE_WORK}/airootfs/etc/monolith/monolith.toml"
fi

# The lite tier strips the monitoring stack and other heavy packages
# from the ISO image to keep the download small and the on-disk
# footprint under ~1 GB. mkarchiso reads packages.x86_64 line-by-line
# so we rewrite that file in the work directory.
if [[ "${TIER}" == "lite" ]]; then
    LITE_DROPS_RE='^(prometheus|grafana|loki|smartmontools|nvme-cli|cockpit|firefox|gnome|plasma|texlive)'
    if [[ -f "${PROFILE_WORK}/packages.x86_64" ]]; then
        grep -Ev "${LITE_DROPS_RE}" "${PROFILE_WORK}/packages.x86_64" \
            > "${PROFILE_WORK}/packages.x86_64.lite"
        mv "${PROFILE_WORK}/packages.x86_64.lite" "${PROFILE_WORK}/packages.x86_64"
    fi
fi

# Tier-specific MOTD line so users can tell at a glance which ISO they
# booted into.
if [[ -f "${PROFILE_WORK}/airootfs/etc/motd" ]]; then
    {
        echo
        printf "  Resource profile: %s\n" "${TIER}"
    } >> "${PROFILE_WORK}/airootfs/etc/motd"
fi

# Tier-specific ISO label so the resulting file in $OUT_DIR is named
# monolith-<version>-<tier>-<arch>.iso.
if [[ -f "${PROFILE_WORK}/profiledef.sh" ]]; then
    if [[ -n "${ISO_LABEL_OVERRIDE}" ]]; then
        sed -i "s/^iso_label=.*/iso_label=\"${ISO_LABEL_OVERRIDE}\"/" \
            "${PROFILE_WORK}/profiledef.sh"
    fi
    if [[ -n "${ISO_VERSION_OVERRIDE}" ]]; then
        sed -i "s/^iso_version=.*/iso_version=\"${ISO_VERSION_OVERRIDE}-${TIER}\"/" \
            "${PROFILE_WORK}/profiledef.sh"
    else
        # Tag the default date-stamped version with the tier.
        sed -i "s/^iso_version=\"\$(date +%Y\.%m\.%d)\"/iso_version=\"\$(date +%Y.%m.%d)-${TIER}\"/" \
            "${PROFILE_WORK}/profiledef.sh" || true
    fi
fi

# Optionally vendor the Monolith release tarball into airootfs so the
# installed system has mnctl/mnpkg/monolith-installer ready to run.
if [[ -n "${RELEASE_TAR}" ]]; then
    if [[ ! -f "${RELEASE_TAR}" ]]; then
        echo "Release tarball not found: ${RELEASE_TAR}" >&2
        exit 1
    fi
    echo "[*] Vendoring ${RELEASE_TAR} into airootfs/usr/local/bin"
    install -d "${PROFILE_WORK}/airootfs/usr/local/bin"
    tar -C "${PROFILE_WORK}/airootfs/usr/local/bin" -xzf "${RELEASE_TAR}"
fi

mkdir -p "${OUT_DIR}"

echo "[*] Running mkarchiso (this can take a while)"
mkarchiso -v -w "${WORK_DIR}/work" -o "${OUT_DIR}" "${PROFILE_WORK}"

echo
echo "[+] Done. ISO image written to: ${OUT_DIR}"
ls -lh "${OUT_DIR}"/*.iso 2>/dev/null || true
