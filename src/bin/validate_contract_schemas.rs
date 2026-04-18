use std::path::PathBuf;

use harborbeacon_local_agent::scripts::integration::IntegrationConfig;
use harborbeacon_local_agent::scripts::validate::run_validate;

#[derive(Debug, Clone)]
struct Cli {
    report: PathBuf,
    skip_live: bool,
    require_live: bool,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            report: PathBuf::from("validate-contract-report.json"),
            skip_live: false,
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
                "--report" => cli.report = PathBuf::from(take_value(&args, &mut index, "--report")),
                value if value.starts_with("--report=") => {
                    cli.report = PathBuf::from(value["--report=".len()..].to_string());
                }
                "--skip-live" => cli.skip_live = true,
                "--require-live" => cli.require_live = true,
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => fail(&format!("unknown flag: {value}")),
                value => fail(&format!("unexpected positional argument: {value}")),
            }
            index += 1;
        }

        if cli.skip_live && cli.require_live {
            fail("--skip-live and --require-live cannot be used together");
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
    eprintln!(
        "Usage: validate-contract-schemas [--report PATH] [--skip-live] [--require-live]"
    );
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
