use std::path::PathBuf;

use clap::Parser;

use harbornas_local_agent::scripts::release_gate::evaluate_release_gate;

#[derive(Debug, Parser)]
struct Cli {
    report_path: PathBuf,
    #[arg(long, default_value = "release-gate-summary.json")]
    output: PathBuf,
    #[arg(long)]
    require_live: bool,
}

fn main() {
    let cli = Cli::parse();
    let report = std::fs::read_to_string(&cli.report_path).expect("failed to read drift report");
    let report_json: serde_json::Value =
        serde_json::from_str(&report).expect("invalid drift report json");

    let payload = evaluate_release_gate(&report_json, cli.require_live);
    std::fs::write(
        &cli.output,
        serde_json::to_string_pretty(&payload).expect("failed to serialize release summary"),
    )
    .expect("failed to write release summary");

    if !payload.allowed {
        std::process::exit(1);
    }
}
