use anyhow::Result;
use crossterm::{
    execute,
    terminal::{Clear, ClearType},
};
use dialoguer::{Confirm, Input, Password, Select, theme::ColorfulTheme};
use std::io::stdout;
use zeroize::Zeroizing;

use crate::logic::{DiskLayout, InstallationType, Partition, UserConfig};

pub fn clear_screen() -> Result<()> {
    let mut out = stdout();
    execute!(out, Clear(ClearType::All))?;
    Ok(())
}

pub fn prompt_confirmation(prompt: &str) -> Result<bool> {
    let input = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .interact()?;
    Ok(input)
}

pub fn prompt_text(prompt: &str, secret: bool) -> Result<Zeroizing<String>> {
    if secret {
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

pub fn prompt_installation_type() -> Result<InstallationType> {
    let choices = ["Minimal (no desktop environment)", "KDE Plasma"];
    let idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select the type of installation")
        .items(choices)
        .interact()?;

    let kind = match idx {
        0 => InstallationType::Minimal,
        _ => InstallationType::KdePlasma,
    };

    Ok(kind)
}

fn prompt_partition(prompt: &str, partitions: &[Partition]) -> Result<Partition> {
    let items: Vec<String> = partitions
        .iter()
        .map(|part| format!("{} - {}", part.name(), part.size))
        .collect();

    let idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&items)
        .interact()?;

    Ok(partitions[idx].clone())
}

pub fn prompt_disk_layout(partitions: &[Partition], is_efi: bool) -> Result<DiskLayout> {
    let efi_partition = if is_efi {
        Some(prompt_partition(
            "Which one of the partitions should be the EFI partition?",
            partitions,
        )?)
    } else {
        None
    };

    let root_partition = prompt_partition(
        "Which one of the partitions should be the root partition?",
        partitions,
    )?;

    let swap_partition = if prompt_confirmation("Do you want to create a swap partition?")? {
        Some(prompt_partition(
            "Which one of the partitions should be the swap partition?",
            partitions,
        )?)
    } else {
        None
    };

    let home_partition = if prompt_confirmation("Do you want to create a separate home partition?")?
    {
        Some(prompt_partition(
            "Which one of the partitions should be the home partition?",
            partitions,
        )?)
    } else {
        None
    };

    let recovery_partition = if prompt_confirmation("Do you want to create a recovery partition?")?
    {
        Some(prompt_partition(
            "Which one of the partitions should be the recovery partition?",
            partitions,
        )?)
    } else {
        None
    };

    Ok(DiskLayout {
        efi_partition,
        root_partition,
        home_partition,
        swap_partition,
        recovery_partition,
    })
}

#[allow(dead_code)]
pub fn print_selected_layout(layout: &DiskLayout) {
    println!("Selected partitions:");
    println!("Root: {}", layout.root_partition.path());
    if let Some(efi) = &layout.efi_partition {
        println!("EFI: {}", efi.path());
    }
    if let Some(home) = &layout.home_partition {
        println!("Home: {}", home.path());
    }
    if let Some(swap) = &layout.swap_partition {
        println!("Swap: {}", swap.path());
    }
    if let Some(recovery) = &layout.recovery_partition {
        println!("Recovery: {}", recovery.path());
    }
}

pub fn prompt_user_config() -> Result<UserConfig> {
    let username = prompt_text("Enter the username for the new user:", false)?.to_string();
    let password = if prompt_confirmation("Should the user have a password?")? {
        Some(prompt_text("Enter the password for the new user:", true)?)
    } else {
        None
    };

    Ok(UserConfig { username, password })
}
