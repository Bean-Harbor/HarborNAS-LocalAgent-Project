use std::path::PathBuf;

use clap::Parser;

use harbornas_local_agent::scripts::e2e::{run_e2e, write_json};
use harbornas_local_agent::scripts::integration::IntegrationConfig;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long)]
    env: String,
    #[arg(long, default_value = "e2e-report.json")]
    report: PathBuf,
    #[arg(long)]
    require_live: bool,
}

fn main() {
    let cli = Cli::parse();
    let root = std::env::current_dir().expect("failed to resolve current dir");
    let config = IntegrationConfig::from_env();

    let (e2e_payload, latency_payload, audit_payload) =
        run_e2e(&root, &cli.env, &config, cli.require_live);

    write_json(&cli.report, &e2e_payload).expect("failed to write e2e report");
    write_json(
        &cli.report.with_file_name("latency-summary.json"),
        &latency_payload,
    )
    .expect("failed to write latency report");
    write_json(
        &cli.report.with_file_name("audit-coverage-summary.json"),
        &audit_payload,
    )
    .expect("failed to write audit coverage report");

    if !e2e_payload.ok {
        std::process::exit(1);
    }
}
