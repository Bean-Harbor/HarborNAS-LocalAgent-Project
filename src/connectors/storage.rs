//! Storage connectors for local filesystems and HarborOS media archives.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTarget {
    LocalDisk,
    HarborOsPool,
    ExternalShare,
}
