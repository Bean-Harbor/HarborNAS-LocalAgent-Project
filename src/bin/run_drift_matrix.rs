use std::path::PathBuf;

use clap::Parser;

use harbornas_local_agent::scripts::drift::run_drift_matrix;
use harbornas_local_agent::scripts::integration::IntegrationConfig;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long)]
    harbor_ref: String,
    #[arg(long)]
    upstream_ref: String,
    #[arg(long, default_value = "drift-matrix-report.json")]
    report: PathBuf,
    #[arg(long)]
    harbor_repo_path: Option<String>,
    #[arg(long)]
    upstream_repo_path: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let root = std::env::current_dir().expect("failed to resolve current dir");
    let config = IntegrationConfig::from_env();

    let payload = run_drift_matrix(
        &root,
        &config,
        &cli.harbor_ref,
        &cli.upstream_ref,
        cli.harbor_repo_path,
        cli.upstream_repo_path,
    );

    std::fs::write(
        &cli.report,
        serde_json::to_string_pretty(&payload).expect("failed to serialize drift matrix report"),
    )
    .expect("failed to write drift matrix report");

    if !payload.docs_missing.is_empty() {
        std::process::exit(1);
    }
}
