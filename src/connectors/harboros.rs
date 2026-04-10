//! HarborOS integration via middleware API and midcli.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarborOsRoute {
    MiddlewareApi,
    Midcli,
}
