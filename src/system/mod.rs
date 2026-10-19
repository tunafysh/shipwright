use anyhow::{Error, Result};
use libcryptsetup_rs::{
    CryptInit, CryptParamsLuks2Ref, Either,
    consts::{flags::CryptActivate, vals::EncryptionFormat},
};
use log::info;
use serde::Deserialize;
use std::{
    env,
    fs::{self, read_to_string, write},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use zeroize::Zeroizing;

mod consts;
use consts::*;

mod util;
use util::{
    arch_chroot_pacman_sync_install, ensure_line_value, run, run_arch_chroot, spawn,
    spawn_arch_chroot,
};
// Re-export interactive spawn helper implemented in util.rs so callers can use
// `system::spawn_interactive(...)` while the implementation lives in util.
pub use util::spawn_interactive;

use crate::logic::{
    disk::{Disk, Partition},
    download,
    pipeline::{InstallStage, InstallState, InstallationType, UserConfig},
};

#[derive(Debug, Deserialize)]
struct Lsblk {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Clone, Debug, Deserialize)]
struct LsblkDevice {
    name: String,
    size: String,
    #[serde(rename = "type")]
    device_type: String,
    children: Option<Vec<LsblkDevice>>,
    uuid: Option<String>,
}

pub fn is_root_user() -> bool {
    std::env::var("USER").unwrap_or_default() == "root"
}

pub fn is_efi() -> bool {
    Path::new("/sys/firmware/efi").exists()
}

pub fn has_bluetooth() -> bool {
    Path::new("/sys/class/bluetooth").exists()
}

pub fn is_cachyos() -> bool {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        return content.contains("CachyOS");
    }
    false
}

pub fn has_battery() -> bool {
    if let Ok(entries) = fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("bat") {
                return true;
            }
        }
    }
    false
}

pub fn has_tpm() -> bool {
    Path::new("/dev/tpm0").exists()
}

pub fn detect_cpu_vendor() -> Option<String> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").ok()?;

    for line in cpuinfo.lines() {
        if line.starts_with("vendor_id") {
            return line.split(':').nth(1).map(|s| s.trim().to_string());
        }
    }

    None
}

pub fn get_partitions() -> Result<Vec<Partition>> {
    let output = run(
        "lsblk",
        &["-J", "-o", "NAME,SIZE,TYPE,MODEL,MOUNTPOINT,UUID"],
    )?;
    let parsed: Lsblk = serde_json::from_str(&output)?;

    fn collect(devices: Vec<LsblkDevice>, partitions: &mut Vec<Partition>) {
        for dev in devices {
            if dev.device_type == "part" {
                partitions.push(Partition::from_lsblk(
                    dev.name.clone(),
                    dev.size.clone(),
                    dev.uuid.unwrap_or_default(),
                ));
            }
            if let Some(children) = dev.children {
                collect(children, partitions);
            }
        }
    }

    let mut partitions = Vec::new();
    collect(parsed.blockdevices, &mut partitions);
    Ok(partitions)
}

pub fn get_disks() -> Result<Vec<Disk>> {
    let output = run("lsblk", &["-J", "-o", "NAME,SIZE,TYPE,MODEL,UUID"])?;
    let parsed: Lsblk = serde_json::from_str(&output)?;

    fn collect(devices: Vec<LsblkDevice>, disks: &mut Vec<Disk>) {
        for dev in devices {
            if dev.device_type == "disk" {
                disks.push(Disk {
                    device: crate::logic::disk::Device::from_name(dev.name.clone()),
                    size: dev.size.clone(),
                });
            }
            if let Some(children) = dev.children {
                collect(children, disks);
            }
        }
    }

    let mut disks: Vec<Disk> = Vec::new();
    collect(parsed.blockdevices, &mut disks);
    Ok(disks)
}

// spawn_interactive is implemented in util.rs and re-exported above.

pub fn prepare_keyring_and_base_tools() -> Result<()> {
    spawn("pacman-key", &["--init"], false)?;
    spawn("pacman-key", &["--populate", "archlinux"], false)?;
    run(
        "pacman",
        &[
            "-Sy",
            "--noconfirm",
            "archlinux-keyring",
            "reflector",
            "btrfs-progs",
            "dosfstools",
        ],
    )?;
    Ok(())
}

