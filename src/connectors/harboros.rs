//! HarborOS integration via middleware API and midcli.
//!
//! This module keeps the HarborOS connector boundary tied to the real
//! HarborOS / TrueNAS-style surface:
//! - real middleware methods
//! - real MidCLI command shapes
//! - preview-only file substrate entries are explicitly marked as scaffold
//!   support for framework consumption, not native HarborOS business logic.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarborOsRoute {
    MiddlewareApi,
    Midcli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarborOsParityKind {
    Real,
    ScaffoldOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarborOsInterfaceSurface {
    pub capability: &'static str,
    pub middleware_method: Option<&'static str>,
    pub midcli_example: Option<&'static str>,
    pub parity_kind: HarborOsParityKind,
    pub notes: &'static str,
}

pub const HARBOROS_INTERFACE_SURFACES: &[HarborOsInterfaceSurface] = &[
    HarborOsInterfaceSurface {
        capability: "service.query",
        middleware_method: Some("service.query"),
        midcli_example: Some("service query service,state,enable WHERE service == '<service>'"),
        parity_kind: HarborOsParityKind::Real,
        notes: "Real HarborOS service inspection surface available through middleware and MidCLI.",
    },
    HarborOsInterfaceSurface {
        capability: "service.control",
        middleware_method: Some("service.control"),
        midcli_example: Some("service start service=<service>"),
        parity_kind: HarborOsParityKind::Real,
        notes: "Real HarborOS service mutation surface; approval and risk gates still apply.",
    },
    HarborOsInterfaceSurface {
        capability: "files.copy",
        middleware_method: Some("filesystem.copy"),
        midcli_example: Some("filesystem copy src=<src> dst=<dst>"),
        parity_kind: HarborOsParityKind::Real,
        notes: "Real filesystem copy mapping on the HarborOS / TrueNAS execution surface.",
    },
    HarborOsInterfaceSurface {
        capability: "files.move",
        middleware_method: Some("filesystem.move"),
        midcli_example: Some("filesystem move src=<src> dst=<dst>"),
        parity_kind: HarborOsParityKind::Real,
        notes: "Real filesystem move mapping on the HarborOS / TrueNAS execution surface.",
    },
    HarborOsInterfaceSurface {
        capability: "files.list",
        middleware_method: Some("filesystem.listdir"),
        midcli_example: Some("filesystem listdir path=<path>"),
        parity_kind: HarborOsParityKind::Real,
        notes: "Read-only directory listing is mapped to real system tooling and stays below retrieval semantics.",
    },
    HarborOsInterfaceSurface {
        capability: "files.stat",
        middleware_method: None,
        midcli_example: None,
        parity_kind: HarborOsParityKind::ScaffoldOnly,
        notes: "Framework preview helper only; keep it scoped to safe metadata exposure and do not treat it as a native HarborOS product surface.",
    },
    HarborOsInterfaceSurface {
        capability: "files.read_text",
        middleware_method: None,
        midcli_example: None,
        parity_kind: HarborOsParityKind::ScaffoldOnly,
        notes: "Framework preview helper only; do not lift ranking, chunking, citation, or answer generation into HarborOS.",
    },
];

pub fn harboros_interface_surface(capability: &str) -> Option<&'static HarborOsInterfaceSurface> {
    HARBOROS_INTERFACE_SURFACES
        .iter()
        .find(|surface| surface.capability == capability)
}

pub fn harboros_real_interface_surfaces() -> &'static [HarborOsInterfaceSurface] {
    HARBOROS_INTERFACE_SURFACES
}

#[cfg(test)]
mod tests {
    use super::{harboros_interface_surface, HarborOsParityKind};

    #[test]
    fn real_harboros_surfaces_are_explicit_and_scaffold_only_entries_are_marked() {
        let service_query = harboros_interface_surface("service.query").unwrap();
        assert_eq!(service_query.parity_kind, HarborOsParityKind::Real);
        assert_eq!(service_query.middleware_method, Some("service.query"));
        assert!(service_query
            .midcli_example
            .unwrap()
            .starts_with("service query"));

        let read_text = harboros_interface_surface("files.read_text").unwrap();
        assert_eq!(read_text.parity_kind, HarborOsParityKind::ScaffoldOnly);
        assert!(read_text.middleware_method.is_none());
        assert!(read_text.midcli_example.is_none());
    }
}
