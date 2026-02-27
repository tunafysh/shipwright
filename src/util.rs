use std::{fs, io::stdout, process::Command};
use anyhow::{Error, Result};
use crossterm::{execute, terminal::{Clear, ClearType}};
use serde::{Serialize, Deserialize};

pub fn clear() -> Result<()> {
    let mut stdout = stdout();
    execute!(stdout, Clear(ClearType::All))?;
    Ok(())
}

pub fn spawn(cmd: &str, args: Vec<&str>) -> Result<i32> {
    let result = if !args.is_empty() {
        Command::new(cmd).args(&args).status()?
    } else {
        Command::new(cmd).status()?
    };

    if let Some(code) = result.code() && result.success() {
        Ok(code)
    } else {
        if !args.is_empty() {
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
}

pub fn run(cmd: &str, args: Vec<&str>) -> Result<String> {
    let result = if !args.is_empty() {
        Command::new(cmd).args(&args).output()?
    } else {
        Command::new(cmd).output()?
    };

    if result.status.success() && result.status.code().unwrap_or(-1) == 0 {
        Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
    } else {
        if !args.is_empty() {
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
}

#[derive(Debug, Deserialize)]
pub struct Lsblk {
    pub blockdevices: Vec<Device>,
}

#[derive(Debug, Deserialize)]
pub struct Device {
    pub name: String,
    pub size: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub model: Option<String>,
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

pub fn is_efi() -> bool {
    std::path::Path::new("/sys/firmware/efi").exists()
}

pub fn is_root() -> bool {
    std::env::var("USER").unwrap_or_default() == "root"
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