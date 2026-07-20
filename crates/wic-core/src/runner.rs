use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use ring::digest::{digest, SHA256};
use serde_json::{json, Map, Value};

use crate::client::{AssistantResponse, CompletionResult, EndpointClient, ToolCall};
use crate::result::{
    redact_transcript_turn, write_transcript_atomic, CapturedTurn, EnvironmentMetadata,
    RunMetadata, RunResult, SamplingParams, ScenarioOutcome, ServerMetadata, Status, Totals,
    Transcript, RESULT_SCHEMA_VERSION,
};
use crate::score::score_response;
use crate::{Message, MessageRole, Scenario};

#[derive(Clone, Debug)]
pub struct RunConfig {
    pub endpoint: String,
    pub model: String,
    pub timeout: Duration,
    pub sampling: SamplingParams,
    pub server: ServerConfig,
    pub environment: EnvironmentMetadata,
    pub request_headers: HeaderMap,
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub preset_name: String,
    pub quirk_flags: Vec<String>,
    pub version_probe: Option<ServerVersionProbe>,
}

#[derive(Clone, Copy, Debug)]
pub struct ServerVersionProbe {
    pub path: &'static str,
    pub field: &'static str,
}

impl RunConfig {
    pub fn new(endpoint: String, model: String, timeout: Duration) -> Self {
        Self {
            endpoint,
            model,
            timeout,
            sampling: SamplingParams {
                temperature: Some(0.0),
                top_p: Some(1.0),
                seed: Some(42),
                max_tokens: Some(1024),
            },
            server: ServerConfig {
                preset_name: "custom".to_owned(),
                quirk_flags: Vec::new(),
                version_probe: None,
            },
            environment: detect_environment(),
            request_headers: HeaderMap::new(),
        }
    }

    pub fn with_server(mut self, server: ServerConfig) -> Self {
        self.server = server;
        self
    }

    pub fn with_host_hardware_class(mut self, host_hardware_class: Option<String>) -> Self {
        if let Some(host_hardware_class) = host_hardware_class {
            self.environment.host_hardware_class = host_hardware_class;
        }
        self
    }
}

fn detect_environment() -> EnvironmentMetadata {
    EnvironmentMetadata {
        host_hardware_class: detect_host_hardware_class(),
        host_os: detect_host_os(),
    }
}

fn detect_host_hardware_class() -> String {
    #[cfg(target_os = "macos")]
    {
        let cpu = command_value("sysctl", &["-n", "machdep.cpu.brand_string"]);
        let memory_gb = command_value("sysctl", &["-n", "hw.memsize"])
            .and_then(|value| value.parse::<u64>().ok())
            .map(|bytes| bytes / 1024 / 1024 / 1024)
            .filter(|gigabytes| *gigabytes > 0);
        if let (Some(cpu), Some(memory_gb)) = (cpu, memory_gb) {
            return format!("{cpu}, {memory_gb}GB");
        }
    }

    format!("{} {} host", std::env::consts::OS, std::env::consts::ARCH)
}

fn detect_host_os() -> String {
    #[cfg(target_os = "macos")]
    if let (Some(name), Some(version)) = (
        command_value("sw_vers", &["-productName"]),
        command_value("sw_vers", &["-productVersion"]),
    ) {
        return format!("{name} {version}");
    }

    if let (Some(name), Some(version)) = (
        command_value("uname", &["-s"]),
        command_value("uname", &["-r"]),
    ) {
        return format!("{name} {version}");
    }

    format!("{} (version unavailable)", std::env::consts::OS)
}

