use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    Pending,
    Approved,
    Executing,
    Success,
    Failed,
    Skipped,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    MiddlewareApi,
    Midcli,
    Browser,
    Mcp,
}

impl Route {
    pub fn as_str(&self) -> &'static str {
        match self {
            Route::MiddlewareApi => "middleware_api",
            Route::Midcli => "midcli",
            Route::Browser => "browser",
            Route::Mcp => "mcp",
        }
    }
}

pub const ROUTE_PRIORITY: [Route; 4] = [
    Route::MiddlewareApi,
    Route::Midcli,
    Route::Browser,
    Route::Mcp,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub domain: String,
    pub operation: String,
    pub resource: Value,
    #[serde(default)]
    pub args: Value,
    #[serde(default = "default_risk_level")]
    pub risk_level: RiskLevel,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub task_id: String,
    pub step_id: String,
    pub executor_used: String,
    #[serde(default)]
    pub fallback_used: bool,
    pub status: StepStatus,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
    #[serde(default = "new_audit_ref")]
    pub audit_ref: String,
    #[serde(default)]
    pub result_payload: Value,
}

impl ExecutionResult {
    pub fn ok(&self) -> bool {
        self.status == StepStatus::Success
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPlan {
    #[serde(default = "new_task_id")]
    pub task_id: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub steps: Vec<Action>,
}

impl TaskPlan {
    pub fn from_json_str(input: &str) -> Result<Self, serde_json::Error> {
        let mut plan: TaskPlan = serde_json::from_str(input)?;
        for action in &mut plan.steps {
            action.normalize();
        }
        Ok(plan)
    }

    pub fn add(&mut self, mut action: Action) {
        action.normalize();
        self.steps.push(action);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    pub task_id: String,
    pub results: Vec<ExecutionResult>,
}

impl TaskResult {
    pub fn new(task_id: String) -> Self {
        Self {
            task_id,
            results: Vec::new(),
        }
    }

    pub fn ok(&self) -> bool {
        self.results.iter().all(ExecutionResult::ok)
    }

    pub fn summary(&self) -> Value {
        let total_steps = self.results.len();
        let succeeded = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Success)
            .count();
        let failed = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Failed)
            .count();
        let blocked = self
            .results
            .iter()
            .filter(|r| r.status == StepStatus::Blocked)
            .count();
        serde_json::json!({
            "task_id": self.task_id,
            "total_steps": total_steps,
            "succeeded": succeeded,
            "failed": failed,
            "blocked": blocked,
            "ok": self.ok(),
        })
    }
}

impl Action {
    pub fn normalize(&mut self) {
        if matches!(self.risk_level, RiskLevel::High | RiskLevel::Critical) {
            self.requires_approval = true;
        }
    }
}

fn default_risk_level() -> RiskLevel {
    RiskLevel::Low
}

fn new_task_id() -> String {
    Uuid::new_v4().as_simple().to_string()
}

fn new_audit_ref() -> String {
    Uuid::new_v4().as_simple().to_string()[..12].to_string()
}
