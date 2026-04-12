use std::collections::HashMap;

use crate::orchestrator::contracts::{Action, ExecutionResult, Route, StepStatus, ROUTE_PRIORITY};

pub trait Executor {
    fn route(&self) -> Route;
    fn supports(&self, _action: &Action) -> bool {
        true
    }
    fn is_available(&self) -> bool;
    fn execute(
        &self,
        action: &Action,
        task_id: &str,
        step_id: &str,
    ) -> Result<ExecutionResult, String>;
}

pub struct Router {
    executors: HashMap<Route, Vec<Box<dyn Executor + Send + Sync>>>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
        }
    }

    pub fn register(&mut self, executor: Box<dyn Executor + Send + Sync>) {
        self.executors
            .entry(executor.route())
            .or_default()
            .push(executor);
    }

    pub fn execute(&self, action: &Action, task_id: &str, step_id: &str) -> ExecutionResult {
        let routes = allowed_routes(action);
        let mut last_error: Option<String> = None;
        let mut fallback_used = false;

        for (idx, route) in routes.iter().enumerate() {
            let Some(executors) = self.executors.get(route) else {
                continue;
            };
            if idx > 0 {
                fallback_used = true;
            }

            for executor in executors {
                if !executor.supports(action) {
                    continue;
                }
                if !executor.is_available() {
                    continue;
                }

                match executor.execute(action, task_id, step_id) {
                    Ok(mut result) => {
                        result.fallback_used = fallback_used;
                        if result.executor_used.is_empty() {
                            result.executor_used = route.as_str().to_string();
                        }
                        return result;
                    }
                    Err(err) => {
                        last_error = Some(err);
                    }
                }
            }
        }

        ExecutionResult {
            task_id: task_id.to_string(),
            step_id: step_id.to_string(),
            executor_used: "none".to_string(),
            fallback_used,
            status: StepStatus::Failed,
            duration_ms: 0,
            error_code: Some("NO_EXECUTOR_AVAILABLE".to_string()),
            error_message: Some(
                last_error.unwrap_or_else(|| "No executor available for this action".to_string()),
            ),
            audit_ref: String::new(),
            result_payload: serde_json::json!({}),
        }
    }
}

pub fn allowed_routes(action: &Action) -> Vec<Route> {
    if matches!(action.domain.as_str(), "service" | "files") {
        return ROUTE_PRIORITY
            .iter()
            .copied()
            .filter(|r| matches!(r, Route::MiddlewareApi | Route::Midcli))
            .collect();
    }
    ROUTE_PRIORITY.to_vec()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::orchestrator::contracts::{Action, ExecutionResult, RiskLevel, Route, StepStatus};

    use super::{allowed_routes, Executor, Router};

    struct MockExecutor {
        route: Route,
        available: bool,
        fail: bool,
    }

    impl Executor for MockExecutor {
        fn route(&self) -> Route {
            self.route
        }

        fn supports(&self, _action: &Action) -> bool {
            true
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn execute(
            &self,
            _action: &Action,
            task_id: &str,
            step_id: &str,
        ) -> Result<ExecutionResult, String> {
            if self.fail {
                return Err("forced failure".to_string());
            }
            Ok(ExecutionResult {
                task_id: task_id.to_string(),
                step_id: step_id.to_string(),
                executor_used: self.route.as_str().to_string(),
                fallback_used: false,
                status: StepStatus::Success,
                duration_ms: 1,
                error_code: None,
                error_message: None,
                audit_ref: String::new(),
                result_payload: json!({"ok": true}),
            })
        }
    }

    #[test]
    fn harbor_domains_use_api_then_midcli_only() {
        let action = Action {
            domain: "service".to_string(),
            operation: "status".to_string(),
            resource: json!({"service_name": "ssh"}),
            args: json!({}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };
        let routes = allowed_routes(&action);
        assert_eq!(routes, vec![Route::MiddlewareApi, Route::Midcli]);
    }

    #[test]
    fn fallback_moves_to_midcli() {
        let mut router = Router::new();
        router.register(Box::new(MockExecutor {
            route: Route::MiddlewareApi,
            available: true,
            fail: true,
        }));
        router.register(Box::new(MockExecutor {
            route: Route::Midcli,
            available: true,
            fail: false,
        }));

        let action = Action {
            domain: "service".to_string(),
            operation: "status".to_string(),
            resource: json!({"service_name": "ssh"}),
            args: json!({}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };

        let result = router.execute(&action, "t1", "s1");
        assert_eq!(result.executor_used, "midcli");
        assert!(result.fallback_used);
    }

    #[test]
    fn multiple_executors_can_share_same_route() {
        let mut router = Router::new();
        router.register(Box::new(MockExecutor {
            route: Route::Mcp,
            available: true,
            fail: true,
        }));
        router.register(Box::new(MockExecutor {
            route: Route::Mcp,
            available: true,
            fail: false,
        }));

        let action = Action {
            domain: "device".to_string(),
            operation: "ptz".to_string(),
            resource: json!({"device_id": "cam-1"}),
            args: json!({}),
            risk_level: RiskLevel::Low,
            requires_approval: false,
            dry_run: false,
        };

        let result = router.execute(&action, "t1", "s1");
        assert_eq!(result.executor_used, "mcp");
        assert_eq!(result.status, StepStatus::Success);
    }
}
