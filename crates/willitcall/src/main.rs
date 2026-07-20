mod report;
mod site;

#[cfg(test)]
#[path = "../../wic-core/tests/support/mod.rs"]
mod support;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use wic_core::client::{
    parse_non_streaming, parse_sse_data, reassemble_sse_payloads, AssistantResponse,
};
use wic_core::result::{
    exit_code_for_totals, parse_and_validate_result, validate_result, write_result_atomic, Cause,
    CauseKind, PreflightOverride, RunResult, Status,
};
use wic_core::runner::{
    contention_preflight, preflight, run_scenarios, RunConfig, ServerConfig, ServerVersionProbe,
};
use wic_core::score::classify_failure;
use wic_core::{load_embedded_scenarios, load_scenarios_from_dir, ToolDefinition};

const EXIT_USAGE: u8 = 2;
const EXIT_PREFLIGHT: u8 = 3;
const EXIT_HARNESS: u8 = 4;
const EXIT_CODE_HELP: &str = "Exit codes:\n  0  all scenarios passed\n  1  at least one model-answer failure\n  2  usage or scenario configuration error\n  3  endpoint preflight failure\n  4  harness error during the run";
const KNOWN_INFERENCE_SERVERS: &[(u16, &str)] = &[
    (11434, "Ollama"),
    (8080, "llama.cpp"),
    (1234, "LM Studio"),
    (8000, "vLLM"),
];

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
    Annotate(AnnotateArgs),
    Rescore(RescoreArgs),
    Site(SiteArgs),
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
    #[arg(long)]
    force: bool,
    #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
    host_hardware_class: Option<String>,
    #[arg(long, value_parser = clap::builder::NonEmptyStringValueParser::new())]
    quant: Option<String>,
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

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("target")
        .required(true)
        .multiple(false)
        .args(["scenario", "all_empty"])
))]
struct AnnotateArgs {
    #[arg(long)]
    result: PathBuf,
    #[arg(long)]
    scenario: Option<String>,
    #[arg(long)]
    all_empty: bool,
    #[arg(long, value_enum)]
    cause: CauseArg,
    #[arg(long)]
    reference: Option<String>,
    #[arg(long)]
    note: Option<String>,
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CauseArg {
    ServerDefect,
    Unknown,
}

impl From<CauseArg> for CauseKind {
    fn from(value: CauseArg) -> Self {
        match value {
            CauseArg::ServerDefect => Self::ServerDefect,
            CauseArg::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Args)]
struct RescoreArgs {
    #[arg(long)]
    result: PathBuf,
}

#[derive(Debug, Args)]
struct SiteArgs {
    #[arg(long)]
    results: PathBuf,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "https://github.com/devYRPauli/willitcall")]
    repo_base: String,
}

#[derive(Debug)]
enum ExecuteError {
    Usage(String),
    Preflight(String),
    Harness(String),
}

fn read_result(path: &Path) -> Result<RunResult, ExecuteError> {
    let bytes = std::fs::read(path).map_err(|error| {
        ExecuteError::Usage(format!("failed to read result {}: {error}", path.display()))
    })?;
    parse_and_validate_result(&bytes).map_err(ExecuteError::Usage)
}

fn write_updated_result(path: &Path, result: &RunResult) -> Result<(), ExecuteError> {
    validate_result(result).map_err(ExecuteError::Usage)?;
    write_result_atomic(path, result).map_err(|error| {
        ExecuteError::Harness(format!(
            "failed to write result {}: {error}",
            path.display()
        ))
    })
}

fn annotate(args: AnnotateArgs) -> Result<usize, ExecuteError> {
    let mut result = read_result(&args.result)?;
    let cause = Cause {
        kind: args.cause.into(),
        reference: args.reference,
        note: args.note,
    };

    let count = if let Some(id) = args.scenario {
        let outcome = result
            .scenarios
            .iter_mut()
            .find(|outcome| outcome.id == id)
            .ok_or_else(|| ExecuteError::Usage(format!("scenario '{id}' was not found")))?;
        if outcome.failure_class.as_deref() != Some("empty_response") && !args.force {
            return Err(ExecuteError::Usage(format!(
                "scenario '{id}' is not an empty-response failure; use --force to annotate it"
            )));
        }
        outcome.cause = Some(cause);
        1
    } else {
        let mut count = 0;
        for outcome in &mut result.scenarios {
            if outcome.failure_class.as_deref() == Some("empty_response") {
                outcome.cause = Some(cause.clone());
                count += 1;
            }
        }
        count
    };

    if count > 0 {
        write_updated_result(&args.result, &result)?;
    }
    Ok(count)
}

