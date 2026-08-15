use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
}

impl Device {
    pub fn from_name(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn path(&self) -> String {
        format!("/dev/{}", self.name)
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Disk {
    pub device: Device,
    pub size: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Partition {
    pub device: Device,
    pub size: String,
    pub uuid: String,
}

impl Partition {
    pub fn from_lsblk(
        name: impl Into<String>,
        size: impl Into<String>,
        uuid: impl Into<String>,
    ) -> Self {
        Self {
            device: Device::from_name(name),
            size: size.into(),
            uuid: uuid.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.device.name
    }

    pub fn path(&self) -> String {
        self.device.path()
    }

    pub fn set_uuid(&mut self, uuid: impl Into<String>) {
        self.uuid = uuid.into();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskLayout {
    pub efi_partition: Option<Partition>,
    pub root_partition: Partition,
    pub home_partition: Option<Partition>,
    pub recovery_partition: Option<Partition>,
    pub swap_partition: Option<Partition>,
}