fn command_value(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub async fn preflight(config: &RunConfig) -> Result<(), String> {
    client(config).preflight().await
}

#[derive(Debug)]
pub struct OccupiedEndpoint {
    pub endpoint: String,
    pub server: String,
}

pub async fn contention_preflight(
    endpoint: &str,
    known_servers: &[(u16, &str)],
) -> Result<Vec<OccupiedEndpoint>, String> {
    const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);

    let url = reqwest::Url::parse(endpoint)
        .map_err(|error| format!("invalid target endpoint: {error}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "invalid target endpoint: missing host".to_owned())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "invalid target endpoint: missing port".to_owned())?;
    let target_addresses =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| "timed out resolving target endpoint".to_owned())?
            .map_err(|error| format!("failed to resolve target endpoint: {error}"))?
            .collect::<Vec<_>>();

    let mut probes = known_servers
        .iter()
        .map(|(port, server)| {
            (
                SocketAddr::from((Ipv4Addr::LOCALHOST, *port)),
                Some(*server),
            )
        })
        .collect::<Vec<_>>();
    for address in &target_addresses {
        if !probes.iter().any(|(candidate, _)| candidate == address) {
            probes.push((*address, None));
        }
    }

    let mut occupied = Vec::new();
    for (address, server) in probes {
        let responding =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(address))
                .await
                .is_ok_and(|result| result.is_ok());
        if responding && !target_addresses.contains(&address) {
            if let Some(server) = server {
                occupied.push(OccupiedEndpoint {
                    endpoint: address.to_string(),
                    server: server.to_owned(),
                });
            }
        }
    }
    Ok(occupied)
}

pub async fn run_scenarios(
    config: &RunConfig,
    scenarios: &[Scenario],
    result_path: &Path,
) -> io::Result<RunResult> {
    let endpoint_client = client(config);
    let reported_version = match config.server.version_probe {
        Some(probe) => {
            endpoint_client
                .server_version(probe.path, probe.field)
                .await
        }
        None => None,
    };
    let mut ordered = scenarios.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    let timestamp = utc_timestamp();
    let run_id = run_id(&timestamp, &config.endpoint, &config.model);
    let result_parent = match result_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut outcomes = Vec::with_capacity(ordered.len());
    for scenario in ordered {
        outcomes.push(run_scenario(&endpoint_client, scenario, &run_id, result_parent).await?);
    }

    let totals = totals(&outcomes);
    Ok(RunResult {
        schema_version: RESULT_SCHEMA_VERSION,
        metadata: RunMetadata {
            run_id,
            timestamp,
            willitcall_version: env!("CARGO_PKG_VERSION").to_owned(),
            endpoint: config.endpoint.clone(),
            model_id: config.model.clone(),
            declared_quant: None,
            server: ServerMetadata {
                preset_name: config.server.preset_name.clone(),
                reported_version,
                quirk_flags: config.server.quirk_flags.clone(),
            },
            environment: Some(config.environment.clone()),
            sampling: config.sampling.clone(),
            preflight_override: None,
        },
        scenarios: outcomes,
        totals,
    })
}

fn client(config: &RunConfig) -> EndpointClient {
    EndpointClient::new_with_headers(
        config.endpoint.clone(),
        config.model.clone(),
        config.timeout,
        config.sampling.clone(),
        config.request_headers.clone(),
    )
}

