//! Minimal tool-loop engine: LLM → tool-call → result → LLM (repeat).
//!
//! The loop runs at most `max_iterations` times. Each iteration either:
//! - selects a tool call from the pending `ToolCall` queue,
//! - executes it through the `Tool` trait, and
//! - pushes the `ToolOutput` back into the conversation trace.
//!
//! Termination conditions:
//! 1. A step yields `ToolCall::FinalAnswer`.
//! 2. `max_iterations` exhausted → returns partial trace.
//! 3. Cumulative duration exceeds `timeout_ms` → returns partial trace.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Tool trait
// ---------------------------------------------------------------------------

/// A concrete, invocable tool that the loop can dispatch to.
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn invoke(&self, args: Value) -> ToolOutput;
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// What the planner asks the loop to execute.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolCall {
    Invoke { tool: String, args: Value },
    FinalAnswer { answer: String },
}

/// Result of a single tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub tool: String,
    pub ok: bool,
    pub data: Value,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
}

/// One iteration inside the loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopStep {
    pub iteration: usize,
    pub call: ToolCall,
    pub output: Option<ToolOutput>,
}

/// Complete trace returned when the loop finishes.
#[derive(Debug, Clone, Serialize)]
pub struct ToolLoopTrace {
    pub finished: bool,
    pub iterations_used: usize,
    pub total_duration_ms: u64,
    pub steps: Vec<LoopStep>,
    pub final_answer: Option<String>,
}

