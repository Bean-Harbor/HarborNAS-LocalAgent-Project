//! Storage connectors for local filesystems and HarborOS media archives.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageTarget {
    LocalDisk,
    HarborOsPool,
    ExternalShare,
}

impl Default for StorageTarget {
    fn default() -> Self {
        Self::LocalDisk
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageObjectRef {
    pub target: StorageTarget,
    pub relative_path: String,
}
