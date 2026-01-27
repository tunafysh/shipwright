#!/usr/bin/env python3
import subprocess
import os
import platform
import yaml
import sys

# ---------- ANSI Colors ----------
GREEN = "\033[92m"
BLUE = "\033[94m"
CYAN = "\033[96m"
YELLOW = "\033[93m"
RED = "\033[91m"
RESET = "\033[0m"

# ---------- Helpers ----------
def run(cmd):
    print(f"{CYAN}[RUN] {cmd}{RESET}")
    subprocess.run(cmd, shell=True, check=True)

def sh(cmd):
    return subprocess.check_output(cmd, shell=True, text=True).strip()

# ---------- Hardware Detection ----------
def detect_cpu():
    info = sh("lscpu")
    vendor = "unknown"
    if "GenuineIntel" in info:
        vendor = "intel"
    elif "AuthenticAMD" in info:
        vendor = "amd"
    model = sh("grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2").strip()
    return {"vendor": vendor, "model": model}

def detect_microcode():
    v = detect_cpu()["vendor"]
    return "intel-ucode" if v == "intel" else "amd-ucode" if v == "amd" else None

def detect_gpu():
    pci = sh("lspci").lower()
    if "nvidia" in pci:
        return {"vendor": "nvidia", "packages": ["nvidia", "nvidia-utils"]}
    if "amd" in pci:
        return {"vendor": "amd", "packages": ["mesa", "xf86-video-amdgpu"]}
    if "intel" in pci:
        return {"vendor": "intel", "packages": ["mesa", "xf86-video-intel"]}
    return {"vendor": "unknown", "packages": []}

def has_bluetooth():
    return "bluetooth" in (sh("lsusb") + sh("lspci")).lower()

def has_wifi():
    return "wireless" in sh("lspci").lower() or "wlan" in sh("ip link")

def is_laptop():
    return os.path.exists("/sys/class/power_supply/BAT0")

def detect_hardware():
    return {
        "cpu": detect_cpu(),
        "microcode": detect_microcode(),
        "gpu": detect_gpu(),
        "bluetooth": has_bluetooth(),
        "wifi": has_wifi(),
        "laptop": is_laptop(),
        "arch": platform.machine(),
        "uefi": os.path.exists("/sys/firmware/efi"),
    }

# ---------- Hardware Manifest ----------
def write_hardware_manifest():
    hw = detect_hardware()
    os.makedirs("/mnt/root", exist_ok=True)
    with open("/mnt/root/hardware.yaml", "w") as f:
        yaml.dump(hw, f, sort_keys=False)
    print(f"{GREEN}Hardware manifest written to /mnt/root/hardware.yaml{RESET}")

# ---------- Packages ----------
BASE_PACKAGES = [
    "base", "linux", "linux-firmware", "sudo",
    "networkmanager", "btrfs-progs",
]

CORE_UTILS = [
    "git", "vim", "wget", "zsh", "bash-completion",
    "htop", "btop", "nmon", "mc",
    "usbutils", "lsof", "dmidecode",
    "fastfetch",
    "rsync", "ddrescue",
    "zip", "unzip", "unrar", "p7zip", "lzop",
    "fwupd", "xdg-user-dirs",
]

NETWORK_TOOLS = [
    "openssh", "nmap", "speedtest-cli", "arp-scan",
    "wavemon", "bind-tools", "traceroute",
    "dsniff", "mitmproxy", "links",
]

DEV_PACKAGES = ["base-devel"]

KDE_LEAN = [
    "plasma",
    "sddm",
    "kde-system-meta",
    "kde-utilities-meta",
    "networkmanager-qt",
    "plasma-nm",
    "powerdevil",
    "kscreen",
    "kpipewire",
    "pipewire",
    "pipewire-alsa",
    "pipewire-pulse",
    "wireplumber",
    "polkit-kde-agent",
    "xdg-desktop-portal",
    "xdg-desktop-portal-kde",
    "ttf-dejavu",
    "ttf-liberation",
    "noto-fonts",
]

GNOME_LEAN = [
    "gnome-shell",
    "gnome-control-center",
    "gnome-terminal",
    "nautilus",
    "gdm",
    "pipewire",
    "pipewire-alsa",
    "pipewire-pulse",
    "wireplumber",
    "xdg-desktop-portal",
    "xdg-desktop-portal-gnome",
]

def hardware_packages(hw):
    pkgs = []
    if hw["microcode"]:
        pkgs.append(hw["microcode"])
    pkgs += hw["gpu"]["packages"]
    if hw["bluetooth"]:
        pkgs += ["bluez", "bluez-utils"]
    if hw["wifi"]:
        pkgs += ["iw", "wpa_supplicant"]
    if hw["laptop"]:
        pkgs += ["tlp", "powertop", "acpid"]
    return pkgs

def build_package_list(desktop_choice):
    hw = detect_hardware()
    pkgs = (
        BASE_PACKAGES
        + CORE_UTILS
        + NETWORK_TOOLS
        + DEV_PACKAGES
        + hardware_packages(hw)
    )

    if desktop_choice == "1":
        pkgs += KDE_LEAN
    elif desktop_choice == "2":
        pkgs += GNOME_LEAN

    return list(dict.fromkeys(pkgs))

