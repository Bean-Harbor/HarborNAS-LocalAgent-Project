//! Interactive / non-interactive approval workflow for supervised mode.
//!
//! Provides a pre-execution hook that checks tool calls against
//! auto_approve / always_ask lists, session-scoped allowlists, and
//! autonomy level policy.  Mirrors ZeroClaw `ApprovalManager` semantics
//! scoped to the HarborOS assistant domain.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::orchestrator::contracts::RiskLevel;

// ── Autonomy level ──────────────────────────────────────────────

/// Runtime autonomy level governing how much the assistant can do
/// without explicit human approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutonomyLevel {
    /// Read-only: all write side-effects are blocked.
    ReadOnly,
    /// Supervised (default): auto_approve list executes freely;
    /// everything else requires approval.
    Supervised,
    /// Full: execute all allowed tools without approval prompts.
    Full,
}

impl Default for AutonomyLevel {
    fn default() -> Self {
        Self::Supervised
    }
}

// ── Autonomy config ─────────────────────────────────────────────

/// Declarative approval policy loaded from config (TOML / JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyConfig {
    #[serde(default)]
    pub level: AutonomyLevel,
    /// Tools that never need approval (e.g. `["service.status", "files.search"]`).
    #[serde(default)]
    pub auto_approve: Vec<String>,
    /// Tools that *always* need approval, overriding session allowlist.
    #[serde(default)]
    pub always_ask: Vec<String>,
    /// Tools excluded from non-CLI execution even in Full mode.
    #[serde(default)]
    pub non_cli_excluded: Vec<String>,
}

impl Default for AutonomyConfig {
    fn default() -> Self {
        Self {
            level: AutonomyLevel::Supervised,
            auto_approve: vec![],
            always_ask: vec![],
            non_cli_excluded: vec![],
        }
    }
}

// ── Approval response ───────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalResponse {
    Yes,
    No,
    Always,
}

// ── Audit entry ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalLogEntry {
    pub tool_name: String,
    pub arguments_summary: String,
    pub decision: ApprovalResponse,
    pub channel: String,
}

// ── ApprovalManager ─────────────────────────────────────────────

pub struct ApprovalManager {
    auto_approve: HashSet<String>,
    always_ask: HashSet<String>,
    autonomy_level: AutonomyLevel,
    non_interactive: bool,
    session_allowlist: HashSet<String>,
    audit_log: Vec<ApprovalLogEntry>,
}

impl ApprovalManager {
    /// Interactive manager (CLI).
    pub fn from_config(config: &AutonomyConfig) -> Self {
        Self {
            auto_approve: config.auto_approve.iter().cloned().collect(),
            always_ask: config.always_ask.iter().cloned().collect(),
            autonomy_level: config.level,
            non_interactive: false,
            session_allowlist: HashSet::new(),
            audit_log: Vec::new(),
        }
    }

    /// Non-interactive manager (channel / daemon).
    pub fn for_non_interactive(config: &AutonomyConfig) -> Self {
        Self {
            auto_approve: config.auto_approve.iter().cloned().collect(),
            always_ask: config.always_ask.iter().cloned().collect(),
            autonomy_level: config.level,
            non_interactive: true,
            session_allowlist: HashSet::new(),
            audit_log: Vec::new(),
        }
    }

    pub fn is_non_interactive(&self) -> bool {
        self.non_interactive
    }

    pub fn autonomy_level(&self) -> AutonomyLevel {
        self.autonomy_level
    }

    /// Check whether a tool call needs approval before execution.
    pub fn needs_approval(&self, tool_name: &str) -> bool {
        if self.autonomy_level == AutonomyLevel::Full {
            return false;
        }
        if self.autonomy_level == AutonomyLevel::ReadOnly {
            return false; // blocked elsewhere
        }
        if self.always_ask.contains("*") || self.always_ask.contains(tool_name) {
            return true;
        }
        // Non-interactive shell gets through to its own risk gate.
        if self.non_interactive && tool_name == "shell" {
            return false;
        }
        if self.auto_approve.contains("*") || self.auto_approve.contains(tool_name) {
            return false;
        }
        if self.session_allowlist.contains(tool_name) {
            return false;
        }
        true // supervised default
    }

    /// Check whether the given risk level is allowed under current autonomy.
    pub fn risk_allowed(&self, risk: RiskLevel) -> bool {
        match self.autonomy_level {
            AutonomyLevel::ReadOnly => matches!(risk, RiskLevel::Low),
            AutonomyLevel::Supervised => true, // approval gate handles it
            AutonomyLevel::Full => true,
        }
    }