async fn run_scenario(
    client: &EndpointClient,
    scenario: &Scenario,
    run_id: &str,
    result_parent: &Path,
) -> io::Result<ScenarioOutcome> {
    let mut messages = Vec::new();
    let mut previous_calls = Vec::new();
    let mut evidence = Vec::new();
    let mut retried = false;

    for (turn_index, turn) in scenario.turns.iter().enumerate() {
        for message in &turn.messages {
            match request_message(message, &previous_calls) {
                Ok(message) => messages.push(message),
                Err(reason) => {
                    return outcome(
                        scenario,
                        Status::Fail,
                        Some(turn_reason(scenario, turn_index, reason)),
                        &evidence,
                        retried,
                        run_id,
                        result_parent,
                    );
                }
            }
        }

        let completion = client.complete(scenario, &messages).await;
        let response = match completion {
            CompletionResult::Parsed {
                response,
                raw_bytes: _,
                turns,
                retried: completion_retried,
            } => {
                evidence.extend(turns);
                retried |= completion_retried;
                response
            }
            CompletionResult::Invalid {
                reason,
                raw_bytes: _,
                turns,
                retried: completion_retried,
            } => {
                evidence.extend(turns);
                retried |= completion_retried;
                return outcome(
                    scenario,
                    Status::Fail,
                    Some(turn_reason(scenario, turn_index, reason)),
                    &evidence,
                    retried,
                    run_id,
                    result_parent,
                );
            }
            CompletionResult::Error {
                reason,
                raw_bytes: _,
                turns,
                retried: completion_retried,
            } => {
                evidence.extend(turns);
                retried |= completion_retried;
                return outcome(
                    scenario,
                    Status::Error,
                    Some(turn_reason(scenario, turn_index, reason)),
                    &evidence,
                    retried,
                    run_id,
                    result_parent,
                );
            }
        };

        if let Err(failure) = score_response(
            &scenario.tools,
            &turn.expected_calls,
            scenario.arguments_match,
            response.content.as_deref(),
            &response.tool_calls,
        ) {
            let mut result = outcome(
                scenario,
                Status::Fail,
                Some(turn_reason(scenario, turn_index, failure.reason)),
                &evidence,
                retried,
                run_id,
                result_parent,
            )?;
            result.failure_class = failure.failure_class;
            return Ok(result);
        }

        previous_calls = response.tool_calls.clone();
        messages.push(assistant_message(&response));
    }

    outcome(
        scenario,
        Status::Pass,
        None,
        &evidence,
        retried,
        run_id,
        result_parent,
    )
}

fn request_message(message: &Message, previous_calls: &[ToolCall]) -> Result<Value, String> {
    let role = match message.role {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => {
            let reference = message
                .tool_call_ref
                .ok_or_else(|| "tool message is missing tool_call_ref".to_owned())?;
            let call = previous_calls.get(reference).ok_or_else(|| {
                format!("tool_call_ref {reference} has no model tool call to reference")
            })?;
            let id = call
                .id
                .as_deref()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| {
                    format!("tool call {reference} is missing an id required by tool_call_ref")
                })?;
            return Ok(json!({
                "role": "tool",
                "content": message.content,
                "tool_call_id": id,
            }));
        }
    };
    Ok(json!({"role": role, "content": message.content}))
}

fn assistant_message(response: &AssistantResponse) -> Value {
    let calls = response
        .tool_calls
        .iter()
        .map(|call| {
            let mut call_value = Map::from_iter([
                ("type".to_owned(), json!("function")),
                (
                    "function".to_owned(),
                    json!({"name": call.name, "arguments": call.arguments}),
                ),
            ]);
            if let Some(id) = &call.id {
                call_value.insert("id".to_owned(), json!(id));
            }
            Value::Object(call_value)
        })
        .collect::<Vec<_>>();
    json!({
        "role": "assistant",
        "content": response.content,
        "tool_calls": calls,
    })
}

fn turn_reason(scenario: &Scenario, turn_index: usize, reason: String) -> String {
    if scenario.turns.len() > 1 {
        format!("turn {}: {reason}", turn_index + 1)
    } else {
        reason
    }
}

