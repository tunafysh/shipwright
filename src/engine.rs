use crate::logic::disk::Disk;
use crate::logic::pipeline::{InstallStage, InstallState};
use crate::system;
use anyhow::Result;
/// Default path for saving pipeline checkpoints.
pub const CHECKPOINT_PATH: &str = "/tmp/shipwright-progress.json";

/// Determine checkpoint path from environment or default.
pub fn checkpoint_path() -> String {
    std::env::var("SHIPWRIGHT_CHECKPOINT_PATH").unwrap_or_else(|_| CHECKPOINT_PATH.to_string())
}
use crate::logic::{
    disk::{DiskLayout, Partition},
    pipeline::{InstallOptions, UserConfig},
    security::{hash_password, verify_password},
};
use crate::ui;
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistedInstallState {
    pub cpu_vendor: String,
    pub partitions: Vec<Partition>,
    pub disk: Disk,
    pub disk_layout: DiskLayout,
    #[serde(default)]
    pub root_encryption_password_hash: Option<String>,
    pub username: String,
    #[serde(default)]
    pub user_password_hash: Option<String>,
    pub root_password_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_encryption_password: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_password: Option<String>,
    pub options: InstallOptions,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallCheckpoint {
    pub completed_stages: Vec<InstallStage>,
    pub state: PersistedInstallState,
}

impl PersistedInstallState {
    pub fn from_install_state(state: &InstallState) -> Result<Self> {
        Ok(Self {
            cpu_vendor: state.cpu_vendor.clone(),
            partitions: state.partitions.clone(),
            disk: state.disk.clone(),
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

    pub fn into_install_state(self) -> Result<InstallState> {
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
            disk: self.disk,
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

pub fn load_checkpoint(path: &str) -> Result<Option<InstallCheckpoint>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let checkpoint: InstallCheckpoint = serde_json::from_str(content.as_str())?;
    Ok(Some(checkpoint))
}

pub fn save_checkpoint(
    path: &str,
    state: &InstallState,
    completed_stages: &[InstallStage],
) -> Result<()> {
    let checkpoint = InstallCheckpoint {
        completed_stages: completed_stages.to_vec(),
        state: PersistedInstallState::from_install_state(state)?,
    };

    let json = serde_json::to_string_pretty(&checkpoint)?;
    fs::write(path, json)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub fn clear_checkpoint(path: &str) -> Result<()> {
    if Path::new(path).exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logic::disk::{Device, Disk, DiskLayout, Partition};
    use crate::logic::pipeline::{
        InstallOptions, InstallStage, InstallState, InstallationType, UserConfig,
    };
    use zeroize::Zeroizing;

    fn make_dummy_state() -> InstallState {
        let root_part = Partition::from_lsblk("sda1", "100G", "uuid-root");
        let disk = Disk {
            device: Device::from_name("sda"),
            size: "10G".to_string(),
        };
        let layout = DiskLayout {
            efi_partition: None,
            root_partition: root_part.clone(),
            home_partition: None,
            recovery_partition: None,
            swap_partition: None,
        };

        InstallState {
            cpu_vendor: "TestCPU".to_string(),
            partitions: vec![root_part],
            disk_layout: layout,
            disk,
            root_encryption_password: Zeroizing::new("encpw".to_string()),
            root_password: Zeroizing::new("rootpw".to_string()),
            user: UserConfig {
                username: "user".to_string(),
                password: Some(Zeroizing::new("userpw".to_string())),
            },
            options: InstallOptions {
                installation_type: InstallationType::Minimal,
                should_format_efi: false,
                is_efi: false,
                has_battery: false,
                has_bluetooth: false,
                has_tpm: false,
            },
        }
    }

    #[test]
    fn persistence_roundtrip_save_and_load() -> Result<()> {
        let mut completed: Vec<InstallStage> = Vec::new();
        let tmp = tempfile::NamedTempFile::new()?;
        let path = tmp.path().to_str().unwrap().to_string();

        // base executor that does nothing
        let mut base = |_stage: InstallStage, _state: &mut InstallState| -> Result<()> { Ok(()) };

        let mut exec = make_checkpointing_executor(&mut completed, |_, _| Ok(()), &mut base);

        let mut state = make_dummy_state();
        exec(InstallStage::DetectHardware, &mut state)?;

        // drop executor before persisting to release mutable borrow
        drop(exec);

        // persist using engine save
        save_checkpoint(&path, &state, &completed)?;

        // load back and verify
        let loaded = load_checkpoint(&path)?.expect("checkpoint should exist");
        assert_eq!(loaded.completed_stages.len(), 1);
        assert_eq!(loaded.state.username, state.user.username);
        Ok(())
    }

    #[test]
    fn checkpointing_executor_updates_completed_and_calls_save() -> Result<()> {
        let mut completed: Vec<InstallStage> = Vec::new();
        let saved_counts = std::rc::Rc::new(std::cell::RefCell::new(Vec::<usize>::new()));

        // base executor that marks called stages (shared via Rc<RefCell<..>>)
        let base_called = std::rc::Rc::new(std::cell::RefCell::new(Vec::<InstallStage>::new()));
        let base_called_cloned = base_called.clone();
        let mut base = move |stage: InstallStage, _state: &mut InstallState| -> Result<()> {
            base_called_cloned.borrow_mut().push(stage);
            Ok(())
        };

        let saved_counts_cloned = saved_counts.clone();
        let mut save_fn =
            move |completed_stages: &[InstallStage], _state: &InstallState| -> Result<()> {
                saved_counts_cloned
                    .borrow_mut()
                    .push(completed_stages.len());
                Ok(())
            };

        let mut exec = make_checkpointing_executor(&mut completed, &mut save_fn, &mut base);

        let mut state = make_dummy_state();

        exec(InstallStage::DetectHardware, &mut state)?;
        exec(InstallStage::FormatPartitions, &mut state)?;

        // drop the executor so the mutable borrow on `completed` is released
        drop(exec);

        assert_eq!(completed.len(), 2);
        assert_eq!(&*saved_counts.borrow(), &vec![1, 2]);
        assert_eq!(
            &*base_called.borrow(),
            &vec![InstallStage::DetectHardware, InstallStage::FormatPartitions]
        );
        Ok(())
    }
}

/// Default executor that delegates stage execution to the system module.
pub fn default_executor(stage: InstallStage, state: &mut InstallState) -> Result<()> {
    system::execute_stage(stage, state)
}

/// Build a checkpointing executor closure that wraps a base executor.
/// The returned closure will call `base_executor`, then update `completed_stages`
/// and call `save_fn` to persist progress.
pub fn make_checkpointing_executor<'a, F, S>(
    completed_stages: &'a mut Vec<InstallStage>,
    mut save_fn: S,
    mut base_executor: F,
) -> impl FnMut(InstallStage, &mut InstallState) -> Result<()> + 'a
where
    F: FnMut(InstallStage, &mut InstallState) -> Result<()> + 'a,
    S: FnMut(&[InstallStage], &InstallState) -> Result<()> + 'a,
{
    move |stage: InstallStage, state: &mut InstallState| {
        base_executor(stage, state)?;

        if !completed_stages.contains(&stage) {
            completed_stages.push(stage);
        }

        save_fn(completed_stages.as_slice(), state)?;
        Ok(())
    }
}
