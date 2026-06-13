use anyhow::{Context, Result};
mod logic;
mod system;
mod ui;

use crate::logic::{
    DiskLayout, InstallOptions, InstallStage, InstallState, Partition, PipelineEngine, UserConfig,
    build_install_plan, hash_password, verify_password,
};
use crossterm::style::Stylize;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

const CHECKPOINT_PATH: &str = "/tmp/shipwright-progress.json";

#[derive(Debug, Serialize, Deserialize)]
struct PersistedInstallState {
    cpu_vendor: String,
    partitions: Vec<Partition>,
    disk_layout: DiskLayout,
    #[serde(default)]
    root_encryption_password_hash: Option<String>,
    username: String,
    #[serde(default)]
    user_password_hash: Option<String>,
    root_password_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root_encryption_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_password: Option<String>,
    options: InstallOptions,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstallCheckpoint {
    completed_stages: Vec<InstallStage>,
    state: PersistedInstallState,
}

impl PersistedInstallState {
    fn from_install_state(state: &InstallState) -> Result<Self> {
        Ok(Self {
            cpu_vendor: state.cpu_vendor.clone(),
            partitions: state.partitions.clone(),
            disk_layout: state.disk_layout.clone(),
            root_encryption_password_hash: Some(hash_password(
                state.root_encryption_password.as_str(),
            )?),
            root_password_hash: hash_password(state.root_password.as_str())?,
            username: state.user.username.clone(),
            user_password_hash: state
                .user
                .password
                .as_ref()
                .map(|p| hash_password(p.as_str()))
                .transpose()?,
            root_encryption_password: None,
            user_password: None,
            options: state.options.clone(),
        })
    }

    fn into_install_state(self) -> Result<InstallState> {
        let root_encryption_password = ui::prompt_text(
            "Checkpoint restored. Re-enter encrypted root partition password:",
            true,
        )?;

        let user_password = if self.user_password_hash.is_some() {
            Some(ui::prompt_text("Re-enter user password:", true)?)
        } else {
            None
        };

        let root_password = ui::prompt_text("Re-enter root password:", true)?;

        // (password, hash)
        let mut checks: Vec<(Option<String>, Option<&String>)> = vec![
            (
                Some(root_password.to_string()),
                Some(&self.root_password_hash),
            ),
            (
                Some(root_encryption_password.to_string()),
                self.root_encryption_password_hash.as_ref(),
            ),
        ];

        if let Some(user_pw) = &user_password {
            checks.push((Some(user_pw.to_string()), self.user_password_hash.as_ref()));
        }

        for (password_opt, hash_opt) in checks {
            let (Some(password), Some(hash)) = (password_opt, hash_opt) else {
                continue;
            };

            if !verify_password(&password, hash)? {
                return Err(anyhow::anyhow!(
                    "Password provided does not match the stored hash. Please ensure you enter the correct password."
                ));
            }
        }

        Ok(InstallState {
            cpu_vendor: self.cpu_vendor,
            partitions: self.partitions,
            disk_layout: self.disk_layout,
            root_encryption_password,
            root_password,
            user: UserConfig {
                username: self.username,
                password: user_password,
            },
            options: self.options,
        })
    }
}

fn load_checkpoint(path: &str) -> Result<Option<InstallCheckpoint>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read checkpoint file at {}", path))?;
    let checkpoint: InstallCheckpoint = serde_json::from_str(content.as_str())
        .with_context(|| format!("Failed to parse checkpoint JSON at {}", path))?;
    Ok(Some(checkpoint))
}

fn save_checkpoint(
    path: &str,
    state: &InstallState,
    completed_stages: &[InstallStage],
) -> Result<()> {
    let checkpoint = InstallCheckpoint {
        completed_stages: completed_stages.to_vec(),
        state: PersistedInstallState::from_install_state(state)?,
    };

    let json = serde_json::to_string_pretty(&checkpoint)?;
    fs::write(path, json)
        .with_context(|| format!("Failed to write checkpoint file at {}", path))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("Failed to set permissions for checkpoint file at {}", path))?;
    Ok(())
}

fn clear_checkpoint(path: &str) -> Result<()> {
    if Path::new(path).exists() {
        fs::remove_file(path)
            .with_context(|| format!("Failed to remove checkpoint file at {}", path))?;
    }
    Ok(())
}

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

    let mut state = if let Some(checkpoint) = load_checkpoint(CHECKPOINT_PATH)? {
        info!(
            "Checkpoint found at {}. Resuming installation with {} completed stages",
            CHECKPOINT_PATH,
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
        clear_checkpoint(CHECKPOINT_PATH)?;
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

    PipelineEngine::new(|stage, state: &mut InstallState| {
        system::execute_stage(stage, state)?;

        if !completed_stages.contains(&stage) {
            completed_stages.push(stage);
        }

        save_checkpoint(CHECKPOINT_PATH, state, completed_stages.as_slice())?;
        Ok(())
    })
    .run(
        &mut state,
        &crate::logic::InstallPlan {
            stages: stages_to_run,
        },
    )?;

    clear_checkpoint(CHECKPOINT_PATH)?;
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

    println!("Fetching partitions...");
    let partitions = system::get_partitions()?;
    info!("Discovered {} partition entries", partitions.len());
    if partitions.is_empty() {
        error!("No partitions found; aborting installation");
        println!(
            "{}",
            "No partitions found. Please ensure you have at least one disk connected."
                .red()
                .bold()
        );
        return Err(anyhow::anyhow!("No partitions found"));
    }

    let root_encryption_password = ui::prompt_text(
        "Please type the password for the encrypted root partition",
        true,
    )?;
    let should_format_efi = ui::prompt_confirmation("Should the EFI partition be formatted?")?;
    let root_password = ui::prompt_text("Enter the password for the root user:", true)?;
    let user = ui::prompt_user_config()?;

    let disk_layout = ui::prompt_disk_layout(&partitions, is_efi)?;
    Ok(InstallState {
        cpu_vendor,
        partitions: partitions.clone(),
        disk_layout,
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
    })
}
