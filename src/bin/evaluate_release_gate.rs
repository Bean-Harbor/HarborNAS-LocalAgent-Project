use std::path::PathBuf;

use harborbeacon_local_agent::scripts::release_gate::evaluate_release_gate_with_model_benchmark;

#[derive(Debug, Clone)]
struct Cli {
    report_path: PathBuf,
    output: PathBuf,
    require_live: bool,
    model_benchmark_report: Option<PathBuf>,
    require_model_benchmark: bool,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            report_path: PathBuf::new(),
            output: PathBuf::from("release-gate-summary.json"),
            require_live: false,
            model_benchmark_report: None,
            require_model_benchmark: false,
        }
    }
}

impl Cli {
    fn parse() -> Self {
        let args = std::env::args().skip(1).collect::<Vec<_>>();
        if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
            print_usage();
            std::process::exit(0);
        }

        let mut cli = Self::default();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--output" => cli.output = PathBuf::from(take_value(&args, &mut index, "--output")),
                value if value.starts_with("--output=") => {
                    cli.output = PathBuf::from(value["--output=".len()..].to_string());
                }
                "--require-live" => cli.require_live = true,
                "--model-benchmark-report" => {
                    cli.model_benchmark_report = Some(PathBuf::from(take_value(
                        &args,
                        &mut index,
                        "--model-benchmark-report",
                    )))
                }
                value if value.starts_with("--model-benchmark-report=") => {
                    cli.model_benchmark_report = Some(PathBuf::from(
                        value["--model-benchmark-report=".len()..].to_string(),
                    ));
                }
                "--require-model-benchmark" => cli.require_model_benchmark = true,
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => fail(&format!("unknown flag: {value}")),
                value => {
                    if cli.report_path.as_os_str().is_empty() {
                        cli.report_path = PathBuf::from(value);
                    } else {
                        fail(&format!("unexpected positional argument: {value}"));
                    }
                }
            }
            index += 1;
        }

        if cli.report_path.as_os_str().is_empty() {
            fail("missing required positional argument: report_path");
        }

        cli
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
    eprintln!("Usage: evaluate-release-gate <report_path> [--output PATH] [--require-live] [--model-benchmark-report PATH] [--require-model-benchmark]");
}

fn main() {
    let cli = Cli::parse();
    let report = std::fs::read_to_string(&cli.report_path).expect("failed to read drift report");
    let report_json: serde_json::Value =
        serde_json::from_str(&report).expect("invalid drift report json");
    let model_benchmark_json = cli.model_benchmark_report.as_ref().map(|path| {
        let report = std::fs::read_to_string(path).expect("failed to read model benchmark report");
        serde_json::from_str::<serde_json::Value>(&report).expect("invalid model benchmark json")
    });

    let payload = evaluate_release_gate_with_model_benchmark(
        &report_json,
        cli.require_live,
        model_benchmark_json.as_ref(),
        cli.require_model_benchmark,
    );
    std::fs::write(
        &cli.output,
        serde_json::to_string_pretty(&payload).expect("failed to serialize release summary"),
    )
    .expect("failed to write release summary");

    if !payload.allowed {
        std::process::exit(1);
    }
}
