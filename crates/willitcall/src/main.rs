mod report;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use wic_core::result::{exit_code_for_totals, parse_and_validate_result, write_result_atomic};
use wic_core::runner::{preflight, run_scenarios, RunConfig, ServerConfig, ServerVersionProbe};
use wic_core::{load_embedded_scenarios, load_scenarios_from_dir};

const EXIT_USAGE: u8 = 2;
const EXIT_PREFLIGHT: u8 = 3;
const EXIT_HARNESS: u8 = 4;
const EXIT_CODE_HELP: &str = "Exit codes:\n  0  all scenarios passed\n  1  at least one model-answer failure\n  2  usage or scenario configuration error\n  3  endpoint preflight failure\n  4  harness error during the run";

#[derive(Debug, Parser)]
#[command(name = "willitcall", after_long_help = EXIT_CODE_HELP)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(RunArgs),
    Scenarios(ScenariosArgs),
    Validate(ValidateArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    #[arg(long)]
    endpoint: Option<String>,
    #[arg(long)]
    model: String,
    #[arg(long, value_enum, default_value_t = ServerPreset::Custom)]
    server: ServerPreset,
    #[arg(long)]
    scenarios: Option<PathBuf>,
    #[arg(long, default_value = "willitcall-result.json")]
    out: PathBuf,
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ServerPreset {
    Llamacpp,
    Ollama,
    Lmstudio,
    Vllm,
    Custom,
}

impl ServerPreset {
    fn name(self) -> &'static str {
        match self {
            Self::Llamacpp => "llamacpp",
            Self::Ollama => "ollama",
            Self::Lmstudio => "lmstudio",
            Self::Vllm => "vllm",
            Self::Custom => "custom",
        }
    }

    fn default_endpoint(self) -> Option<&'static str> {
        match self {
            Self::Llamacpp => Some("http://127.0.0.1:8080/v1"),
            Self::Ollama => Some("http://127.0.0.1:11434/v1"),
            Self::Lmstudio => Some("http://127.0.0.1:1234/v1"),
            Self::Vllm => Some("http://127.0.0.1:8000/v1"),
            Self::Custom => None,
        }
    }

    fn version_probe(self) -> Option<ServerVersionProbe> {
        match self {
            Self::Llamacpp => Some(ServerVersionProbe {
                path: "/props",
                field: "build_info",
            }),
            Self::Ollama => Some(ServerVersionProbe {
                path: "/api/version",
                field: "version",
            }),
            Self::Vllm => Some(ServerVersionProbe {
                path: "/version",
                field: "version",
            }),
            Self::Lmstudio | Self::Custom => None,
        }
    }

    fn config(self) -> ServerConfig {
        ServerConfig {
            preset_name: self.name().to_owned(),
            quirk_flags: Vec::new(),
            version_probe: self.version_probe(),
        }
    }
}

fn resolve_endpoint(preset: ServerPreset, endpoint: Option<String>) -> Result<String, String> {
    endpoint
        .or_else(|| preset.default_endpoint().map(str::to_owned))
        .ok_or_else(|| "--endpoint is required when --server custom is selected".to_owned())
}

#[derive(Debug, Args)]
struct ScenariosArgs {
    #[command(subcommand)]
    command: ScenariosCommand,
}

#[derive(Debug, Subcommand)]
enum ScenariosCommand {
    List(ListArgs),
}

