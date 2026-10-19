#!/bin/bash

RESET="\e[0m"
RED="\e[1;31m"
GREEN="\e[1;32m"
BLUE="\e[1;34m"

check() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo -e "${RED}The command $1 is not installed.${RESET}"
        return 1
    fi
}

check qemu-system-x86_64 || exit 1
check qemu-img || exit 1

ISO="${1:?Usage: $0 /path/to/archlinux.iso}"

if [[ ! -f "$ISO" ]]; then
    echo -e "${RED}ISO not found: $ISO${RESET}"
    exit 1
fi

if [[ ! -f arch.qcow2 ]]; then
    qemu-img create -f qcow2 arch.qcow2 20G || exit 1
fi

qemu-system-x86_64 \
    -enable-kvm \
    -smp 2 \
    -m 4G \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.4m.fd \
    -drive if=pflash,format=raw,file=./arch_VARS.fd \
    -drive file=arch.qcow2,format=qcow2,if=ide,index=0 \
    -cdrom "$ISO" \
    -boot order=d \
    -netdev user,id=net0,hostfwd=tcp::2222-:22 \
    -device virtio-net-pci,netdev=net0