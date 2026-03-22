use std::fs;
use std::path::PathBuf;

use clap::Parser;
use serde_json::json;

use harbornas_local_agent::orchestrator::executors::harbor_ops::{
    MiddlewareExecutor, MidcliExecutor,
};
use harbornas_local_agent::orchestrator::{ApprovalContext, Router, Runtime, TaskPlan};

#[derive(Debug, Parser)]
#[command(name = "harbornas-agent")]
#[command(about = "HarborNAS local assistant runtime (Rust)")]
struct Cli {
    #[arg(long, help = "Path to task plan JSON file")]
    plan: PathBuf,

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
        token: cli.approval_token,
        required_token: cli.required_approval_token,
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
