//! Normalized AI provider interface boundary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    LocalSidecar,
    OpenAiCompatible,
    RemoteCloud,
    HarborOsService,
}
