use std::fs;
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use serde_json::json;

use harbornas_local_agent::adapters::onvif::WsDiscoveryOnvifAdapter;
use harbornas_local_agent::adapters::rtsp::CommandRtspAdapter;
use harbornas_local_agent::orchestrator::approval::{AutonomyConfig, AutonomyLevel};
use harbornas_local_agent::orchestrator::executors::device_discovery::DeviceDiscoveryExecutor;
use harbornas_local_agent::orchestrator::executors::harbor_ops::{
    MidcliExecutor, MiddlewareExecutor, MiddlewareHttpExecutor, MiddlewareWsExecutor,
};
use harbornas_local_agent::orchestrator::executors::vision::VisionExecutor;
use harbornas_local_agent::orchestrator::tool_loop::{
    ToolCall, ToolLoopConfig, ToolLoopEngine, ToolRegistry,
};
use harbornas_local_agent::orchestrator::{
    ApprovalContext, ApprovalManager, Router, Runtime, TaskPlan,
};

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
    plan: Option<PathBuf>,

    #[arg(
        long,
        help = "Open an RTSP stream directly in a local player, e.g. rtsp://192.168.1.50/live"
    )]
    rtsp_open_url: Option<String>,

    #[arg(
        long,
        help = "Preferred RTSP player, e.g. ffplay | mpv | vlc | gst-launch-1.0 | open | iina"
    )]
    rtsp_player: Option<String>,

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

    #[arg(long, help = "HarborOS API base URL, e.g. http://192.168.3.61")]
    harbor_url: Option<String>,

    #[arg(long, help = "HarborOS API key (Bearer token auth)")]
    harbor_api_key: Option<String>,

    #[arg(
        long,
        help = "HarborOS API username (basic auth, used with --harbor-password)"
    )]
    harbor_user: Option<String>,

    #[arg(long, help = "HarborOS API password (basic auth)")]
    harbor_password: Option<String>,

    #[arg(long, value_enum, default_value_t = AutonomyArg::Supervised, help = "Autonomy level")]
    autonomy: AutonomyArg,

    #[arg(
        long,
        default_value_t = 8,
        help = "Max tool-loop iterations (loop mode)"
    )]
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

    if let Some(stream_url) = &cli.rtsp_open_url {
        run_rtsp_open_mode(&cli, stream_url);
        return;
    }

    let plan_path = match &cli.plan {
        Some(path) => path,
        None => {
            eprintln!("--plan is required unless --rtsp-open-url is provided");
            std::process::exit(1);
        }
    };

    let plan_text = match fs::read_to_string(plan_path) {
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

fn run_rtsp_open_mode(cli: &Cli, stream_url: &str) {
    let adapter = CommandRtspAdapter::default();
    let request = harbornas_local_agent::runtime::media::StreamOpenRequest::new(
        "direct-rtsp-open",
        stream_url,
        cli.rtsp_player.clone(),
    );

    match harbornas_local_agent::adapters::rtsp::RtspProbeAdapter::open_stream(&adapter, &request) {
        Ok(result) => match serde_json::to_string_pretty(&result) {
            Ok(out) => println!("{out}"),
            Err(e) => {
                eprintln!("failed to serialize RTSP open result: {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("failed to open RTSP stream: {e}");
            std::process::exit(1);
        }
    }
}

fn run_plan_mode(cli: &Cli, plan: TaskPlan) {
    let mut router = Router::new();
    let device_registry_path = PathBuf::from(".harbornas/device-registry.json");

    // If --harbor-url is provided, use WS executor (user+pass) or HTTP executor (api-key);
    // otherwise fall back to local preview/passthrough executors.
    if let Some(ref url) = cli.harbor_url {
        if let Some(ref api_key) = cli.harbor_api_key {
            // API key → HTTP REST executor
            match MiddlewareHttpExecutor::with_api_key(url, api_key) {
                Ok(exec) => router.register(Box::new(exec)),
                Err(e) => {
                    eprintln!("failed to create HTTP executor: {e}");
                    std::process::exit(1);
                }
            }
        } else if let (Some(ref user), Some(ref pass)) = (&cli.harbor_user, &cli.harbor_password) {
            // Username + password → WebSocket middleware executor
            router.register(Box::new(MiddlewareWsExecutor::new(url, user, pass)));
        } else {
            eprintln!("--harbor-url requires --harbor-api-key or --harbor-user/--harbor-password");
            std::process::exit(1);
        }
    } else if !cli.disable_middleware {
        router.register(Box::new(MiddlewareExecutor::new(true)));
    }

    if !cli.disable_midcli {
        router.register(Box::new(MidcliExecutor::new(
            true,
            "midcli".to_string(),
            cli.midcli_passthrough,
        )));
    }

    let device_executor = match DeviceDiscoveryExecutor::new(
        Box::new(CommandRtspAdapter::default()),
        Some(Box::new(WsDiscoveryOnvifAdapter::default())),
        None,
        None,
    )
    .with_registry_store(
        harbornas_local_agent::runtime::registry::DeviceRegistryStore::new(device_registry_path),
    ) {
        Ok(executor) => executor,
        Err(e) => {
            eprintln!("failed to initialize device registry: {e}");
            std::process::exit(1);
        }
    };
    router.register(Box::new(device_executor));
    router.register(Box::new(VisionExecutor::new(
        harbornas_local_agent::runtime::registry::DeviceRegistryStore::new(PathBuf::from(
            ".harbornas/device-registry.json",
        )),
    )));

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
                    steps_snapshot[done_count]["operation"]
                        .as_str()
                        .unwrap_or("?")
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
