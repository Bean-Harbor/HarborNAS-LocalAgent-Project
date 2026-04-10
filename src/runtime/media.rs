//! Snapshot, clip, stream, and timeline processing primitives.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAssetKind {
    Snapshot,
    Clip,
    Stream,
    TimelineEntry,
}