// ---------------------------------------------------------------------------
// Registry (maps tool names → impls)
// ---------------------------------------------------------------------------

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|b| b.as_ref())
    }

    pub fn list_schemas(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": t.parameters_schema(),
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ToolLoopConfig {
    pub max_iterations: usize,
    pub timeout_ms: u64,
}

impl Default for ToolLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 8,
            timeout_ms: 30_000,
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Drives the tool-call loop.
///
/// The caller supplies a `resolve_fn` that takes the accumulated trace and
/// returns the next `ToolCall` the planner wants to execute. This keeps the
/// engine decoupled from any specific LLM client.
pub struct ToolLoopEngine {
    pub registry: ToolRegistry,
    pub config: ToolLoopConfig,
}

impl ToolLoopEngine {
    pub fn new(registry: ToolRegistry, config: ToolLoopConfig) -> Self {
        Self { registry, config }
    }

    /// Run the tool loop.
    ///
    /// `resolve_fn` is called once per iteration to decide what to do next.
    /// It receives a read-only view of steps so far and returns a `ToolCall`.
    pub fn run<F>(&self, mut resolve_fn: F) -> ToolLoopTrace
    where
        F: FnMut(&[LoopStep]) -> ToolCall,
    {
        let start = Instant::now();
        let timeout = Duration::from_millis(self.config.timeout_ms);

        let mut steps: Vec<LoopStep> = Vec::new();
        let mut final_answer: Option<String> = None;

        for iteration in 0..self.config.max_iterations {
            // Budget check
            if start.elapsed() >= timeout {
                break;
            }

            let call = resolve_fn(&steps);

            match &call {
                ToolCall::FinalAnswer { answer } => {
                    final_answer = Some(answer.clone());
                    steps.push(LoopStep {
                        iteration,
                        call,
                        output: None,
                    });
                    break;
                }
                ToolCall::Invoke { tool, args } => {
                    let t0 = Instant::now();
                    let output = match self.registry.get(tool) {
                        Some(t) => {
                            let mut out = t.invoke(args.clone());
                            out.duration_ms = t0.elapsed().as_millis() as u64;
                            out
                        }
                        None => ToolOutput {
                            tool: tool.clone(),
                            ok: false,
                            data: Value::Null,
                            error: Some(format!("unknown tool: {tool}")),
                            duration_ms: 0,
                        },
                    };
                    steps.push(LoopStep {
                        iteration,
                        call,
                        output: Some(output),
                    });
                }
            }
        }

        let finished = final_answer.is_some();
        ToolLoopTrace {
            finished,
            iterations_used: steps.len(),
            total_duration_ms: start.elapsed().as_millis() as u64,
            steps,
            final_answer,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;

    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echoes back the input"
        }
        fn parameters_schema(&self) -> Value {
            json!({"type": "object", "properties": {"msg": {"type": "string"}}})
        }
        fn invoke(&self, args: Value) -> ToolOutput {
            ToolOutput {
                tool: "echo".into(),
                ok: true,
                data: args,
                error: None,
                duration_ms: 0,
            }
        }
    }

    struct FailTool;

    impl Tool for FailTool {
        fn name(&self) -> &str {
            "fail"
        }
        fn description(&self) -> &str {
            "always fails"
        }
        fn parameters_schema(&self) -> Value {
            json!({})
        }
        fn invoke(&self, _args: Value) -> ToolOutput {
            ToolOutput {
                tool: "fail".into(),
                ok: false,
                data: Value::Null,
                error: Some("boom".into()),
                duration_ms: 0,
            }
        }
    }

    fn make_engine(tools: Vec<Box<dyn Tool>>) -> ToolLoopEngine {
        let mut reg = ToolRegistry::new();
        for t in tools {
            reg.register(t);
        }
        ToolLoopEngine::new(reg, ToolLoopConfig::default())
    }

    #[test]
    fn immediate_final_answer() {
        let engine = make_engine(vec![]);
        let trace = engine.run(|_steps| ToolCall::FinalAnswer {
            answer: "done".into(),
        });
        assert!(trace.finished);
        assert_eq!(trace.iterations_used, 1);
        assert_eq!(trace.final_answer.as_deref(), Some("done"));
    }

    #[test]
    fn single_tool_then_answer() {
        let engine = make_engine(vec![Box::new(EchoTool)]);
        let trace = engine.run(|steps| {
            if steps.is_empty() {
                ToolCall::Invoke {
                    tool: "echo".into(),
                    args: json!({"msg": "hi"}),
                }
            } else {
                ToolCall::FinalAnswer {
                    answer: "echoed".into(),
                }
            }
        });
        assert!(trace.finished);
        assert_eq!(trace.iterations_used, 2);
        let out = trace.steps[0].output.as_ref().unwrap();
        assert!(out.ok);
        assert_eq!(out.data, json!({"msg": "hi"}));
    }

    #[test]
    fn unknown_tool_returns_error() {
        let engine = make_engine(vec![]);
        let trace = engine.run(|steps| {
            if steps.is_empty() {
                ToolCall::Invoke {
                    tool: "nope".into(),
                    args: json!({}),
                }
            } else {
                ToolCall::FinalAnswer {
                    answer: "done".into(),
                }
            }
        });
        assert!(trace.finished);
        let out = trace.steps[0].output.as_ref().unwrap();
        assert!(!out.ok);
        assert!(out.error.as_ref().unwrap().contains("unknown tool"));
    }

    #[test]
    fn max_iterations_cap() {
        let engine = ToolLoopEngine::new(
            {
                let mut r = ToolRegistry::new();
                r.register(Box::new(EchoTool));
                r
            },
            ToolLoopConfig {
                max_iterations: 3,
                timeout_ms: 60_000,
            },
        );
        // Never return FinalAnswer – loop must stop at max_iterations
        let trace = engine.run(|_steps| ToolCall::Invoke {
            tool: "echo".into(),
            args: json!({"n": 1}),
        });
        assert!(!trace.finished);
        assert_eq!(trace.iterations_used, 3);
    }

    #[test]
    fn tool_failure_still_records_step() {
        let engine = make_engine(vec![Box::new(FailTool)]);
        let trace = engine.run(|steps| {
            if steps.is_empty() {
                ToolCall::Invoke {
                    tool: "fail".into(),
                    args: json!({}),
                }
            } else {
                ToolCall::FinalAnswer {
                    answer: "handled".into(),
                }
            }
        });
        assert!(trace.finished);
        let out = trace.steps[0].output.as_ref().unwrap();
        assert!(!out.ok);
        assert_eq!(out.error.as_deref(), Some("boom"));
    }

    #[test]
    fn registry_list_schemas() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(EchoTool));
        reg.register(Box::new(FailTool));
        let schemas = reg.list_schemas();
        assert_eq!(schemas.len(), 2);
        let names: Vec<&str> = schemas.iter().filter_map(|s| s["name"].as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"fail"));
    }

    #[test]
    fn trace_serializes_to_json() {
        let engine = make_engine(vec![Box::new(EchoTool)]);
        let trace = engine.run(|steps| {
            if steps.is_empty() {
                ToolCall::Invoke {
                    tool: "echo".into(),
                    args: json!({"x": 1}),
                }
            } else {
                ToolCall::FinalAnswer {
                    answer: "ok".into(),
                }
            }
        });
        let json_str = serde_json::to_string(&trace).unwrap();
        assert!(json_str.contains("\"finished\":true"));
        assert!(json_str.contains("\"final_answer\":\"ok\""));
    }
}