pub fn sync_time() -> Result<()> {
    spawn("timedatectl", &["set-ntp", "true"], false)?;
    Ok(())
}

pub fn luks_format(root: &mut Partition, password: &str) -> Result<()> {
    let root_path = PathBuf::from(root.path());
    let root_uuid = uuid::Uuid::new_v4();

    info!("Initializing LUKS encryption on partition {}", root.path());
    let mut crypt = CryptInit::init(&root_path)?;

    info!("Formatting partition {} with LUKS2 encryption", root.path());
    let mut context = crypt.context_handle();

    // 1. FORMAT (creates LUKS container + master key)
    context.format(
        EncryptionFormat::Luks2,
        ("aes", "xts-plain64"),
        Some(root_uuid),
        Either::Right(32), // 256-bit key (NOT 256/8 confusion risk)
        None::<&mut CryptParamsLuks2Ref>,
    )?;

    info!("Adding keyslot to encrypted partition {}", root.path());
    let mut keyslot = crypt.keyslot_handle();

    // 2. ADD FIRST KEY (password)
    keyslot.add_by_passphrase(
        None, // auto keyslot
        b"",  // no existing password (fresh disk)
        password.as_bytes(),
    )?;

    info!(
        "Activating encrypted partition {} with provided password",
        root.path()
    );
    let mut activate = crypt.activate_handle();
    // 3. ACTIVATE (map the encrypted partition to /dev/mapper/encrypted_root)
    activate.activate_by_passphrase(
        Some("encrypted_root"),
        None,
        password.as_bytes(),
        CryptActivate::empty(),
    )?;

    root.set_uuid(root_uuid);

    Ok(())
}

pub fn install_aditional_packages() -> Result<()> {
    let mut args = vec!["pacman", "-Sy", "--noconfirm"];
    args.extend_from_slice(&ADDITIONAL_PACKAGES);
    spawn_arch_chroot(&args)?;
    Ok(())
}

