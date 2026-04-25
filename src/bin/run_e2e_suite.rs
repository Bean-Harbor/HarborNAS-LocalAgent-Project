use std::path::PathBuf;

use harborbeacon_local_agent::scripts::e2e::{run_e2e, write_json};
use harborbeacon_local_agent::scripts::integration::IntegrationConfig;

#[derive(Debug, Clone)]
struct Cli {
    env: String,
    report: PathBuf,
    require_live: bool,
}

impl Cli {
    fn parse() -> Self {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
            print_usage();
            std::process::exit(0);
        }

        let mut env = None;
        let mut report = PathBuf::from("e2e-report.json");
        let mut require_live = false;
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--env" => env = Some(take_value(&args, &mut index, "--env")),
                value if value.starts_with("--env=") => {
                    env = Some(value["--env=".len()..].to_string());
                }
                "--report" => report = PathBuf::from(take_value(&args, &mut index, "--report")),
                value if value.starts_with("--report=") => {
                    report = PathBuf::from(value["--report=".len()..].to_string());
                }
                "--require-live" => require_live = true,
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
            env: env.unwrap_or_else(|| fail("missing required flag --env")),
            report,
            require_live,
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
    eprintln!("Usage: run-e2e-suite --env NAME [--report PATH] [--require-live]");
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
