pub const STANDARD_LINUX: &str = "linux";

pub const CACHYOS_LINUX: &str = "linux-cachyos";

pub const CORE_PACKAGES: [&str; 50] = [
    "base",
    "linux-firmware",
    "efibootmgr",
    "grub",
    "os-prober",
    "networkmanager",
    "base-devel",
    "git",
    "sudo",
    "vim",
    "nano",
    "mc",
    "links",
    "btrfs-progs",
    "dosfstools",
    "cryptsetup",
    "linux-firmware",
    "linux-headers",
    "btop",
    "htop",
    "fastfetch",
    "parted",
    "gptfdisk",
    "lvm2",
    "reflector",
    "zsh",
    "zoxide",
    "bind-tools",
    "rsync",
    "speedtest-cli",
    "sshfs",
    "nmap",
    "dsniff",
    "zip",
    "unzip",
    "powertop",
    "libarchive",
    "arp-scan",
    "wavemon",
    "kbd",
    "dracut",
    "iwd",
    "smartmontools",
    "p7zip",
    "unrar",
    "xdg-user-dirs",
    "pipewire-audio",
    "pipewire-alsa",
    "pipewire-pulse",
    "wireplumber",
];

// Replaced the generic "plasma" and bloated "kde-applications" groups
pub const KDE_PACKAGES: [&str; 26] = [
    // Core Desktop Shell
    "plasma-desktop",
    "sddm",
    "kwallet-pam", // Keeps your wallet unlocked completely silently
    // Core Base Apps (Essential Utilities)
    "konsole",
    "dolphin",
    "ark",
    "spectacle",
    "plasma-systemmonitor", // Clean GUI resource/task manager
    // Hardware & Tray Integration (The "Middle Ground")
    "systemsettings",
    "kinfocenter", // Adds "About this System" profile pane
    "plasma-nm",
    "powerdevil",
    "plasma-pa",
    "bluedevil",
    "kde-cli-tools",
    "kde-gtk-config",         // Essential to sync system theme to GTK apps
    "xdg-desktop-portal-kde", // Vital wayland file chooser / screenshare integration
    // Display Server Protocols
    "wayland-protocols",
    "wayland",
    "xorg-server-xwayland",
    "qt5-wayland",
    "qt6-wayland",
    "noto-fonts",
    "noto-fonts-emoji",
    "ttf-hack",
    "weston",
];

pub const CODE_EDITOR: &str = "visual-studio-code-insiders-bin";

// i can't install flatpak packages in the chroot for some reason, so i'm skipping it for now. will add it back later when i have more time to debug the issue

// pub const FLATPAK_PACKAGES: [&str; 2] = ["org.vinegarhq.Sober", "org.prismlauncher.PrismLauncher"];

pub const MULTILIB_PACKAGES: [&str; 1] = ["steam"];

pub const RECOVERY_PACKAGES: [&str; 15] = [
    "btrfs-progs",
    "dosfstools",
    "parted",
    "efibootmgr",
    "util-linux",
    "e2fsprogs",
    "openrc",
    "openssh",
    "networkmanager",
    "networkmanager-openrc",
    "curl",
    "tar",
    "gzip",
    "linux-lts",
    "bash",
];

pub const ADDITIONAL_PACKAGES: [&str; 13] = [
    "lutris",
    "prismlauncher",
    "supertuxkart",
    "retroarch",
    "supertux",
    "timeshift",
    "kitty",
    "neovim",
    "starship",
    "gparted",
    "gimp",
    "krita",
    "vlc",
];

pub const AMD_MICROCODE_PACKAGES: [&str; 1] = ["amd-ucode"];
pub const INTEL_MICROCODE_PACKAGES: [&str; 1] = ["intel-ucode"];

pub const UNFORMATTED_RECOVERY_ENTRY: &str = include_str!("../recovery_entry.txt");

pub const BOOTLOADER_THEME: &str = "grub-theme-minegrub-world-selection-git";
