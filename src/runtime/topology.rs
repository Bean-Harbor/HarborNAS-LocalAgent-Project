//! Home, room, and zone topology mapping.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyKind {
    Home,
    Room,
    Zone,
}
