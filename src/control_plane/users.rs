//! User, home, and membership domain types.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleKind {
    Owner,
    Admin,
    Operator,
    Member,
    Viewer,
    Guest,
}
