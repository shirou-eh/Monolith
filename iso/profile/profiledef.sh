#!/usr/bin/env bash
# shellcheck disable=SC2034
# Monolith OS — archiso profile definition
# Used as a starting template by `mnctl iso build`. See:
# https://wiki.archlinux.org/title/archiso

iso_name="monolith"
iso_label="MONOLITH_$(date +%Y%m)"
iso_publisher="Monolith OS <https://github.com/shirou-eh/Monolith>"
iso_application="Monolith OS Live/Install Medium"
iso_version="$(date +%Y.%m.%d)"
install_dir="monolith"
buildmodes=('iso')
bootmodes=(
    'bios.syslinux.mbr'
    'bios.syslinux.eltorito'
    'uefi-ia32.grub.esp'
    'uefi-x64.grub.esp'
    'uefi-ia32.grub.eltorito'
    'uefi-x64.grub.eltorito'
)
arch="x86_64"
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'xz' '-Xbcj' 'x86' '-b' '1M' '-Xdict-size' '1M')
file_permissions=(
    ["/etc/shadow"]="0:0:400"
    ["/root"]="0:0:750"
    ["/root/.automated_script.sh"]="0:0:755"
    # Trailing slash = mkarchiso chowns/chmods this recursively (see
    # its own source: `[[ "${filename: -1}" == "/" ]]` gates -R).
    # mkarchiso's own airootfs copy step does NOT preserve arbitrary
    # custom permissions from the source tree — only what's listed
    # here survives, confirmed by two scripts in a row (monolith-
    # installer, then monolith-selftest) landing as non-executable
    # 644 in the built image despite being 755 in git. Per-file
    # entries meant remembering to add a new line every time a script
    # is added to this directory — exactly what already got missed
    # once. One recursive entry for the whole directory closes the
    # bug class instead of the individual symptom.
    ["/usr/local/bin/"]="0:0:755"
)