fn parse_transcript(path: &Path) -> Result<(AssistantResponse, Vec<ToolDefinition>), String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read transcript {}: {error}", path.display()))?;
    let transcript: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("failed to parse transcript {}: {error}", path.display()))?;
    let final_turn = transcript
        .get("turns")
        .and_then(serde_json::Value::as_array)
        .and_then(|turns| turns.last())
        .ok_or_else(|| {
            format!(
                "failed to parse transcript {}: no final turn",
                path.display()
            )
        })?;
    let body = final_turn
        .get("response")
        .and_then(|response| response.get("body_raw"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "failed to parse transcript {}: final response has no body_raw",
                path.display()
            )
        })?;
    let tools = match final_turn.pointer("/request/body/tools") {
        None => Vec::new(),
        Some(tools) => tools
            .as_array()
            .ok_or_else(|| {
                format!(
                    "failed to parse transcript {}: request tools is not an array",
                    path.display()
                )
            })?
            .iter()
            .enumerate()
            .map(|(index, tool)| {
                let function = tool.get("function").cloned().ok_or_else(|| {
                    format!(
                        "failed to parse transcript {}: request tool {index} has no function",
                        path.display()
                    )
                })?;
                serde_json::from_value(function).map_err(|error| {
                    format!(
                        "failed to parse transcript {}: invalid request tool {index}: {error}",
                        path.display()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
    };

    let response = if body.trim_start().starts_with("data:") {
        let payloads = parse_sse_data(body.as_bytes())?;
        reassemble_sse_payloads(&payloads)
    } else {
        parse_non_streaming(body.as_bytes())
    }?;
    Ok((response, tools))
}

fn rescore(args: RescoreArgs) -> Result<(usize, Vec<String>), ExecuteError> {
    let mut result = read_result(&args.result)?;
    let result_parent = match args.result.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut changed = 0;
    let mut unparseable = Vec::new();

    for outcome in &mut result.scenarios {
        if outcome.status != Status::Fail || outcome.failure_class.is_some() {
            continue;
        }
        let Some(evidence_path) = outcome.evidence_path.as_deref() else {
            continue;
        };
        let path = result_parent.join(evidence_path);
        match parse_transcript(&path) {
            Ok((response, tools)) => {
                if let Some(failure_class) = classify_failure(
                    outcome.status,
                    &tools,
                    response.content.as_deref(),
                    &response.tool_calls,
                ) {
                    outcome.failure_class = Some(failure_class.to_owned());
                    changed += 1;
                }
            }
            Err(error) => unparseable.push(format!("{}: {error}", outcome.id)),
        }
    }

    if changed > 0 {
        write_updated_result(&args.result, &result)?;
    }
    Ok((changed, unparseable))
}

async fn execute(cli: Cli) -> Result<u8, ExecuteError> {
    execute_with_known_servers(cli, KNOWN_INFERENCE_SERVERS).await
}

async fn execute_with_known_servers(
    cli: Cli,
    known_servers: &[(u16, &str)],
) -> Result<u8, ExecuteError> {
    match cli.command {
        Command::Run(args) => {
            let scenarios = match args.scenarios {
                Some(path) => load_scenarios_from_dir(&path),
                None => load_embedded_scenarios(),
            }
            .map_err(|error| ExecuteError::Usage(error.to_string()))?;
            let endpoint =
                resolve_endpoint(args.server, args.endpoint).map_err(ExecuteError::Usage)?;
            let occupied = contention_preflight(&endpoint, known_servers)
                .await
                .map_err(ExecuteError::Preflight)?;
            if !occupied.is_empty() && !args.force {
                let endpoints = occupied
                    .iter()
                    .map(|endpoint| format!("{} ({})", endpoint.endpoint, endpoint.server))
                    .collect::<Vec<_>>()
                    .join(", ");
                let stop = if occupied.len() == 1 {
                    "stop it"
                } else {
                    "stop them"
                };
                return Err(ExecuteError::Preflight(format!(
                    "another inference server is responding on {endpoints}; {stop}, or re-run with --force"
                )));
            }
            let config = RunConfig::new(endpoint, args.model, Duration::from_secs(args.timeout))
                .with_server(args.server.config())
                .with_host_hardware_class(args.host_hardware_class)
                .with_declared_quant(args.quant);
            preflight(&config).await.map_err(ExecuteError::Preflight)?;
            let mut result = run_scenarios(&config, &scenarios, &args.out)
                .await
                .map_err(|error| {
                    ExecuteError::Harness(format!(
                        "failed to write evidence for {}: {error}",
                        args.out.display()
                    ))
                })?;
            if args.force && !occupied.is_empty() {
                result.metadata.preflight_override = Some(PreflightOverride {
                    forced: true,
                    foreign_endpoints: occupied
                        .into_iter()
                        .map(|endpoint| endpoint.endpoint)
                        .collect(),
                });
            }
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
            let is_directory = args.result_file.is_dir();
            let mut paths = if is_directory {
                std::fs::read_dir(&args.result_file)
                    .map_err(|error| {
                        ExecuteError::Usage(format!(
                            "failed to read results directory {}: {error}",
                            args.result_file.display()
                        ))
                    })?
                    .map(|entry| {
                        entry.map(|entry| entry.path()).map_err(|error| {
                            ExecuteError::Usage(format!(
                                "failed to read results directory entry: {error}"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                vec![args.result_file]
            };
            if is_directory {
                paths.retain(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "json")
                });
            }
            paths.sort();
            for path in paths {
                let bytes = std::fs::read(&path).map_err(|error| {
                    ExecuteError::Usage(format!(
                        "failed to read result {}: {error}",
                        path.display()
                    ))
                })?;
                parse_and_validate_result(&bytes).map_err(ExecuteError::Usage)?;
                println!("valid: {}", path.display());
            }
            Ok(0)
        }
        Command::Annotate(args) => {
            let count = annotate(args)?;
            println!(
                "annotated {count} scenario{}",
                if count == 1 { "" } else { "s" }
            );
            Ok(0)
        }
        Command::Rescore(args) => {
            let result_path = args.result.clone();
            let (changed, unparseable) = rescore(args)?;
            for error in unparseable {
                eprintln!("warning: could not parse response for {error}");
            }
            println!(
                "rescored {}: {changed} scenario{} changed",
                result_path.display(),
                if changed == 1 { "" } else { "s" }
            );
            Ok(0)
        }
        Command::Site(args) => {
            let count = site::generate(&args.results, &args.out, &args.repo_base)
                .map_err(ExecuteError::Harness)?;
            println!(
                "generated {} from {count} result file{}",
                args.out.display(),
                if count == 1 { "" } else { "s" }
            );
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

    use super::{
        execute_with_known_servers, resolve_endpoint, Cli, Command, ExecuteError, ServerPreset,
    };

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
            "--force",
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
        assert!(args.force);
    }

    #[test]
    fn run_subcommand_parses_host_hardware_class_override() {
        let cli = Cli::try_parse_from([
            "willitcall",
            "run",
            "--endpoint",
            "http://127.0.0.1:8080/v1",
            "--model",
            "local-model",
            "--host-hardware-class",
            "Contributor workstation, 32GB",
        ])
        .expect("run arguments should parse");

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(
            args.host_hardware_class.as_deref(),
            Some("Contributor workstation, 32GB")
        );
    }

    #[test]
    fn run_subcommand_parses_quant_declaration() {
        let cli = Cli::try_parse_from([
            "willitcall",
            "run",
            "--endpoint",
            "http://127.0.0.1:8080/v1",
            "--model",
            "local-model",
            "--quant",
            "Q4_K_M-imatrix",
        ])
        .expect("run arguments should parse");

        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.quant.as_deref(), Some("Q4_K_M-imatrix"));
    }

    #[tokio::test]
    async fn contention_refusal_is_exit_three_and_writes_no_result() {
        let target = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind target listener");
        let target_port = target.local_addr().expect("target address").port();
        let foreign = crate::support::MockServer::start().await;
        let foreign_port = foreign.port();
        let directory = tempfile::tempdir().expect("temp directory");
        let output_path = directory.path().join("result.json");
        let cli = Cli::try_parse_from([
            "willitcall".to_owned(),
            "run".to_owned(),
            "--endpoint".to_owned(),
            format!("http://127.0.0.1:{target_port}/v1"),
            "--model".to_owned(),
            "fixture-model".to_owned(),
            "--out".to_owned(),
            output_path.display().to_string(),
        ])
        .expect("run arguments should parse");

        let error = execute_with_known_servers(cli, &[(foreign_port, "Test server")])
            .await
            .expect_err("foreign listener should refuse the run");

        assert!(matches!(error, ExecuteError::Preflight(_)));
        assert_eq!(super::EXIT_PREFLIGHT, 3);
        let ExecuteError::Preflight(message) = error else {
            unreachable!("checked preflight error")
        };
        assert!(message.contains(&format!("127.0.0.1:{foreign_port}")));
        assert!(message.contains("Test server"));
        assert!(message.contains("stop it"));
        assert!(message.contains("--force"));
        assert!(!output_path.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn force_records_the_contention_override_in_the_result() {
        let server = crate::support::MockServer::start().await;
        let foreign = crate::support::MockServer::start().await;
        let foreign_port = foreign.port();
        let directory = tempfile::tempdir().expect("temp directory");
        let scenario_path = directory.path().join("scenarios");
        std::fs::create_dir(&scenario_path).expect("scenario directory");
        std::fs::write(
            scenario_path.join("single-weather.toml"),
            include_str!("../../wic-core/scenarios/single-weather.toml"),
        )
        .expect("write scenario");
        let output_path = directory.path().join("result.json");
        let cli = Cli::try_parse_from([
            "willitcall".to_owned(),
            "run".to_owned(),
            "--endpoint".to_owned(),
            server.endpoint(),
            "--model".to_owned(),
            "fixture-model".to_owned(),
            "--scenarios".to_owned(),
            scenario_path.display().to_string(),
            "--out".to_owned(),
            output_path.display().to_string(),
            "--force".to_owned(),
        ])
        .expect("run arguments should parse");

        let code = execute_with_known_servers(cli, &[(foreign_port, "Test server")])
            .await
            .expect("force should allow the run");

        assert_eq!(code, 0);
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output_path).expect("result file"))
                .expect("valid result JSON");
        assert_eq!(document["metadata"]["preflight_override"]["forced"], true);
        assert_eq!(
            document["metadata"]["preflight_override"]["foreign_endpoints"],
            serde_json::json!([format!("127.0.0.1:{foreign_port}")])
        );
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
                environment: None,
                sampling: SamplingParams {
                    temperature: Some(0.0),
                    top_p: Some(1.0),
                    seed: Some(42),
                    max_tokens: Some(1024),
                },
                preflight_override: None,
            },
            scenarios: vec![
                ScenarioOutcome {
                    id: "single-ok".to_owned(),
                    category: ScenarioCategory::SingleCall,
                    status: Status::Pass,
                    failure_reason: None,
                    failure_class: None,
                    cause: None,
                    evidence_hash: None,
                    evidence_path: None,
                    retried: false,
                },
                ScenarioOutcome {
                    id: "single-bad".to_owned(),
                    category: ScenarioCategory::SingleCall,
                    status: Status::Fail,
                    failure_reason: Some("wrong tool call: expected 'a', got 'b'".to_owned()),
                    failure_class: None,
                    cause: None,
                    evidence_hash: None,
                    evidence_path: None,
                    retried: false,
                },
                ScenarioOutcome {
                    id: "stream-error".to_owned(),
                    category: ScenarioCategory::Streaming,
                    status: Status::Error,
                    failure_reason: Some("request timed out after 60s".to_owned()),
                    failure_class: None,
                    cause: None,
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
                environment: None,
                sampling: SamplingParams {
                    temperature: None,
                    top_p: None,
                    seed: None,
                    max_tokens: None,
                },
                preflight_override: None,
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
