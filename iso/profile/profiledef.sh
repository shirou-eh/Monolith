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
    ["/usr/local/bin/monolith-firstboot"]="0:0:755"
    ["/usr/local/bin/monolith-installer-launcher"]="0:0:755"
)
