use crate::orchestrator::audit::AuditLog;
use crate::orchestrator::contracts::{ExecutionResult, StepStatus, TaskPlan, TaskResult};
use crate::orchestrator::policy::{enforce, ApprovalContext};
use crate::orchestrator::router::Router;

pub struct Runtime {
    router: Router,
    audit: AuditLog,
    approval: Option<ApprovalContext>,
}

impl Runtime {
    pub fn new(router: Router, approval: Option<ApprovalContext>) -> Self {
        Self {
            router,
            audit: AuditLog::new(),
            approval,
        }
    }

    pub fn audit(&self) -> &AuditLog {
        &self.audit
    }

    pub fn execute_plan(&mut self, plan: TaskPlan) -> TaskResult {
        let mut task_result = TaskResult::new(plan.task_id.clone());

        for (idx, action) in plan.steps.iter().enumerate() {
            let step_id = format!("s{}", idx + 1);
            let event_id = self.audit.record_start(&plan.task_id, &step_id, action);
            let audit_ref = self.audit.audit_ref_for(event_id);

            if let Err(pv) = enforce(action, self.approval.as_ref()) {
                self.audit
                    .record_policy_block(event_id, &pv.code, &pv.message);
                task_result.results.push(ExecutionResult {
                    task_id: plan.task_id.clone(),
                    step_id,
                    executor_used: "none".to_string(),
                    fallback_used: false,
                    status: StepStatus::Blocked,
                    duration_ms: 0,
                    error_code: Some(pv.code),
                    error_message: Some(pv.message),
                    audit_ref,
                    result_payload: serde_json::json!({}),
                });
                continue;
            }

            if action.dry_run {
                let result = ExecutionResult {
                    task_id: plan.task_id.clone(),
                    step_id,
                    executor_used: "dry_run".to_string(),
                    fallback_used: false,
                    status: StepStatus::Success,
                    duration_ms: 0,
                    error_code: None,
                    error_message: None,
                    audit_ref,
                    result_payload: serde_json::to_value(action)
                        .unwrap_or_else(|_| serde_json::json!({})),
                };
                self.audit.record_complete(event_id, &result);
                task_result.results.push(result);
                continue;
            }

            let mut result = self.router.execute(action, &plan.task_id, &step_id);
            result.audit_ref = audit_ref;
            self.audit.record_complete(event_id, &result);
            task_result.results.push(result);
        }

        task_result
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::orchestrator::contracts::{Action, RiskLevel, TaskPlan};
    use crate::orchestrator::router::Router;

    use super::Runtime;

    #[test]
    fn high_risk_without_approval_gets_blocked() {
        let action = Action {
            domain: "service".to_string(),
            operation: "restart".to_string(),
            resource: json!({"service_name": "ssh"}),
            args: json!({}),
            risk_level: RiskLevel::High,
            requires_approval: true,
            dry_run: false,
        };

        let plan = TaskPlan {
            task_id: "t1".to_string(),
            goal: "test".to_string(),
            steps: vec![action],
        };

        let mut runtime = Runtime::new(Router::new(), None);
        let result = runtime.execute_plan(plan);
        assert_eq!(result.results.len(), 1);
        assert_eq!(format!("{:?}", result.results[0].status), "Blocked");
    }
}