pub fn add_home_volume(fstab_path: &str, home_partition: &Partition) -> Result<()> {
    let content = read_to_string(fstab_path)?;
    let mut new_lines = vec![];

    let home_path = home_partition.path();
    let part_uuid = run("blkid", &[home_path.as_str(), "-s", "UUID", "-o", "value"])?;

    for line in content.lines() {
        if line.contains("/home") {
            new_lines.push(format!(
                "UUID={}  /home  btrfs  defaults 0  2",
                part_uuid.trim()
            ));
        } else {
            new_lines.push(line.to_string());
        }
    }

    write(fstab_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

pub fn configure_dracut(state: &InstallState) -> Result<()> {
    let dracut_conf_path = "/mnt/etc/dracut.conf";
    let mut modules = vec!["crypt", "systemd"];
    if state.options.has_tpm {
        modules.push("tpm2-tss");
    }
    let config = format!(
        r#"
# Dracut configuration for Shipwright installer
add_dracutmodules+=" {} "
hostonly="yes"
    "#,
        modules.join(" ")
    );

    write(dracut_conf_path, config)?;
    Ok(())
}

fn run_with_password(args: &[&str], password: &str) -> Result<()> {
    let key_path = PathBuf::from("/dev/shm/luks.key");
    let mut key_file = fs::File::create(&key_path)?;
    key_file.write_all(password.as_bytes())?;

    let mut child = Command::new("systemd-cryptenroll")
        .args(["--unlock-key-file", key_path.display().to_string().as_str()]) // holy conversion
        .args(args)
        .spawn()?;

    let status = child.wait()?;

    if !status.success() {
        anyhow::bail!("command failed: {:?}", args);
    }

    spawn(
        "shred",
        &["-u", key_path.to_str().unwrap_or("/dev/shm/luks.key")],
        false,
    )?;

    Ok(())
}

pub fn enroll_tpm(root: &str, passwd: Zeroizing<String>) -> Result<()> {
    let passwd_ref = passwd.as_str();

    // 1. TPM enrollment
    run_with_password(&["--tpm2-device=auto", root], passwd_ref)?;

    Ok(())
}

fn configure_pacman_conf(path: &str, use_cachyos_repo: bool) -> Result<()> {
    let mut content = read_to_string(path)?;

    if content.contains("#Color") {
        content = content.replacen("#Color", "Color", 1);
    }

    if !content.lines().any(|line| line.trim() == "ILoveCandy") {
        let mut lines = Vec::new();
        let mut inserted = false;

        for line in content.lines() {
            lines.push(line.to_string());
            if !inserted && line.trim() == "Color" {
                lines.push("ILoveCandy".to_string());
                inserted = true;
            }
        }

        if !inserted {
            lines.push("Color".to_string());
            lines.push("ILoveCandy".to_string());
        }

        content = lines.join("\n");
    }

    if use_cachyos_repo && !content.contains("[cachyos]") {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("\n[cachyos]\nServer = https://cachyos.org/repo/$arch\n");
    }

    if !content.ends_with('\n') {
        content.push('\n');
    }

    write(path, content)?;
    Ok(())
}

fn install_base_packages(state: &InstallState) -> Result<()> {
    let mut pkgs: Vec<&str> = CORE_PACKAGES.to_vec();
    if state.cpu_vendor.contains("AMD") {
        pkgs.extend(AMD_MICROCODE_PACKAGES.iter().copied());
    } else if state.cpu_vendor.contains("Intel") {
        pkgs.extend(INTEL_MICROCODE_PACKAGES.iter().copied());
    }

    if is_cachyos() {
        pkgs.push(CACHYOS_LINUX);
    } else {
        pkgs.push(STANDARD_LINUX);
    }

    let mut args = vec!["/mnt"];
    args.extend(pkgs);
    spawn("pacstrap", &args, false)?;
    Ok(())
}

fn configure_crypttab(root: &Partition, has_tpm: bool) -> Result<()> {
    let entry = if has_tpm {
        format!(
            "encrypted_root UUID={} none luks,tpm2-device=auto\n",
            root.uuid.trim()
        )
    } else {
        format!("encrypted_root UUID={} none luks\n", root.uuid.trim())
    };
    write("/mnt/etc/crypttab", entry)?;
    Ok(())
}

fn configure_system(state: &InstallState) -> Result<()> {
    info!("Generating fstab and writing to /mnt/etc/fstab");
    let output = run("genfstab", &["-U", "/mnt"])?;
    fs::write("/mnt/etc/fstab", output)?;

    info!("Generating crypttab");
    configure_crypttab(&state.disk_layout.root_partition, state.options.has_tpm)?;

    info!("Configuring dracut");
    configure_dracut(state)?;

    if let Some(home) = &state.disk_layout.home_partition {
        info!("Adding home volume to /mnt/etc/fstab");
        add_home_volume("/mnt/etc/fstab", home)?;
    }

    if state.options.has_tpm {
        info!(
            "Installing tools and dracut modules for tpm and enrolling the root partition to tpm"
        );
        arch_chroot_pacman_sync_install(&["tpm2-tools", "tpm2-tss"])?;
        enroll_tpm(
            &state.disk_layout.root_partition.path(),
            state.root_encryption_password.clone(),
        )?;
    }

    ensure_line_value(
        "/mnt/etc/sudoers",
        "# %wheel ALL=(ALL) NOPASSWD: ALL",
        "%wheel ALL=(ALL) NOPASSWD: ALL",
    )?;

    configure_pacman_conf("/mnt/etc/pacman.conf", is_cachyos())?;

    let _ = fs::remove_file("/mnt/etc/locale.conf");
    fs::write("/mnt/etc/locale.conf", "LANG=en_US.UTF-8")?;
    fs::write("/mnt/etc/vconsole.conf", "KEYMAP=us")?;
    fs::write("/mnt/etc/hostname", "Mothership")?;
    spawn_arch_chroot(&["dracut", "-f", "/boot/initramfs-linux.img"])?;
    // set this to the password of the harddisk
    info!("Setting root password via chpasswd");
    let mut child = Command::new("arch-chroot")
        .args(["/mnt", "chpasswd"])
        .stdin(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        // Write "username:password" directly to the chroot's chpasswd stdin
        writeln!(stdin, "root:{}", state.root_password.as_str())?;
    }

    let status = child.wait()?;
    if !status.success() {
        anyhow::bail!("Failed to set root password");
    }

    if state.options.installation_type == InstallationType::KdePlasma {
        arch_chroot_pacman_sync_install(&KDE_PACKAGES)?;
        spawn_arch_chroot(&["systemctl", "enable", "sddm"])?;
        spawn_arch_chroot(&[
            "weston",
            "--backend=headless",
            "--socket=wayland-shipwright",
            "--idle-time=0",
            "--",
            "lookandfeeltool",
            "-a",
            "org.kde.breezedark.desktop",
        ])?;
    }

    if state.options.has_battery {
        spawn_arch_chroot(&["systemctl", "enable", "tlp"])?;
        arch_chroot_pacman_sync_install(&["power-profiles-daemon"])?;
        spawn_arch_chroot(&["systemctl", "enable", "power-profiles-daemon"])?;
    }

    if state.options.has_bluetooth {
        arch_chroot_pacman_sync_install(&["bluez", "bluez-utils"])?;
        spawn_arch_chroot(&["systemctl", "enable", "bluetooth"])?;
    }

    spawn_arch_chroot(&["systemctl", "enable", "NetworkManager"])?;

    Ok(())
}

fn install_bootloader(state: &InstallState) -> Result<()> {
    ensure_line_value(
        "/mnt/etc/default/grub",
        "#GRUB_DISABLE_OS_PROBER=false",
        "GRUB_DISABLE_OS_PROBER=false",
    )?;

    ensure_line_value(
        "/mnt/etc/default/grub",
        "GRUB_CMDLINE_LINUX=\"\"",
        format!(
            "GRUB_CMDLINE_LINUX=\"rd.luks.name={}=encrypted_root root=/dev/mapper/encrypted_root\"",
            state.disk_layout.root_partition.uuid.trim()
        )
        .as_str(),
    )?;

    ensure_line_value(
        "/mnt/etc/default/grub",
        "#GRUB_TERMINAL_OUTPUT=\"console\"",
        "GRUB_TERMINAL_OUTPUT=\"gfxterm\"",
    )?;
    ensure_line_value(
        "/mnt/etc/default/grub",
        "#GRUB_THEME=/path/to/theme.txt",
        "GRUB_THEME=/boot/grub/themes/minegrub-world-selection/theme.txt",
    )?;

    if state.options.is_efi {
        spawn_arch_chroot(&[
            "grub-install",
            "--target=x86_64-efi",
            "--efi-directory=/boot",
            "--bootloader-id=GRUB",
        ])?;
        spawn_arch_chroot(&["grub-mkconfig", "-o", "/boot/grub/grub.cfg"])?;
    } else {
        let root_disk_path = state.disk_layout.root_partition.path();
        spawn_arch_chroot(&["grub-install", "--target=i386-pc", root_disk_path.as_str()])?;
        spawn_arch_chroot(&["grub-mkconfig", "-o", "/boot/grub/grub.cfg"])?;
    }

    Ok(())
}

fn create_user(state: &UserConfig) -> Result<()> {
    run_arch_chroot(&["useradd", "-m", "-G", "wheel", state.username.as_str()])?;

    if let Some(password) = state.password.as_ref() {
        let mut child = Command::new("arch-chroot")
            .args(["/mnt", "chpasswd"])
            .stdin(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{}:{}", state.username.as_str(), password.as_str())?;
        }

        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!("Failed to set user password");
        }
    }

    println!("User {} created successfully", state.username);
    Ok(())
}

fn delete_user(username: &str) -> Result<()> {
    run_arch_chroot(&["userdel", "-r", username])?;
    Ok(())
}

fn format_partitions(state: &mut InstallState) -> Result<()> {
    let _ = spawn("umount", &["-R", "/mnt"], false);

    if state.options.is_efi
        && state.options.should_format_efi
        && let Some(efi) = &state.disk_layout.efi_partition
    {
        let result = spawn("mkfs.fat", &["-F32", "-I", efi.path().as_str()], false)?;
        if result != 0 {
            return Err(Error::msg(format!(
                "Failed to format EFI partition. Command: mkfs.fat -F32 {}",
                efi.path()
            )));
        }
    }

    luks_format(
        &mut state.disk_layout.root_partition,
        state.root_encryption_password.as_str(),
    )?;

    spawn("mkfs.btrfs", &["/dev/mapper/encrypted_root"], false)?;

    if let Some(home) = &state.disk_layout.home_partition {
        spawn("mkfs.btrfs", &[home.path().as_str()], false)?;
    }

    if let Some(swap) = &state.disk_layout.swap_partition {
        let swap_path = swap.path();
        spawn("mkswap", &[swap_path.as_str()], false)?;
        spawn("swapon", &[swap_path.as_str()], false)?;
    }

    if let Some(recovery) = &state.disk_layout.recovery_partition {
        spawn("mkfs.ext4", &[recovery.path().as_str()], false)?;
    }

    Ok(())
}

fn create_btrfs_subvolumes(state: &InstallState) -> Result<()> {
    spawn("mount", &["/dev/mapper/encrypted_root", "/mnt"], false)?;

    let mount_options = "defaults,compress=zstd,discard=async,ssd,noatime";

    let mut subvolumes = Vec::from(["@", "@snapshots"]);

    if state.disk_layout.home_partition.is_none() {
        subvolumes.push("@home");
    }

    for subvol in &subvolumes {
        spawn(
            "btrfs",
            &["subvolume", "create", &format!("/mnt/{}", subvol)],
            false,
        )?;
    }

    spawn("umount", &["/mnt"], false)?;

    // root
    spawn(
        "mount",
        &[
            "-o",
            &format!("{},subvol=@", mount_options),
            "/dev/mapper/encrypted_root",
            "/mnt",
        ],
        false,
    )?;

    // ensure dirs exist
    spawn("mkdir", &["-p", "/mnt/.snapshots"], false)?;
    if state.disk_layout.home_partition.is_none() {
        spawn("mkdir", &["-p", "/mnt/home"], false)?;

        // home
        spawn(
            "mount",
            &[
                "-o",
                &format!("{},subvol=@home", mount_options),
                "/dev/mapper/encrypted_root",
                "/mnt/home",
            ],
            false,
        )?;
    }

    // snapshots
    spawn(
        "mount",
        &[
            "-o",
            &format!("{},subvol=@snapshots", mount_options),
            "/dev/mapper/encrypted_root",
            "/mnt/.snapshots",
        ],
        false,
    )?;

    Ok(())
}

fn mount_filesystems(state: &InstallState) -> Result<()> {
    create_btrfs_subvolumes(state)?;

    fs::create_dir_all("/mnt/boot")?;

    if state.options.is_efi
        && let Some(efi) = &state.disk_layout.efi_partition
    {
        spawn("mount", &[efi.path().as_str(), "/mnt/boot"], false)?;
    }

    if let Some(home) = &state.disk_layout.home_partition {
        fs::create_dir_all("/mnt/home")?;
        spawn("mount", &[home.path().as_str(), "/mnt/home"], false)?;
    }

    Ok(())
}

fn install_yay() -> Result<()> {
    create_user(&UserConfig {
        username: "aurbuilder".to_string(),
        password: None,
    })?;

    arch_chroot_pacman_sync_install(&["base-devel", "git", "go"])?;
    spawn_arch_chroot(&[
        "sudo",
        "-u",
        "aurbuilder",
        "bash",
        "-c",
        "git clone https://aur.archlinux.org/yay.git /tmp/yay && cd /tmp/yay && makepkg -si --noconfirm && rm -rf /tmp/yay",
    ])?;
    Ok(())
}

fn install_aur_packages(state: &InstallState) -> Result<()> {
    let packages: &[&str] = if state.options.installation_type == InstallationType::KdePlasma {
        &[BOOTLOADER_THEME, CODE_EDITOR]
    } else {
        &[BOOTLOADER_THEME]
    };
    let mut args = vec![
        "sudo",
        "-u",
        state.user.username.as_str(),
        "yay",
        "-Sy",
        "--noconfirm",
        "--answerdiff",
        "None",
        "--answerclean",
        "None",
        "--answeredit",
        "None",
    ];
    args.extend_from_slice(packages);
    spawn_arch_chroot(&args)?;

    delete_user("aurbuilder")?;
    Ok(())
}

fn install_recovery_system(state: &InstallState) -> Result<()> {
    // Installing alpine as a recovery partition (around 300 mb)
    if state.disk_layout.recovery_partition.is_none() {
        return Ok(());
    }

    if let Some(recovery) = &state.disk_layout.recovery_partition {
        fs::create_dir_all("/mnt/recovery")?;
        spawn("mount", &[recovery.path().as_str(), "/mnt/recovery"], false)?;
    }

    let recovery = state.disk_layout.recovery_partition.as_ref().unwrap();
    let image_url = format!(
        "https://dl-cdn.alpinelinux.org/alpine/v3.23/releases/{}/alpine-minirootfs-3.23.4-{}.tar.gz",
        env::consts::ARCH,
        env::consts::ARCH
    );
    let image_path = "/tmp/alpine.tar.gz";

    download(&image_url, image_path)?;

    // Extract the ISO contents to the recovery partition
    spawn("tar", &["-xzf", image_path, "-C", "/mnt/recovery"], false)?;

    // Chroot into the recovery partition and set up the necessary filesystem structure
    spawn(
        "mount",
        &["-t", "proc", "proc", "/mnt/recovery/proc"],
        false,
    )?;

    spawn("mount", &["--rbind", "/dev", "/mnt/recovery/dev"], false)?;
    spawn("mount", &["--make-rslave", "/mnt/recovery/dev"], false)?;

    spawn("mount", &["--rbind", "/sys", "/mnt/recovery/sys"], false)?;
    spawn("mount", &["--make-rslave", "/mnt/recovery/sys"], false)?;

    // isolated runtime instead of leaking host /run
    spawn(
        "mount",
        &["-t", "tmpfs", "tmpfs", "/mnt/recovery/run"],
        false,
    )?;

    let alpine_recovery_script = include_str!("../alpine_install_script.txt")
        .to_string()
        .replace("{{packages}}", RECOVERY_PACKAGES.join(" ").as_str());

    spawn(
        "chroot",
        &[
            "/mnt/recovery",
            "/bin/sh",
            "-c",
            alpine_recovery_script.as_str(),
        ],
        false,
    )?;

    // Adding bootloader entry for recovery system

    let recovery_entry = UNFORMATTED_RECOVERY_ENTRY.replace("{{ROOT_UUID}}", &recovery.uuid);
    let recovery_entry_path = "/mnt/etc/grub.d/40_custom";
    let mut entry_data = fs::read_to_string(recovery_entry_path).unwrap_or_default();
    if entry_data.contains(&recovery_entry) {
        info!("Recovery entry already exists in GRUB configuration. Skipping addition.");
        return Ok(());
    }

    entry_data.push_str(format!("\n{}", recovery_entry).as_str());

    fs::write(recovery_entry_path, recovery_entry)?;
    // Update GRUB configuration to include the new entry
    spawn_arch_chroot(&["grub-mkconfig", "-o", "/boot/grub/grub.cfg"])?;

    Ok(())
}

fn enable_multilib_repo() -> Result<()> {
    let pacman_conf_path = "/mnt/etc/pacman.conf";
    ensure_line_value(
        pacman_conf_path,
        "Include = /etc/pacman.d/mirrorlist",
        "Include = /etc/pacman.d/mirrorlist\n[multilib]\nInclude = /etc/pacman.d/mirrorlist",
    )
}

fn install_multilib_packages() -> Result<()> {
    enable_multilib_repo()?;
    arch_chroot_pacman_sync_install(&MULTILIB_PACKAGES)?;
    Ok(())
}

pub fn execute_stage(stage: InstallStage, state: &mut InstallState) -> Result<()> {
    match stage {
        InstallStage::DetectHardware => {
            prepare_keyring_and_base_tools()?;
            sync_time()?;
        }
        InstallStage::FormatPartitions => format_partitions(state)?,
        InstallStage::MountFilesystem => mount_filesystems(state)?,
        InstallStage::InstallBaseSystem => install_base_packages(state)?,
        InstallStage::ConfigureSystem => configure_system(state)?,
        InstallStage::CreateUser => create_user(&state.user)?,
        InstallStage::InstallYayAndAURPackages => {
            install_yay()?;
            install_aur_packages(state)?;
        }
        InstallStage::InstallFlatpakPackages => {
            arch_chroot_pacman_sync_install(&["flatpak"])?;
            ()
        }
        InstallStage::InstallMultilibPackages => install_multilib_packages()?,
        InstallStage::InstallAdditionalPackages => {
            install_aditional_packages()?;
        }
        InstallStage::InstallBootloader => install_bootloader(state)?,
        InstallStage::InstallRecoverySystem => install_recovery_system(state)?,
        InstallStage::SyncDisks => {
            let _ = spawn("sync", &[], false)?;
        }
        InstallStage::UnmountDisks => {
            let _ = spawn("umount", &["-R", "/mnt"], false)?;
        }
    }

    Ok(())
}
