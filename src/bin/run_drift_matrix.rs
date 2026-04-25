use std::path::PathBuf;

use harborbeacon_local_agent::scripts::drift::run_drift_matrix;
use harborbeacon_local_agent::scripts::integration::IntegrationConfig;

#[derive(Debug, Clone)]
struct Cli {
    harbor_ref: String,
    upstream_ref: String,
    report: PathBuf,
    harbor_repo_path: Option<String>,
    upstream_repo_path: Option<String>,
}

impl Cli {
    fn parse() -> Self {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
            print_usage();
            std::process::exit(0);
        }

        let mut harbor_ref = None;
        let mut upstream_ref = None;
        let mut report = PathBuf::from("drift-matrix-report.json");
        let mut harbor_repo_path = None;
        let mut upstream_repo_path = None;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--harbor-ref" => harbor_ref = Some(take_value(&args, &mut index, "--harbor-ref")),
                value if value.starts_with("--harbor-ref=") => {
                    harbor_ref = Some(value["--harbor-ref=".len()..].to_string());
                }
                "--upstream-ref" => {
                    upstream_ref = Some(take_value(&args, &mut index, "--upstream-ref"))
                }
                value if value.starts_with("--upstream-ref=") => {
                    upstream_ref = Some(value["--upstream-ref=".len()..].to_string());
                }
                "--report" => report = PathBuf::from(take_value(&args, &mut index, "--report")),
                value if value.starts_with("--report=") => {
                    report = PathBuf::from(value["--report=".len()..].to_string());
                }
                "--harbor-repo-path" => {
                    harbor_repo_path = Some(take_value(&args, &mut index, "--harbor-repo-path"))
                }
                value if value.starts_with("--harbor-repo-path=") => {
                    harbor_repo_path = Some(value["--harbor-repo-path=".len()..].to_string());
                }
                "--upstream-repo-path" => {
                    upstream_repo_path = Some(take_value(&args, &mut index, "--upstream-repo-path"))
                }
                value if value.starts_with("--upstream-repo-path=") => {
                    upstream_repo_path = Some(value["--upstream-repo-path=".len()..].to_string());
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => fail(&format!("unknown flag: {value}")),
                value => fail(&format!("unexpected positional argument: {value}")),
            }
            index += 1;
        }

        Self {
            harbor_ref: harbor_ref.unwrap_or_else(|| fail("missing required flag --harbor-ref")),
            upstream_ref: upstream_ref
                .unwrap_or_else(|| fail("missing required flag --upstream-ref")),
            report,
            harbor_repo_path,
            upstream_repo_path,
        }
    }
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> String {
    *index += 1;
    if *index >= args.len() {
        fail(&format!("missing value for {flag}"));
    }
    args[*index].clone()
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn print_usage() {
    eprintln!(
        "Usage: run-drift-matrix --harbor-ref REF --upstream-ref REF [--report PATH] [--harbor-repo-path PATH] [--upstream-repo-path PATH]"
    );
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
