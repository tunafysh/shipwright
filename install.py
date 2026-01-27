import subprocess
from colorama import init, Fore, Style

# Initialize colorama
init(autoreset=True)

# ---------- Helper functions ----------
def run(cmd):
    print(Fore.CYAN + f"[RUN] {cmd}")
    subprocess.run(cmd, shell=True, check=True)

def list_partitions():
    print(Fore.MAGENTA + "\nExisting partitions:")
    subprocess.run("lsblk -o NAME,SIZE,FSTYPE,TYPE,MOUNTPOINT -dn", shell=True)

def select_partitions():
    print(Fore.YELLOW + "\nSelect partitions carefully!")
    efi = input(Fore.GREEN + "EFI partition to use (do NOT format, e.g., /dev/nvme0n1p1): ")
    root = input(Fore.GREEN + "Root partition to format (e.g., /dev/nvme0n1p2): ")
    confirm = input(Fore.RED + f"\nWARNING: Only {root} will be formatted! EFI ({efi}) will NOT be formatted. Continue? (y/N): ")
    if confirm.lower() != "y":
        print(Fore.RED + "Aborting installation.")
        exit(0)
    return efi, root

# ---------- Formatting / Encryption ----------
def format_root(root_partition):
    print(Fore.BLUE + f"\nEncrypting and formatting root partition: {root_partition}")
    run(f"cryptsetup luksFormat --type luks2 {root_partition}")
    run(f"systemd-cryptenroll --tpm2-device=auto {root_partition}")
    run(f"cryptsetup luksOpen {root_partition} luksroot")
    run(f"mkfs.btrfs -f /dev/mapper/luksroot")

def mount_partitions(efi_partition):
    print(Fore.BLUE + "\nMounting partitions...")
    run("mount /dev/mapper/luksroot /mnt")
    run("mkdir -p /mnt/boot/efi")
    run(f"mount {efi_partition} /mnt/boot/efi")  # do NOT format EFI

# ---------- System configuration ----------
def get_root_uuid():
    result = subprocess.run(
        "blkid -s UUID -o value /dev/mapper/luksroot",
        shell=True,
        capture_output=True,
        text=True,
        check=True
    )
    return result.stdout.strip()

def create_crypttab(uuid):
    print(Fore.BLUE + "\nCreating /etc/crypttab...")
    with open("/mnt/etc/crypttab", "w") as f:
        f.write(f"luks-root UUID={uuid} - luks tpm2-device=auto\n")
    print(Fore.GREEN + "crypttab created successfully!")

def install_base_system():
    print(Fore.BLUE + "\nInstalling base system + KDE + extras...")
    run("pacstrap /mnt base linux linux-firmware btrfs-progs sudo networkmanager plasma kde-applications neofetch nmap htop git vim")

def configure_system(hostname="archbox"):
    print(Fore.BLUE + "\nConfiguring system...")
    # Hostname
    run(f"arch-chroot /mnt /bin/bash -c 'echo {hostname} > /etc/hostname'")
    # Timezone
    run("arch-chroot /mnt /bin/bash -c 'ln -sf /usr/share/zoneinfo/Europe/Tirane /etc/localtime'")
    run("arch-chroot /mnt /bin/bash -c 'hwclock --systohc'")
    # Locale
    run("arch-chroot /mnt /bin/bash -c 'echo \"en_US.UTF-8 UTF-8\" > /etc/locale.gen'")
    run("arch-chroot /mnt /bin/bash -c 'locale-gen'")
    run("arch-chroot /mnt /bin/bash -c 'echo \"LANG=en_US.UTF-8\" > /etc/locale.conf'")
    # Enable network
    run("arch-chroot /mnt /bin/bash -c 'systemctl enable NetworkManager'")

def install_bootloader(efi_partition="/boot/efi"):
    print(Fore.BLUE + "\nInstalling GRUB bootloader...")
    run("arch-chroot /mnt /bin/bash -c 'pacman -S --noconfirm grub efibootmgr'")
    # Use the existing EFI partition without formatting
    run(f"arch-chroot /mnt /bin/bash -c 'grub-install --target=x86_64-efi --efi-directory={efi_partition} --bootloader-id=GRUB'")
    run("arch-chroot /mnt /bin/bash -c 'grub-mkconfig -o /boot/grub/grub.cfg'")

# ---------- Main installer ----------
def main():
    print(Fore.GREEN + "\n=== Arch Triple-Boot Safe Installer ===")
    list_partitions()
    efi, root = select_partitions()
    format_root(root)
    mount_partitions(efi)
    uuid = get_root_uuid()
    create_crypttab(uuid)
    install_base_system()
    configure_system()
    install_bootloader(efi)
    print(Fore.GREEN + "\n✅ Installation complete! Ready to reboot into your new Arch KDE system.")

if __name__ == "__main__":
    main()
