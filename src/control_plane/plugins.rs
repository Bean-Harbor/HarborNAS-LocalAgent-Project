//! Plugin registry and manifest boundaries.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRuntime {
    NativeRust,
    HttpSidecar,
    WasmSandbox,
}
