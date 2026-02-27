use anyhow::Result;
mod util;
use crossterm::style::Stylize;
use util::{clear, spawn, get_disks};

use crate::util::{detect_cpu_vendor, is_efi, is_root};

static CORE_PACKAGES: [&str; 12] = ["base", "linux", "linux-firmware", "efibootmgr", "grub", "os-prober", "networkmanager", "base-devel", "git", "sudo", "vim", "nano"];
static KDE_PACKAGES: [&str; 5] = ["plasma", "sddm", "konsole", "dolphin", "kde-applications"];

fn main() -> Result<()> {
    // Startup
    if !is_root() {
        println!("{}", "This installer must be run as root. Please run with sudo or as the root user.".red().bold());
        return Ok(());
    }

    clear()?;
    println!("{}", "=== Shipwright Installer ===".green().bold());
    let cpu_vendor = detect_cpu_vendor().unwrap_or_else(|| "Unknown".to_string());

    // Time synchronization

    println!("Syncing time...");
    spawn("timedatectl", vec!["set-ntp", "true"])?;

    // Partitioning and formatting (user intervention required).

    println!("Fetching disks...");
    let disks = get_disks()?;
    if disks.blockdevices.is_empty() {
        println!("{}", "No disks found. Please ensure you have at least one disk connected.".red().bold());
        return Ok(());
    }
    if is_efi(){

    }



    Ok(())  
}