# ---------- Disk ----------
def list_partitions():
    run("lsblk -o NAME,SIZE,FSTYPE,TYPE,MOUNTPOINT")

def select_partitions():
    efi = input(f"{GREEN}EFI partition (e.g. /dev/nvme0n1p1): {RESET}")
    format_efi = input(f"{YELLOW}Format EFI partition? (y/N): {RESET}").lower() == "y"
    root = input(f"{GREEN}Root partition to format: {RESET}")
    if input(f"{RED}Format {root}? (y/N): {RESET}").lower() != "y":
        sys.exit(1)
    return efi, format_efi, root

def format_root(root):
    run(f"cryptsetup luksFormat --type luks2 {root}")
    run(f"systemd-cryptenroll --tpm2-device=auto {root}")
    run(f"cryptsetup luksOpen {root} luksroot")
    run("mkfs.btrfs -f /dev/mapper/luksroot")

def mount_partitions(efi, format_efi):
    run("mount /dev/mapper/luksroot /mnt")
    run("mkdir -p /mnt/boot/efi")
    if format_efi:
        run(f"mkfs.fat -F32 {efi}")
    run(f"mount {efi} /mnt/boot/efi")

def create_crypttab(root):
    os.makedirs("/mnt/etc", exist_ok=True)
    uuid = sh(f"blkid -s UUID -o value {root}")
    with open("/mnt/etc/crypttab", "w") as f:
        f.write(f"luks-root UUID={uuid} none luks,tpm2-device=auto\n")

# ---------- Install ----------
def update_keyring():
    print(f"{BLUE}Updating Arch keyring...{RESET}")
    run("pacman -Sy --noconfirm archlinux-keyring")

def install_base_system(desktop_choice):
    run("pacstrap /mnt " + " ".join(build_package_list(desktop_choice)))
    write_hardware_manifest()

def configure_system(hostname="mothership", desktop_choice="1"):
    run(f"arch-chroot /mnt bash -c 'echo {hostname} > /etc/hostname'")
    run("arch-chroot /mnt ln -sf /usr/share/zoneinfo/Europe/Tirane /etc/localtime")
    run("arch-chroot /mnt hwclock --systohc")
    run("arch-chroot /mnt bash -c 'echo en_US.UTF-8 UTF-8 > /etc/locale.gen'")
    run("arch-chroot /mnt locale-gen")
    run("arch-chroot /mnt bash -c 'echo LANG=en_US.UTF-8 > /etc/locale.conf'")

    hw = detect_hardware()
    services = ["NetworkManager", "sshd"]

    if desktop_choice == "1":
        services.append("sddm")
    elif desktop_choice == "2":
        services.append("gdm")

    if hw["bluetooth"]:
        services.append("bluetooth")
    if hw["laptop"]:
        services.append("tlp")

    for s in services:
        run(f"arch-chroot /mnt systemctl enable {s}")

def install_bootloader():
    run("arch-chroot /mnt pacman -S --noconfirm grub efibootmgr")
    run("arch-chroot /mnt grub-install --target=x86_64-efi --efi-directory=/boot/efi --bootloader-id=GRUB")
    run("arch-chroot /mnt grub-mkconfig -o /boot/grub/grub.cfg")

# ---------- Step System ----------
class Step:
    def __init__(self, name, fn):
        self.name = name
        self.fn = fn
        self.done = False

def render_status(steps):
    os.system("clear")
    print(f"{GREEN}=== Shipwright Installer ==={RESET}\n")
    for s in steps:
        print(f"[{'x' if s.done else ' '}] {s.name}")
    print()

def run_steps(efi, format_efi, root, desktop_choice):
    steps = [
        Step("Update keyring", update_keyring),
        Step("Format root partition", lambda: format_root(root)),
        Step("Mount partitions", lambda: mount_partitions(efi, format_efi)),
        Step("Install base system", lambda: install_base_system(desktop_choice)),
        Step("Create crypttab", lambda: create_crypttab(root)),
        Step("Configure system", lambda: configure_system(desktop_choice=desktop_choice)),
        Step("Install bootloader", install_bootloader),
    ]

    for step in steps:
        render_status(steps)
        print(f"{BLUE}>>> {step.name}{RESET}\n")
        step.fn()
        step.done = True

    render_status(steps)
    print(f"{GREEN}Installation complete. Reboot when ready.{RESET}")

# ---------- Desktop selection ----------
def select_desktop():
    print(f"""
{GREEN}Select desktop environment:{RESET}
  1) KDE Plasma (lean, recommended)
  2) GNOME (lean)
  3) Minimal / no desktop
""")
    choice = input("Choice [1-3]: ").strip()
    if choice not in ["1","2","3"]:
        print(f"{RED}Invalid choice, defaulting to Minimal.{RESET}")
        choice = "3"
    return choice

# ---------- Main ----------
def main():
    print(f"{GREEN}=== Shipwright ==={RESET}")
    list_partitions()
    efi, format_efi, root = select_partitions()
    desktop_choice = select_desktop()
    run_steps(efi, format_efi, root, desktop_choice)

if __name__ == "__main__":
    main()
