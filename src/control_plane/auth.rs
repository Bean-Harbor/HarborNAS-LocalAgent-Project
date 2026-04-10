//! Authentication entrypoints for local, HarborOS, and IM identities.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    Local,
    HarborOs,
    ImChannel,
}
