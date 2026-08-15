mod logic;
mod system;
mod ui;
mod engine;

use anyhow::Result;
use crossterm::style::Stylize;
use log::{error, info};
 

use crate::logic::{
    pipeline::{
        InstallOptions, InstallPlan, InstallStage, InstallState, PipelineEngine,
        build_install_plan,
    },
};

use crate::engine::checkpoint_path;

// Checkpoint persistence moved to `engine` module.

fn init_logging() -> Result<()> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}] {}",
                record.target(),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(fern::log_file("shipwright.log")?)
        .apply()?;

    Ok(())
}

fn main() -> Result<()> {
    init_logging()?;

    if !system::is_root_user() {
        error!("Installer started without root privileges");
        println!(
            "{}",
            "This installer must be run as root. Please run with sudo or as the root user."
                .red()
                .bold()
        );
        return Ok(());
    }

    ui::clear_screen()?;
    println!("{}", "=== Shipwright Installer ===".green().bold());
    let mut completed_stages: Vec<InstallStage> = Vec::new();

    let cp_path = checkpoint_path();
    let mut state = if let Some(checkpoint) = engine::load_checkpoint(&cp_path)? {
        info!(
            "Checkpoint found at {}. Resuming installation with {} completed stages",
            &cp_path,
            checkpoint.completed_stages.len()
        );
        println!(
            "{}",
            format!(
                "Resuming installation from checkpoint ({} completed stages).",
                checkpoint.completed_stages.len()
            )
            .yellow()
            .bold()
        );
        completed_stages = checkpoint.completed_stages;
        checkpoint.state.into_install_state()?
    } else {
        build_fresh_install_state()?
    };

    info!("Building install plan");
    let plan = build_install_plan(&state)?;
    let stages_to_run: Vec<InstallStage> = plan
        .stages
        .iter()
        .copied()
        .filter(|stage| !completed_stages.contains(stage))
        .collect();

    if stages_to_run.is_empty() {
        info!("All stages already completed according to checkpoint; clearing checkpoint");
        engine::clear_checkpoint(&cp_path)?;
        println!(
            "{}",
            "Install already complete according to checkpoint."
                .green()
                .bold()
        );
        return Ok(());
    }

    info!(
        "Executing install pipeline ({} remaining of {} total stages)",
        stages_to_run.len(),
        plan.stages.len()
    );

    let base = engine::default_executor;
    let executor = engine::make_checkpointing_executor(
        &mut completed_stages,
        |completed, state| engine::save_checkpoint(&cp_path, state, completed),
        base,
    );

    PipelineEngine::new(executor).run(
        &mut state,
        &InstallPlan { stages: stages_to_run },
    )?;

    engine::clear_checkpoint(&cp_path)?;
    info!("Install pipeline completed successfully");

    Ok(())
}

fn build_fresh_install_state() -> Result<InstallState> {
    let is_efi = system::is_efi();
    let has_bluetooth = system::has_bluetooth();
    let has_battery = system::has_battery();
    let has_tpm = system::has_tpm();
    info!(
        "Hardware flags detected: is_efi={}, bluetooth={}, battery={}, tpm={}",
        is_efi, has_bluetooth, has_battery, has_tpm
    );

    let cpu_vendor = system::detect_cpu_vendor().unwrap_or_else(|| "Unknown".to_string());
    let installation_type = ui::prompt_installation_type()?;

    // Disk selection and partitioning flow
    println!("Available disks:");
    let disks = system::get_disks()?;
    if disks.is_empty() {
        error!("No disks found; aborting installation");
        println!(
            "{}",
            "No disks found. Please ensure you have at least one disk connected."
                .red()
                .bold()
        );
        return Err(anyhow::anyhow!("No disks found"));
    }

    // Loop to allow selecting disk, running cfdisk, and confirming layout

    loop {
        let selected_disk = ui::prompt_disk(&disks)?;

        println!(
            "Launching cfdisk on {}. Make your partition changes and exit when done.",
            selected_disk.device.path()
        );

        // Launch cfdisk interactively; do not pipe stdin so user can interact
        system::spawn_interactive("cfdisk", &[selected_disk.device.path().as_str()])?;

        // Re-read partitions after cfdisk
        println!("Refreshing partition table...");
        let partitions = system::get_partitions()?;
        info!("Discovered {} partition entries", partitions.len());

        if partitions.is_empty() {
            println!(
                "{}",
                "No partitions found after partitioning. Do you want to try again?"
                    .yellow()
                    .bold()
            );
            if ui::prompt_confirmation("Re-run partitioning on another disk?")? {
                continue;
            } else {
                return Err(anyhow::anyhow!("No partitions found after partitioning"));
            }
        }

        // Prompt for passwords and user config before finalizing layout
        let root_encryption_password = ui::prompt_text(
            "Please type the password for the encrypted root partition",
            true,
        )?;
        let should_format_efi = ui::prompt_confirmation("Should the EFI partition be formatted?")?;
        let root_password = ui::prompt_text("Enter the password for the root user:", true)?;
        let user = ui::prompt_user_config()?;

        // Prompt for disk layout using discovered partitions
        let chosen_layout = ui::prompt_disk_layout(&partitions, is_efi)?;

        // Confirmation loop: show partition summary and selected layout before asking
        loop {
            ui::print_partitions(&partitions);
            ui::print_selected_layout(&chosen_layout);
            if ui::prompt_confirmation("Confirm this partition layout?")? {
                // Build and return InstallState
                return Ok(InstallState {
                    cpu_vendor,
                    partitions: partitions.clone(),
                    disk_layout: chosen_layout,
                    root_encryption_password,
                    root_password,
                    user,
                    options: InstallOptions {
                        installation_type,
                        should_format_efi,
                        is_efi,
                        has_battery,
                        has_bluetooth,
                        has_tpm,
                    },
                });
            } else {
                // Ask whether to re-run cfdisk on same disk, choose another disk, or abort
                if ui::prompt_confirmation("Re-run cfdisk on this disk?")? {
                    system::spawn_interactive("cfdisk", &[selected_disk.device.path().as_str()])?;
                    // after return, refresh partitions and re-prompt layout
                    break; // break to outer loop to refresh partitions
                } else if ui::prompt_confirmation("Select a different disk to partition?")? {
                    break; // break to outer loop to choose disk again
                } else {
                    return Err(anyhow::anyhow!("User did not confirm partition layout"));
                }
            }
        }
    }
}