fn outcome(
    scenario: &Scenario,
    status: Status,
    failure_reason: Option<String>,
    captured_turns: &[CapturedTurn],
    retried: bool,
    run_id: &str,
    result_parent: &Path,
) -> io::Result<ScenarioOutcome> {
    let (evidence_hash, evidence_path) = if captured_turns.is_empty() {
        (None, None)
    } else {
        let relative_path = format!("evidence/{run_id}/{}.json", scenario.id);
        let path = result_parent.join(&relative_path);
        fs::create_dir_all(path.parent().expect("evidence path has a parent"))?;
        let transcript = Transcript {
            schema_version: 1,
            run_id: run_id.to_owned(),
            scenario_id: scenario.id.clone(),
            turns: captured_turns
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, turn)| redact_transcript_turn(index, turn))
                .collect(),
        };
        let bytes = write_transcript_atomic(&path, &transcript)?;
        (Some(evidence_hash(&bytes)), Some(relative_path))
    };
    Ok(ScenarioOutcome {
        id: scenario.id.clone(),
        category: scenario.category,
        status,
        failure_reason,
        failure_class: None,
        cause: None,
        evidence_hash,
        evidence_path,
        retried,
    })
}

fn totals(outcomes: &[ScenarioOutcome]) -> Totals {
    let mut totals = Totals {
        total: outcomes.len() as u32,
        passed: 0,
        failed: 0,
        errors: 0,
        skipped: 0,
    };
    for outcome in outcomes {
        match outcome.status {
            Status::Pass => totals.passed += 1,
            Status::Fail => totals.failed += 1,
            Status::Error => totals.errors += 1,
            Status::Skipped => totals.skipped += 1,
        }
    }
    totals
}

fn evidence_hash(transcript_bytes: &[u8]) -> String {
    let hash = digest(&SHA256, transcript_bytes);
    format!("sha256:{}", hex(hash.as_ref()))
}

fn run_id(timestamp: &str, endpoint: &str, model_id: &str) -> String {
    let compact_timestamp = timestamp.replace(['-', ':'], "");
    let source = format!("{timestamp}\n{endpoint}\n{model_id}");
    let hash = digest(&SHA256, source.as_bytes());
    format!("{compact_timestamp}-{}", &hex(hash.as_ref())[..8])
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;

    #[test]
    fn run_config_collects_environment_and_accepts_hardware_override() {
        let config = super::RunConfig::new(
            "http://127.0.0.1:8080/v1".to_owned(),
            "fixture-model".to_owned(),
            std::time::Duration::from_secs(60),
        )
        .with_host_hardware_class(Some("Fixture workstation, 32GB".to_owned()));

        assert_eq!(
            config.environment.host_hardware_class,
            "Fixture workstation, 32GB"
        );
        assert!(!config.environment.host_os.is_empty());
    }

    async fn unused_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral listener");
        listener.local_addr().expect("listener address").port()
    }

    #[tokio::test]
    async fn contention_probe_all_clear_proceeds() {
        let port = unused_port().await;

        let occupied =
            super::contention_preflight("http://127.0.0.1:65535/v1", &[(port, "Test server")])
                .await
                .expect("probe succeeds");

        assert!(occupied.is_empty());
    }

    #[tokio::test]
    async fn contention_probe_ignores_the_responding_target_port() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind target listener");
        let port = listener.local_addr().expect("listener address").port();

        let occupied = super::contention_preflight(
            &format!("http://127.0.0.1:{port}/v1"),
            &[(port, "Test server")],
        )
        .await
        .expect("probe succeeds");

        assert!(occupied.is_empty());
    }

    #[tokio::test]
    async fn contention_probe_reports_a_responding_foreign_port() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind foreign listener");
        let port = listener.local_addr().expect("listener address").port();

        let occupied =
            super::contention_preflight("http://127.0.0.1:65535/v1", &[(port, "Test server")])
                .await
                .expect("probe succeeds");

        assert_eq!(occupied.len(), 1);
        assert_eq!(occupied[0].endpoint, format!("127.0.0.1:{port}"));
        assert_eq!(occupied[0].server, "Test server");
    }

    #[test]
    fn run_id_uses_compact_timestamp_and_metadata_hash_prefix() {
        assert_eq!(
            super::run_id(
                "2026-07-19T20:45:00Z",
                "http://localhost:11434/v1",
                "qwen2.5:7b-instruct",
            ),
            "20260719T204500Z-beda7dcb"
        );
    }
}