#[derive(Debug, Args)]
struct ListArgs {
    #[arg(long)]
    scenarios: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ValidateArgs {
    result_file: PathBuf,
}

#[derive(Debug)]
enum ExecuteError {
    Usage(String),
    Preflight(String),
    Harness(String),
}

async fn execute(cli: Cli) -> Result<u8, ExecuteError> {
    match cli.command {
        Command::Run(args) => {
            let scenarios = match args.scenarios {
                Some(path) => load_scenarios_from_dir(&path),
                None => load_embedded_scenarios(),
            }
            .map_err(|error| ExecuteError::Usage(error.to_string()))?;
            let endpoint =
                resolve_endpoint(args.server, args.endpoint).map_err(ExecuteError::Usage)?;
            let config = RunConfig::new(endpoint, args.model, Duration::from_secs(args.timeout))
                .with_server(args.server.config());
            preflight(&config).await.map_err(ExecuteError::Preflight)?;
            let result = run_scenarios(&config, &scenarios, &args.out)
                .await
                .map_err(|error| {
                    ExecuteError::Harness(format!(
                        "failed to write evidence for {}: {error}",
                        args.out.display()
                    ))
                })?;
            write_result_atomic(&args.out, &result).map_err(|error| {
                ExecuteError::Harness(format!(
                    "failed to write result {}: {error}",
                    args.out.display()
                ))
            })?;
            if args.json {
                let document = serde_json::to_string_pretty(&result).map_err(|error| {
                    ExecuteError::Harness(format!("failed to serialize result: {error}"))
                })?;
                println!("{document}");
            } else {
                let color =
                    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
                print!("{}", report::render_report(&result, color));
            }
            Ok(exit_code_for_totals(&result.totals))
        }
        Command::Scenarios(args) => match args.command {
            ScenariosCommand::List(args) => {
                let scenarios = match args.scenarios {
                    Some(path) => load_scenarios_from_dir(&path),
                    None => load_embedded_scenarios(),
                }
                .map_err(|error| ExecuteError::Usage(error.to_string()))?;
                for scenario in scenarios {
                    println!("{}\t{}", scenario.id, scenario.category);
                }
                Ok(0)
            }
        },
        Command::Validate(args) => {
            let bytes = std::fs::read(&args.result_file).map_err(|error| {
                ExecuteError::Usage(format!(
                    "failed to read result {}: {error}",
                    args.result_file.display()
                ))
            })?;
            parse_and_validate_result(&bytes).map_err(ExecuteError::Usage)?;
            println!("valid: {}", args.result_file.display());
            Ok(0)
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(exit_code as u8);
        }
    };
    match execute(cli).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let (code, message) = match error {
                ExecuteError::Usage(message) => (EXIT_USAGE, message),
                ExecuteError::Preflight(message) => {
                    (EXIT_PREFLIGHT, format!("preflight failed: {message}"))
                }
                ExecuteError::Harness(message) => (EXIT_HARNESS, message),
            };
            eprintln!("error: {message}");
            ExitCode::from(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;
    use wic_core::result::{
        RunMetadata, RunResult, SamplingParams, ScenarioOutcome, ServerMetadata, Status, Totals,
    };
    use wic_core::ScenarioCategory;

    use super::{resolve_endpoint, Cli, Command, ServerPreset};

    #[test]
    fn run_subcommand_parses_m0_arguments() {
        let cli = Cli::try_parse_from([
            "willitcall",
            "run",
            "--endpoint",
            "http://127.0.0.1:8080/v1",
            "--model",
            "local-model",
            "--scenarios",
            "corpus",
            "--out",
            "result.json",
            "--json",
        ])
        .expect("run arguments should parse");

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.endpoint.as_deref(), Some("http://127.0.0.1:8080/v1"));
        assert_eq!(args.model, "local-model");
        assert_eq!(args.scenarios, Some(PathBuf::from("corpus")));
        assert_eq!(args.out, PathBuf::from("result.json"));
        assert_eq!(args.timeout, 60);
        assert!(args.json);
    }

    #[test]
    fn run_defaults_output_and_timeout() {
        let cli = Cli::try_parse_from([
            "willitcall",
            "run",
            "--endpoint",
            "http://127.0.0.1:8080/v1",
            "--model",
            "local-model",
        ])
        .expect("run arguments should parse");
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.out, PathBuf::from("willitcall-result.json"));
        assert_eq!(args.timeout, 60);
        assert_eq!(args.server, ServerPreset::Custom);
    }

    #[test]
    fn server_presets_select_defaults_and_endpoint_overrides_them() {
        assert_eq!(
            resolve_endpoint(ServerPreset::Llamacpp, None).expect("llamacpp endpoint"),
            "http://127.0.0.1:8080/v1"
        );
        assert_eq!(
            resolve_endpoint(ServerPreset::Ollama, None).expect("ollama endpoint"),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            resolve_endpoint(ServerPreset::Lmstudio, None).expect("lmstudio endpoint"),
            "http://127.0.0.1:1234/v1"
        );
        assert_eq!(
            resolve_endpoint(ServerPreset::Vllm, None).expect("vllm endpoint"),
            "http://127.0.0.1:8000/v1"
        );
        assert!(resolve_endpoint(ServerPreset::Custom, None).is_err());
        assert_eq!(
            resolve_endpoint(
                ServerPreset::Ollama,
                Some("http://example.test/v1".to_owned())
            )
            .expect("endpoint override"),
            "http://example.test/v1"
        );
    }

