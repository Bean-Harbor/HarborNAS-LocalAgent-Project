use std::fs;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde_json::json;

use harbornas_local_agent::orchestrator::approval::{AutonomyConfig, AutonomyLevel};
use harbornas_local_agent::orchestrator::executors::harbor_ops::{
    MiddlewareExecutor, MidcliExecutor,
};
use harbornas_local_agent::orchestrator::tool_loop::{
    ToolCall, ToolLoopConfig, ToolLoopEngine, ToolRegistry,
};
use harbornas_local_agent::orchestrator::{ApprovalContext, ApprovalManager, Router, Runtime, TaskPlan};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RunMode {
    /// Classic: read plan JSON, execute, print report
    Plan,
    /// Tool-loop: run LLM→tool→LLM iteration from plan JSON
    Loop,
}

#[derive(Debug, Parser)]
#[command(name = "harbornas-agent")]
#[command(about = "HarborNAS local assistant runtime (Rust)")]
struct Cli {
    #[arg(long, help = "Path to task plan JSON file")]
    plan: PathBuf,

    #[arg(long, value_enum, default_value_t = RunMode::Plan, help = "Execution mode: plan | loop")]
    mode: RunMode,

    #[arg(long, help = "Approval token used for HIGH/CRITICAL actions")]
    approval_token: Option<String>,

    #[arg(long, help = "Expected approval token value")]
    required_approval_token: Option<String>,

    #[arg(long, help = "Disable middleware_api route")]
    disable_middleware: bool,

    #[arg(long, help = "Disable midcli route")]
    disable_midcli: bool,

    #[arg(long, help = "Execute real midcli command instead of preview mode")]
    midcli_passthrough: bool,

    #[arg(long, help = "Force all actions to run as dry-run")]
    force_dry_run: bool,

    #[arg(long, value_enum, default_value_t = AutonomyArg::Supervised, help = "Autonomy level")]
    autonomy: AutonomyArg,

    #[arg(long, default_value_t = 8, help = "Max tool-loop iterations (loop mode)")]
    max_iterations: usize,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AutonomyArg {
    Readonly,
    Supervised,
    Full,
}

impl From<AutonomyArg> for AutonomyLevel {
    fn from(a: AutonomyArg) -> Self {
        match a {
            AutonomyArg::Readonly => AutonomyLevel::ReadOnly,
            AutonomyArg::Supervised => AutonomyLevel::Supervised,
            AutonomyArg::Full => AutonomyLevel::Full,
        }
    }
}

fn main() {
    let cli = Cli::parse();

    let plan_text = match fs::read_to_string(&cli.plan) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read plan file: {e}");
            std::process::exit(1);
        }
    };

    let mut plan = match TaskPlan::from_json_str(&plan_text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("failed to parse plan JSON: {e}");
            std::process::exit(1);
        }
    };

    if cli.force_dry_run {
        for action in &mut plan.steps {
            action.dry_run = true;
        }
    }

    // Build approval manager
    let _approval_mgr = ApprovalManager::from_config(&AutonomyConfig {
        level: cli.autonomy.into(),
        auto_approve: vec![],
        always_ask: vec![],
        non_cli_excluded: vec![],
    });

    match cli.mode {
        RunMode::Plan => run_plan_mode(&cli, plan),
        RunMode::Loop => run_loop_mode(&cli, &plan),
    }
}

fn run_plan_mode(cli: &Cli, plan: TaskPlan) {
    let mut router = Router::new();
    if !cli.disable_middleware {
        router.register(Box::new(MiddlewareExecutor::new(true)));
    }
    if !cli.disable_midcli {
        router.register(Box::new(MidcliExecutor::new(
            true,
            "midcli".to_string(),
            cli.midcli_passthrough,
        )));
    }

    let approval = ApprovalContext {
        token: cli.approval_token.clone(),
        required_token: cli.required_approval_token.clone(),
        approver_id: None,
    };

    let mut runtime = Runtime::new(router, Some(approval));
    let task_result = runtime.execute_plan(plan);

    let report = json!({
        "task_result": task_result,
        "summary": task_result.summary(),
        "audit_events": runtime.audit().events(),
    });

    match serde_json::to_string_pretty(&report) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("failed to serialize report: {e}");
            std::process::exit(1);
        }
    }
}

fn run_loop_mode(cli: &Cli, plan: &TaskPlan) {
    let registry = ToolRegistry::new();
    // In production: register real tools from skills registry here.

    let engine = ToolLoopEngine::new(
        registry,
        ToolLoopConfig {
            max_iterations: cli.max_iterations,
            timeout_ms: 30_000,
        },
    );

    // The resolve_fn simulates planner logic: emit plan steps as tool calls,
    // then FinalAnswer. A real integration replaces this with LLM calls.
    let steps_snapshot: Vec<_> = plan
        .steps
        .iter()
        .map(|a| {
            json!({
                "domain": a.domain,
                "operation": a.operation,
                "resource": a.resource,
            })
        })
        .collect();

    let total_planned = steps_snapshot.len();

    let trace = engine.run(|history| {
        let done_count = history.len();
        if done_count < total_planned {
            ToolCall::Invoke {
                tool: format!(
                    "{}.{}",
                    steps_snapshot[done_count]["domain"].as_str().unwrap_or("?"),
                    steps_snapshot[done_count]["operation"].as_str().unwrap_or("?")
                ),
                args: steps_snapshot[done_count].clone(),
            }
        } else {
            ToolCall::FinalAnswer {
                answer: format!("completed {total_planned} steps via tool-loop"),
            }
        }
    });

    match serde_json::to_string_pretty(&trace) {
        Ok(out) => println!("{out}"),
        Err(e) => {
            eprintln!("failed to serialize trace: {e}");
            std::process::exit(1);
        }
    }
}
