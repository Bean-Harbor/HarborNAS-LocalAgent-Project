use std::path::PathBuf;

use clap::Parser;

use harbornas_local_agent::scripts::integration::IntegrationConfig;
use harbornas_local_agent::scripts::validate::run_validate;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long, default_value = "validate-contract-report.json")]
    report: PathBuf,
    #[arg(long)]
    skip_live: bool,
    #[arg(long)]
    require_live: bool,
}

fn main() {
    let cli = Cli::parse();
    let root = std::env::current_dir().expect("failed to resolve current dir");
    let config = IntegrationConfig::from_env();
    let payload = run_validate(&root, &config, cli.skip_live, cli.require_live);

    std::fs::write(
        &cli.report,
        serde_json::to_string_pretty(&payload).expect("failed to serialize report"),
    )
    .expect("failed to write report");

    if !payload.passed {
        std::process::exit(1);
    }
}
