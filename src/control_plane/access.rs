//! Role and permission bindings for homes, rooms, and resources.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Platform,
    Home,
    Room,
    Resource,
}
