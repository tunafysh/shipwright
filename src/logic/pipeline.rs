use std::fmt;

use anyhow::{Result, anyhow};
use log::info;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::logic::disk::{DiskLayout, Partition};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallationType {
    Minimal,
    KdePlasma,
}

#[derive(Clone, Debug)]
pub struct UserConfig {
    pub username: String,
    pub password: Option<Zeroizing<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallOptions {
    pub installation_type: InstallationType,
    pub should_format_efi: bool,
    pub is_efi: bool,
    pub has_battery: bool,
    pub has_bluetooth: bool,
    pub has_tpm: bool,
}

#[derive(Clone, Debug)]
pub struct InstallState {
    pub cpu_vendor: String,
    pub partitions: Vec<Partition>,
    pub disk_layout: DiskLayout,
    pub root_encryption_password: Zeroizing<String>,
    pub root_password: Zeroizing<String>,
    pub user: UserConfig,
    pub options: InstallOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallStage {
    DetectHardware,
    FormatPartitions,
    MountFilesystem,
    InstallBaseSystem,
    ConfigureSystem,
    CreateUser,
    InstallYayAndAURPackages,
    InstallFlatpakPackages,
    InstallMultilibPackages,
    InstallAdditionalPackages,
    InstallBootloader,
    InstallRecoverySystem,
    SyncDisks,
    UnmountDisks,
}

impl fmt::Display for InstallStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage_name = match self {
            InstallStage::DetectHardware => "DetectHardware",
            InstallStage::FormatPartitions => "FormatPartitions",
            InstallStage::MountFilesystem => "MountFilesystem",
            InstallStage::InstallBaseSystem => "InstallBaseSystem",
            InstallStage::ConfigureSystem => "ConfigureSystem",
            InstallStage::CreateUser => "CreateUser",
            InstallStage::InstallYayAndAURPackages => "InstallYayAndAURPackages",
            InstallStage::InstallFlatpakPackages => "InstallFlatpakPackages",
            InstallStage::InstallMultilibPackages => "InstallMultilibPackages",
            InstallStage::InstallAdditionalPackages => "InstallAdditionalPackages",
            InstallStage::InstallBootloader => "InstallBootloader",
            InstallStage::InstallRecoverySystem => "InstallRecoverySystem",
            InstallStage::SyncDisks => "SyncDisks",
            InstallStage::UnmountDisks => "UnmountDisks",
        };

        write!(f, "{}", stage_name)
    }
}

#[derive(Clone, Debug)]
pub struct InstallPlan {
    pub stages: Vec<InstallStage>,
}

pub trait StageExecutor {
    fn execute_stage(&mut self, stage: InstallStage, state: &mut InstallState) -> Result<()>;
}

impl<F> StageExecutor for F
where
    F: FnMut(InstallStage, &mut InstallState) -> Result<()>,
{
    fn execute_stage(&mut self, stage: InstallStage, state: &mut InstallState) -> Result<()> {
        self(stage, state)
    }
}

pub trait InstallExtension {
    fn before_stage(&mut self, _stage: InstallStage, _state: &InstallState) -> Result<()> {
        Ok(())
    }

    fn after_stage(&mut self, _stage: InstallStage, _state: &InstallState) -> Result<()> {
        Ok(())
    }
}

pub struct PipelineEngine<E: StageExecutor> {
    executor: E,
    extensions: Vec<Box<dyn InstallExtension>>,
}

impl<E: StageExecutor> PipelineEngine<E> {
    pub fn new(executor: E) -> Self {
        Self {
            executor,
            extensions: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn with_extension(mut self, extension: Box<dyn InstallExtension>) -> Self {
        self.extensions.push(extension);
        self
    }

    pub fn run(mut self, state: &mut InstallState, plan: &InstallPlan) -> Result<()> {
        for stage in &plan.stages {
            info!("Starting stage: {}", stage);
            for ext in &mut self.extensions {
                ext.before_stage(*stage, state)?;
            }

            self.executor.execute_stage(*stage, state)?;

            for ext in &mut self.extensions {
                ext.after_stage(*stage, state)?;
            }
            info!("Completed stage: {}", stage);
        }

        Ok(())
    }
}

pub fn validate_install_state(state: &InstallState) -> Result<()> {
    if state.disk_layout.root_partition.name().is_empty() {
        return Err(anyhow!("Root partition must be selected"));
    }

    if state.options.is_efi && state.disk_layout.efi_partition.is_none() {
        return Err(anyhow!("EFI mode requires an EFI partition"));
    }

    if state.user.username.trim().is_empty() {
        return Err(anyhow!("Username cannot be empty"));
    }

    if state.root_encryption_password.is_empty() {
        return Err(anyhow!("Root encryption password cannot be empty"));
    }

    if state.user.password.as_ref().is_some_and(|p| p.is_empty()) {
        return Err(anyhow!("User password cannot be empty"));
    }

    Ok(())
}

pub fn build_install_plan(state: &InstallState) -> Result<InstallPlan> {
    validate_install_state(state)?;
    if state.options.installation_type == InstallationType::KdePlasma {
        Ok(InstallPlan {
            stages: vec![
                InstallStage::DetectHardware,
                InstallStage::FormatPartitions,
                InstallStage::MountFilesystem,
                InstallStage::InstallBaseSystem,
                InstallStage::ConfigureSystem,
                InstallStage::CreateUser,
                InstallStage::InstallYayAndAURPackages,
                InstallStage::InstallFlatpakPackages,
                InstallStage::InstallMultilibPackages,
                InstallStage::InstallBootloader,
                InstallStage::InstallAdditionalPackages,
                InstallStage::InstallRecoverySystem,
                InstallStage::SyncDisks,
                InstallStage::UnmountDisks,
            ],
        })
    } else {
        Ok(InstallPlan {
            stages: vec![
                InstallStage::DetectHardware,
                InstallStage::FormatPartitions,
                InstallStage::MountFilesystem,
                InstallStage::InstallBaseSystem,
                InstallStage::ConfigureSystem,
                InstallStage::CreateUser,
                InstallStage::InstallBootloader,
                InstallStage::InstallRecoverySystem,
                InstallStage::SyncDisks,
                InstallStage::UnmountDisks,
            ],
        })
    }
}

#[allow(dead_code)]
pub fn run_pipeline<F>(state: &mut InstallState, plan: &InstallPlan, execute_stage: F) -> Result<()>
where
    F: FnMut(InstallStage, &mut InstallState) -> Result<()>,
{
    PipelineEngine::new(execute_stage).run(state, plan)
}
