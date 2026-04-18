use std::path::PathBuf;

use harborbeacon_local_agent::scripts::release_gate::evaluate_release_gate;

#[derive(Debug, Clone)]
struct Cli {
    report_path: PathBuf,
    output: PathBuf,
    require_live: bool,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            report_path: PathBuf::new(),
            output: PathBuf::from("release-gate-summary.json"),
            require_live: false,
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
    eprintln!("Usage: evaluate-release-gate <report_path> [--output PATH] [--require-live]");
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
