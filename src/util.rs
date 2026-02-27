use anyhow::{Error, Result};
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
};
use dialoguer::{Confirm, Input, Password, Select, theme::ColorfulTheme};
use serde::Deserialize;
use std::{
    fs::{self, read_to_string, write},
    io::{Write, stdout},
    process::{Command, Stdio},
};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

pub fn clear() -> Result<()> {
    let mut stdout = stdout();
    execute!(stdout, Clear(ClearType::All))?;
    Ok(())
}

pub fn spawn(cmd: &str, args: Vec<&str>) -> Result<i32> {
    let result = Command::new(cmd).args(&args).status()?;
    if let Some(code) = result.code()
        && result.success()
    {
        Ok(code)
    } else if !args.is_empty() {
        Err(Error::msg(format!(
            "Command execution failed. Command: {} with args: {:?}. Exit code: {}",
            cmd,
            args,
            result.code().unwrap_or(-1)
        )))
    } else {
        Err(Error::msg(format!(
            "Command execution failed. Command: {} with no args. Exit code: {}",
            cmd,
            result.code().unwrap_or(-1)
        )))
    }
}

pub fn run(cmd: &str, args: Vec<&str>) -> Result<String> {
    let result = if !args.is_empty() {
        Command::new(cmd).stdin(Stdio::piped()).args(&args).output()?
    } else {
        Command::new(cmd).stdin(Stdio::piped()).output()?
    };

    if result.status.success() && result.status.code().unwrap_or(-1) == 0 {
        Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
    } else if !args.is_empty() {
        Err(Error::msg(format!(
            "Command execution failed. Command: {} with args: {:?}. Exit code: {}",
            cmd,
            args,
            result.status.code().unwrap_or(-1)
        )))
    } else {
        Err(Error::msg(format!(
            "Command execution failed. Command: {} with no args. Exit code: {}",
            cmd,
            result.status.code().unwrap_or(-1)
        )))
    }
}

#[derive(Debug, Deserialize)]
pub struct Lsblk {
    pub blockdevices: Vec<Device>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Device {
    pub name: String,
    pub size: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub children: Option<Vec<Device>>, // Include children for partitions
}

/// Get partitions instead of top-level disks
pub fn get_partitions() -> Result<Vec<Device>> {
    // Don't use -d, so we get both disks and partitions
    let command_output = run("lsblk", vec!["-J", "-o", "NAME,SIZE,TYPE,MODEL,MOUNTPOINT"])?;
    let parsed_output: Lsblk = serde_json::from_str(&command_output)?;

    // Collect all children (partitions) recursively
    fn collect_partitions(devices: Vec<Device>, partitions: &mut Vec<Device>) {
        for dev in devices {
            if dev.device_type == "part" {
                partitions.push(dev.clone());
            }
            if let Some(children) = dev.children {
                collect_partitions(children, partitions);
            }
        }
    }

    let mut partitions = Vec::new();
    collect_partitions(parsed_output.blockdevices, &mut partitions);

    Ok(partitions)
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

pub fn confirmation(prompt: &str) -> Result<bool> {
    let input = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .interact()?;
    Ok(input)
}

pub fn question(prompt: &str, pass: bool) -> Result<Zeroizing<String>> {
    if pass {
        let input = Password::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .interact()?;
        Ok(Zeroizing::new(input))
    } else {
        let input = Input::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .interact()?;
        Ok(Zeroizing::new(input))
    }
}

pub fn selection(prompt: &str, choices: Vec<&str>) -> Result<String> {
    let input = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&choices)
        .interact()?;
    Ok(choices[input].to_string())
}

pub fn select_partition(prompt: &str, partitions: &[Device]) -> Result<String> {
    let items: Vec<String> = partitions
        .iter()
        .map(|dev| format!("{} - {}", dev.name, dev.size))
        .collect();

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&items)
        .interact()?;

    Ok(partitions[selection].name.clone())
}
pub struct DiskLayout {
    pub efi_partition: Option<String>,
    pub root_partition: String,
    pub home_partition: Option<String>,
    pub swap_partition: Option<String>,
}

pub fn luks_format(dev: &str, password: Zeroizing<String>) -> Result<()> {
    // 1️⃣ Create a secure temporary file
    let mut tmpfile = NamedTempFile::new()?;

    // 2️⃣ Write password to it
    tmpfile.write_all(password.as_bytes())?;
    tmpfile.flush()?; // ensure it's written

    let path = tmpfile.path().to_str().unwrap();

    // 3️⃣ Call cryptsetup with --key-file
    let status = std::process::Command::new("cryptsetup")
        .args(["luksFormat", format!("/dev/{}", dev).as_str(), "-q", "--key-file", path])
        .status()?;

    if !status.success() {
        return Err(anyhow::anyhow!("Failed to format device {}", dev));
    }

    run(
        "cryptsetup",
        [
            "open",
            "-q",
            "--key-file",
            path,
            format!("/dev/{}", dev).as_str(),
            "encrypted_root",
        ]
        .to_vec(),
    )?;

    // 4️⃣ tmpfile automatically deleted when dropped
    Ok(())
}

pub fn add_home_volume(fstab_path: &str, home_partition: &str) -> Result<()> {
    let content = read_to_string(fstab_path)?;
    let mut new_lines = vec![];

    let part_uuid = run(
        "blkid",
        vec![
            format!("/dev/{}", home_partition).as_str(),
            "-s",
            "UUID",
            "-o",
            "value",
        ],
    )?;

    for line in content.lines() {
        if line.contains("/home") {
            // replace with Btrfs subvolume mount
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

pub fn add_nvme_module(mkinitcpio_conf: &str) -> Result<()> {
    let content = read_to_string(mkinitcpio_conf)?;
    let mut new_lines = Vec::new();

    for line in content.lines() {
        if line.starts_with("MODULES=(") {
            // Only append nvme if not already present
            if !line.contains("nvme") {
                let line = line.replace(")", " nvme)");
                new_lines.push(line);
            } else {
                new_lines.push(line.to_string());
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    write(mkinitcpio_conf, new_lines.join("\n") + "\n")?;
    Ok(())
}

pub fn enroll_tpm(root_partition: &str) -> Result<()> {
    // Ensure the root LUKS device is open first
    spawn(
        "systemd-cryptenroll",
        vec!["--tpm2-device=auto", root_partition],
    )?;
    Ok(())
}

pub fn create_user() -> Result<()> {
    // Ask for username
    let username = question("Enter the username for the new user:", false)?.to_string();

    // Create the user and home directory
    run(
        "arch-chroot",
        vec!["/mnt", "useradd", "-m", "-G", "wheel", &username],
    )?;

    // Set the user's password
    let password_process = Command::new("arch-chroot")
        .args(["/mnt", "passwd", &username])
        .stdin(std::process::Stdio::piped())
        .status()?;

    if !password_process.success() {
        return Err(anyhow::anyhow!(
            "Failed to set password for user {}",
            username
        ));
    }

    println!("User {} created successfully", username);
    Ok(())
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
