use anyhow::{Error, Result};
mod util;
use crate::util::{
    DiskLayout, add_home_volume, add_nvme_module, confirmation, create_user, detect_cpu_vendor,
    enroll_tpm, has_battery, luks_format, question, run, select_partition, selection,
};
use crossterm::style::Stylize;
use std::{fs, path::Path, process::Command};
use util::{clear, get_partitions, spawn};

static CORE_PACKAGES: [&str; 40] = [
    "base",
    "linux",
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
    "wavemon"
];

static KDE_PACKAGES: [&str; 10] = [
    "plasma",
    "sddm",
    "konsole",
    "dolphin",
    "kde-applications",
    "wayland-protocols",
    "wayland",
    "xorg-server-xwayland",
    "qt5-wayland",
    "qt6-wayland",
];

static AMD_MICROCODE_PACKAGES: [&str; 1] = ["amd-ucode"];
static INTEL_MICROCODE_PACKAGES: [&str; 1] = ["intel-ucode"];

fn main() -> Result<()> {
    // Startup
    if std::env::var("USER").unwrap_or_default() != "root" {
        println!(
            "{}",
            "This installer must be run as root. Please run with sudo or as the root user."
                .red()
                .bold()
        );
        return Ok(());
    }

    let is_efi = std::path::Path::new("/sys/firmware/efi").exists();
    let has_bluetooth = std::path::Path::new("/sys/class/bluetooth").exists();

    clear()?;
    println!("{}", "=== Shipwright Installer ===".green().bold());
    let cpu_vendor = detect_cpu_vendor().unwrap_or_else(|| "Unknown".to_string());

    println!("{}", "Setting up keyring and installing essential packages...".bold());
    spawn(
        "pacman-key",
        [
            "--init",
        ]
        .to_vec(),
    )?;
    spawn(
        "pacman-key",
        [
            "--populate",
            "archlinux",
        ]
        .to_vec(),
    )?;
    run(
        "pacman",
        [
            "-Sy",
            "--noconfirm",
            "archlinux-keyring",
            "reflector",
            "btrfs-progs",
            "dosfstools",
        ]
        .to_vec()
    )?;

    let installation_type = selection(
        "Select the type of installation",
        vec!["Minimal (no desktop environment)", "KDE Plasma"],
    )?;

    // Time synchronization

    println!("Syncing time...");
    spawn("timedatectl", vec!["set-ntp", "true"])?;

    // Partitioning (user intervention required).

    println!("Fetching partitions...");
    let partitions = get_partitions()?;
    if partitions.is_empty() {
        println!(
            "{}",
            "No partitions found. Please ensure you have at least one disk connected."
                .red()
                .bold()
        );
        return Ok(());
    }

    let mut layout = DiskLayout {
        efi_partition: None,
        root_partition: "".to_string(),
        home_partition: None,
        swap_partition: None,
    };

    if is_efi {
        let selected = select_partition(
            "Which one of the partitions should be the EFI partition?",
            &partitions,
        )?;
        layout.efi_partition = Some(selected);
    }

    let selected = select_partition(
        "Which one of the partitions should be the root partition?",
        &partitions,
    )?;
    layout.root_partition = selected;

    let should_swap = confirmation("Do you want to create a swap partition?")?;
    if should_swap {
        let selected = select_partition(
            "Which one of the partitions should be the swap partition?",
            &partitions,
        )?;
        layout.swap_partition = Some(selected);
    }

    let should_home = confirmation("Do you want to create a separate home partition?")?;
    if should_home {
        let selected = select_partition(
            "Which one of the partitions should be the home partition?",
            &partitions,
        )?;
        layout.home_partition = Some(selected)
    }

    println!("Selected partitions:");
    println!("Root: /dev/{}", layout.root_partition);
    if let Some(efi) = &layout.efi_partition {
        println!("EFI: /dev/{}", efi);
    }
    if let Some(home) = &layout.home_partition {
        println!("Home: /dev/{}", home);
    }
    if let Some(swap) = &layout.swap_partition {
        println!("Swap: /dev/{}", swap);
    }

    let password = question(
        "Please type the password for the encrypted root partition",
        true,
    )?;

    // Formatting and LUKS encryption setup.

    spawn("umount", ["-R", "/mnt"].to_vec()).ok();
    let should_format_efi = confirmation("Should the EFI partition be formatted?")?;
    if is_efi
        && should_format_efi
        && let Some(efi_partition) = &layout.efi_partition
    {
        println!("{}", "Formatting EFI".bold());
        let result = spawn(
            "mkfs.fat",
            ["-F32", "-I", format!("/dev/{}", efi_partition).as_str()].to_vec(),
        )?;
        if result != 0 {
            Err(Error::msg(format!("Failed to format EFI partition. Command: mkfs.fat -F32 /dev/{}", efi_partition)))?;
        }
    }

    println!("{}", "Formatting root".bold());
    luks_format(&layout.root_partition, password)?;
    spawn("mkfs.btrfs", ["/dev/mapper/encrypted_root"].to_vec())?;
    spawn("mount", ["/dev/mapper/encrypted_root", "/mnt"].to_vec())?;
    fs::create_dir_all("/mnt/boot")?;
    if is_efi && let Some(efi_partition) = &layout.efi_partition {
        fs::create_dir_all("/mnt/boot/efi")?;
        spawn(
            "mount",
            [format!("/dev/{}", efi_partition).as_str(), "/mnt/boot/efi"].to_vec(),
        )?;
    }
    if let Some(home_partition) = &layout.home_partition {
        spawn(
            "mkfs.btrfs",
            [format!("/dev/{}", home_partition).as_str()].to_vec(),
        )?;
        spawn("mount", [format!("/dev/{}", home_partition).as_str(), "/mnt/home"].to_vec())?;
    }

    if let Some(swap_partition) = &layout.swap_partition {
        spawn(
        "mkswap",
        [format!("/dev/{}", swap_partition).as_str()].to_vec(),
        )?;
        spawn(
        "swapon",
        [format!("/dev/{}", swap_partition).as_str()].to_vec(),
        )?;
    }

    // Base system installation.

    let mut pkgs: Vec<&str> = CORE_PACKAGES.to_vec();
    if cpu_vendor.contains("AMD") {
        pkgs.extend(AMD_MICROCODE_PACKAGES.iter().copied());
    } else if cpu_vendor.contains("Intel") {
        pkgs.extend(INTEL_MICROCODE_PACKAGES.iter().copied());
    }

    let mut args = vec!["/mnt"];
    args.extend(pkgs);
    spawn("pacstrap", args)?;

    // Fstab generation and configuration.

    run("genfstab", ["-U", "/mnt", ">>", "/mnt/etc/fstab"].to_vec())?;
    if let Some(home_partition) = &layout.home_partition {
        add_home_volume(
            "/mnt/etc/fstab",
            home_partition,
        )?;
    }

    if layout.root_partition.contains("nvme") {
        add_nvme_module("/mnt/etc/mkinitcpio.conf")?;
    }

    if Path::new("/dev/tpm0").exists() {
        spawn(
            "pacman",
            [
                "-Sy",
                "--noconfirm",
                "tpm2-tools"
            ].to_vec(),
        )?;
        enroll_tpm("/dev/mapper/encrypted_root")?;
    }
    

    // User creation.
    create_user()?;
    run(
        "sed",
        [
            "-i",
            "s/^# %wheel ALL=(ALL) NOPASSWD: ALL/%wheel ALL=(ALL) NOPASSWD: ALL/",
            "/mnt/etc/sudoers",
        ]
        .to_vec(),
    )?;

    spawn(
        "sed",
        ["-i", "s/#Color/Color\nILoveCandy/", "/mnt/etc/pacman.conf"].to_vec(),
    )?;
    fs::remove_file("/mnt/etc/locale.conf")?;
    fs::write("/mnt/etc/locale.conf", "LANG=en_US.UTF-8")?;
    fs::write("/mnt/etc/vconsole.conf", "KEYMAP=us\nFONT=ter-v16n")?;
    fs::write("/mnt/etc/hostname", "Mothership")?;
    spawn("mkinitcpio", ["-P"].to_vec())?;
    Command::new("arch-chroot")
        .args(["/mnt", "passwd"])
        .status()?;

    // Installing yay and desktop environment.

    spawn(
        "arch-chroot",
        vec![
            "/mnt",
            "bash",
            "-c",
            "git clone https://aur.archlinux.org/yay.git /tmp/yay && cd /tmp/yay && makepkg -si --noconfirm && rm -rf /tmp/yay",
        ],
    )?;

    if installation_type == "KDE Plasma" {
        spawn(
            "arch-chroot",
            [
                "/mnt",
                "pacman",
                "-Sy",
                "--noconfirm",
                KDE_PACKAGES.join(" ").as_str(),
            ]
            .to_vec(),
        )?;
        spawn(
            "arch-chroot",
            ["/mnt", "systemctl", "enable", "sddm"].to_vec(),
        )?;
    }

    if has_battery() {
        spawn(
            "arch-chroot",
            ["/mnt", "systemctl", "enable", "tlp"].to_vec(),
        )?;
        spawn(
            "arch-chroot",
            [
                "/mnt",
                "pacman",
                "-Sy",
                "--noconfirm",
                "power-profiles-daemon",
            ]
            .to_vec(),
        )?;
        spawn(
            "arch-chroot",
            ["/mnt", "systemctl", "enable", "power-profiles-daemon"].to_vec(),
        )?;
    }

    if has_bluetooth {
        spawn(
            "arch-chroot",
            [
                "/mnt",
                "pacman",
                "-Sy",
                "--noconfirm",
                "bluez",
                "bluez-utils",
            ]
            .to_vec(),
        )?;
        spawn(
            "arch-chroot",
            ["/mnt", "systemctl", "enable", "bluetooth"].to_vec(),
        )?;
    }

    spawn(
        "sed",
        [
            "-i",
            "s/#DISABLE_OS_PROBER=false/DISABLE_OS_PROBER=false/",
            "/mnt/etc/default/grub",
        ]
        .to_vec(),
    )?;
    spawn(
        "sed",
        [
            "-i",
            "s/GRUB_ENABLE_CRYPTODISK=y/GRUB_ENABLE_CRYPTODISK=y/",
            "/mnt/etc/default/grub",
        ]
        .to_vec(),
    )?;

    if is_efi {
        spawn(
            "arch-chroot",
            [
                "/mnt",
                "grub-install",
                "--target=x86_64-efi",
                "--efi-directory=/boot/efi",
                "--bootloader-id=GRUB",
            ]
            .to_vec(),
        )?;
        spawn(
            "arch-chroot",
            ["/mnt", "grub-mkconfig", "-o", "/boot/grub/grub.cfg"].to_vec(),
        )?;
    } else {
        spawn(
            "arch-chroot",
            [
                "/mnt",
                "grub-install",
                "--target=i386-pc",
                format!("/dev/{}", layout.root_partition).as_str(),
            ]
            .to_vec(),
        )?;
        spawn(
            "arch-chroot",
            ["/mnt", "grub-mkconfig", "-o", "/boot/grub/grub.cfg"].to_vec(),
        )?;
    }

    Ok(())
}