    pub fn record_decision(
        &mut self,
        tool_name: &str,
        args_summary: &str,
        decision: ApprovalResponse,
        channel: &str,
    ) {
        if decision == ApprovalResponse::Always {
            self.session_allowlist.insert(tool_name.to_string());
        }
        self.audit_log.push(ApprovalLogEntry {
            tool_name: tool_name.to_string(),
            arguments_summary: args_summary.to_string(),
            decision,
            channel: channel.to_string(),
        });
    }

    pub fn audit_log(&self) -> &[ApprovalLogEntry] {
        &self.audit_log
    }

    pub fn session_allowlist(&self) -> &HashSet<String> {
        &self.session_allowlist
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn supervised_config() -> AutonomyConfig {
        AutonomyConfig {
            level: AutonomyLevel::Supervised,
            auto_approve: vec!["service.status".into(), "files.search".into()],
            always_ask: vec!["service.stop".into()],
            non_cli_excluded: vec![],
        }
    }

    #[test]
    fn auto_approve_skips_prompt() {
        let mgr = ApprovalManager::from_config(&supervised_config());
        assert!(!mgr.needs_approval("service.status"));
        assert!(!mgr.needs_approval("files.search"));
    }

    #[test]
    fn always_ask_overrides_everything() {
        let mgr = ApprovalManager::from_config(&supervised_config());
        assert!(mgr.needs_approval("service.stop"));
    }

    #[test]
    fn unknown_tool_needs_approval_in_supervised() {
        let mgr = ApprovalManager::from_config(&supervised_config());
        assert!(mgr.needs_approval("service.restart"));
    }

    #[test]
    fn full_autonomy_never_prompts() {
        let cfg = AutonomyConfig {
            level: AutonomyLevel::Full,
            ..AutonomyConfig::default()
        };
        let mgr = ApprovalManager::from_config(&cfg);
        assert!(!mgr.needs_approval("service.stop"));
        assert!(!mgr.needs_approval("anything"));
    }

    #[test]
    fn readonly_never_prompts() {
        let cfg = AutonomyConfig {
            level: AutonomyLevel::ReadOnly,
            ..AutonomyConfig::default()
        };
        let mgr = ApprovalManager::from_config(&cfg);
        assert!(!mgr.needs_approval("shell"));
    }

    #[test]
    fn always_response_adds_to_session_allowlist() {
        let mut mgr = ApprovalManager::from_config(&supervised_config());
        assert!(mgr.needs_approval("service.restart"));
        mgr.record_decision("service.restart", "ssh", ApprovalResponse::Always, "cli");
        assert!(!mgr.needs_approval("service.restart"));
    }

    #[test]
    fn always_ask_overrides_session_allowlist() {
        let mut mgr = ApprovalManager::from_config(&supervised_config());
        mgr.record_decision("service.stop", "ssh", ApprovalResponse::Always, "cli");
        assert!(mgr.needs_approval("service.stop"));
    }

    #[test]
    fn non_interactive_shell_bypasses_outer_gate() {
        let mgr = ApprovalManager::for_non_interactive(&AutonomyConfig::default());
        assert!(!mgr.needs_approval("shell"));
    }

    #[test]
    fn non_interactive_unknown_tools_need_approval() {
        let mgr = ApprovalManager::for_non_interactive(&supervised_config());
        assert!(mgr.needs_approval("service.restart"));
    }

    #[test]
    fn risk_allowed_readonly_blocks_non_low() {
        let cfg = AutonomyConfig {
            level: AutonomyLevel::ReadOnly,
            ..AutonomyConfig::default()
        };
        let mgr = ApprovalManager::from_config(&cfg);
        assert!(mgr.risk_allowed(RiskLevel::Low));
        assert!(!mgr.risk_allowed(RiskLevel::High));
        assert!(!mgr.risk_allowed(RiskLevel::Critical));
    }

    #[test]
    fn audit_log_records_decisions() {
        let mut mgr = ApprovalManager::from_config(&supervised_config());
        mgr.record_decision("service.stop", "ssh", ApprovalResponse::No, "telegram");
        mgr.record_decision("service.start", "ssh", ApprovalResponse::Yes, "cli");
        assert_eq!(mgr.audit_log().len(), 2);
        assert_eq!(mgr.audit_log()[0].decision, ApprovalResponse::No);
    }
}
