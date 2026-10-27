use anyhow::{Error, Result, anyhow};
use regex::Regex;
use std::{
    fs::{read_to_string, write},
    path::Path,
    process::{Command, Stdio},
};

use crate::logic::disk::Partition;

pub fn ensure_line_value(path: &str, old_line: &str, new_line: &str) -> Result<()> {
    if !Path::new(path).exists() {
        if path.starts_with("/mnt/") {
            return Err(anyhow!(
                "Config file not found at {}. Refusing to fall back to host path for installer safety.",
                path
            ));
        }

        return Err(anyhow!("Config file not found at {}", path));
    }

    let content = read_to_string(path)?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let old_line_trimmed = old_line.trim();
    let new_line_trimmed = new_line.trim();

    if lines.iter().any(|line| line.trim() == new_line_trimmed) {
        return Ok(());
    }

    let mut replaced = false;
    for line in &mut lines {
        let trimmed = line.trim();
        let commented_new = format!("#{}", new_line_trimmed);
        let commented_new_with_space = format!("# {}", new_line_trimmed);

        if trimmed == old_line_trimmed
            || trimmed == commented_new
            || trimmed == commented_new_with_space
        {
            *line = new_line.to_string();
            replaced = true;
            break;
        }
    }

    if !replaced {
        lines.push(new_line.to_string());
    }

    let mut updated = lines.join("\n");
    updated.push('\n');

    write(path, updated)?;
    Ok(())
}

pub fn ensure_line_value_regex(path: &str, old_line: &str, new_line: &str) -> Result<()> {
    if !Path::new(path).exists() {
        if path.starts_with("/mnt/") {
            return Err(anyhow!(
                "Config file not found at {}. Refusing to fall back to host path for installer safety.",
                path
            ));
        }

        return Err(anyhow!("Config file not found at {}", path));
    }

    let content = read_to_string(path)?;
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let new_line_trimmed = new_line.trim();

    if lines.iter().any(|line| line.trim() == new_line_trimmed) {
        return Ok(());
    }

    let mut replaced = false;
    let old_pattern = Regex::new(old_line)?;
    for line in &mut lines {
        let trimmed = line.trim();
        let commented_new = format!("#{}", new_line_trimmed);
        let commented_new_with_space = format!("# {}", new_line_trimmed);

        if old_pattern.is_match(trimmed)
            || trimmed == commented_new
            || trimmed == commented_new_with_space
        {
            *line = new_line.to_string();
            replaced = true;
            break;
        }
    }

    if !replaced {
        lines.push(new_line.to_string());
    }

    let mut updated = lines.join("\n");
    updated.push('\n');

    write(path, updated)?;
    Ok(())
}

pub fn spawn(cmd: &str, args: &[&str], pipe_stdin: bool) -> Result<i32> {
    let mut command = Command::new(cmd);
    command.args(args);
    if pipe_stdin {
        command.stdin(Stdio::piped());
    }
    let result = command.status()?;
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

pub fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(cmd)
        .stdin(Stdio::piped())
        .args(args)
        .output()?;

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

pub fn run_arch_chroot(args: &[&str]) -> Result<String> {
    let mut chroot_args = vec!["/mnt"];
    chroot_args.extend_from_slice(args);
    run("arch-chroot", &chroot_args)
}

pub fn spawn_arch_chroot(args: &[&str]) -> Result<i32> {
    let mut chroot_args = vec!["/mnt"];
    chroot_args.extend_from_slice(args);
    spawn("arch-chroot", &chroot_args, false)
}

/// Convenience wrapper for launching interactive commands (no stdin piping).
/// Implemented here so other modules can re-export or call it directly.
pub fn spawn_interactive(cmd: &str, args: &[&str]) -> Result<i32> {
    spawn(cmd, args, false)
}

pub fn arch_chroot_pacman_sync_install(packages: &[&str]) -> Result<i32> {
    let mut args = vec!["pacman", "-Sy", "--noconfirm"];
    args.extend_from_slice(packages);
    spawn_arch_chroot(&args)
}

pub fn refresh_uuid(partition: &mut Partition) -> Result<()> {
    let output = std::process::Command::new("blkid")
        .args(["-s", "UUID", "-o", "value"])
        .arg(partition.path())
        .output()?;

    if !output.status.success() {
        return Err(Error::msg(format!(
            "Failed to retrieve filesystem UUID for {}",
            partition.path()
        )));
    }

    let uuid = String::from_utf8(output.stdout)?.trim().to_string();

    if uuid.is_empty() {
        return Err(Error::msg(format!(
            "No filesystem UUID found for {}",
            partition.path()
        )));
    }

    partition.set_uuid(uuid);
    Ok(())
}

pub fn derive_part_number_from_path(path: String) -> Result<String> {
    let res = Regex::new(r#"^(/dev/.*?)(?:p)?(\d+)$"#)?
        .captures(&path)
        .and_then(|caps| caps.get(2))
        .ok_or(anyhow!("Failed to derive partition number from "))?
        .as_str()
        .to_string();
    Ok(res)
}