    #[test]
    fn report_groups_categories_and_prints_precise_failures() {
        let result = RunResult {
            schema_version: 1,
            metadata: RunMetadata {
                run_id: String::new(),
                timestamp: "2026-07-19T12:00:00Z".to_owned(),
                willitcall_version: "0.1.0".to_owned(),
                endpoint: "http://127.0.0.1:8080/v1".to_owned(),
                model_id: "fixture-model".to_owned(),
                declared_quant: None,
                server: ServerMetadata {
                    preset_name: "custom".to_owned(),
                    reported_version: None,
                    quirk_flags: Vec::new(),
                },
                sampling: SamplingParams {
                    temperature: Some(0.0),
                    top_p: Some(1.0),
                    seed: Some(42),
                    max_tokens: Some(1024),
                },
            },
            scenarios: vec![
                ScenarioOutcome {
                    id: "single-ok".to_owned(),
                    category: ScenarioCategory::SingleCall,
                    status: Status::Pass,
                    failure_reason: None,
                    evidence_hash: None,
                    evidence_path: None,
                    retried: false,
                },
                ScenarioOutcome {
                    id: "single-bad".to_owned(),
                    category: ScenarioCategory::SingleCall,
                    status: Status::Fail,
                    failure_reason: Some("wrong tool call: expected 'a', got 'b'".to_owned()),
                    evidence_hash: None,
                    evidence_path: None,
                    retried: false,
                },
                ScenarioOutcome {
                    id: "stream-error".to_owned(),
                    category: ScenarioCategory::Streaming,
                    status: Status::Error,
                    failure_reason: Some("request timed out after 60s".to_owned()),
                    evidence_hash: None,
                    evidence_path: None,
                    retried: true,
                },
            ],
            totals: Totals {
                total: 3,
                passed: 1,
                failed: 1,
                errors: 1,
                skipped: 0,
            },
        };

        let rendered = super::report::render_report(&result, false);

        assert!(rendered.contains("single_call       1 passed  1 failed  0 errors"));
        assert!(rendered.contains("streaming         0 passed  0 failed  1 errors"));
        assert!(rendered.contains("FAIL  single-bad: wrong tool call: expected 'a', got 'b'"));
        assert!(rendered.contains("ERROR stream-error: request timed out after 60s"));
        assert!(rendered.ends_with("TOTAL 1 passed  1 failed  1 errors  0 skipped  3 total\n"));
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn color_report_uses_ansi_only_when_requested() {
        let result = RunResult {
            schema_version: 1,
            metadata: RunMetadata {
                run_id: String::new(),
                timestamp: String::new(),
                willitcall_version: String::new(),
                endpoint: String::new(),
                model_id: String::new(),
                declared_quant: None,
                server: ServerMetadata {
                    preset_name: "custom".to_owned(),
                    reported_version: None,
                    quirk_flags: Vec::new(),
                },
                sampling: SamplingParams {
                    temperature: None,
                    top_p: None,
                    seed: None,
                    max_tokens: None,
                },
            },
            scenarios: Vec::new(),
            totals: Totals {
                total: 0,
                passed: 0,
                failed: 0,
                errors: 0,
                skipped: 0,
            },
        };

        assert!(super::report::render_report(&result, true).contains('\u{1b}'));
        assert!(!super::report::render_report(&result, false).contains('\u{1b}'));
    }

    #[test]
    fn validate_subcommand_parses_a_result_path() {
        let cli = Cli::try_parse_from(["willitcall", "validate", "result.json"])
            .expect("validate arguments should parse");
        let Command::Validate(args) = cli.command else {
            panic!("expected validate command");
        };
        assert_eq!(args.result_file, PathBuf::from("result.json"));
    }

    #[test]
    fn help_documents_all_exit_codes() {
        let help = Cli::try_parse_from(["willitcall", "--help"])
            .expect_err("help exits through clap")
            .to_string();
        for code in 0..=4 {
            assert!(
                help.contains(&format!("{code}  ")),
                "missing exit code {code}"
            );
        }
    }
}
