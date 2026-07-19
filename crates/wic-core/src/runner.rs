use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ring::digest::{digest, SHA256};
use serde_json::{json, Map, Value};

use crate::client::{AssistantResponse, CompletionResult, EndpointClient, ToolCall};
use crate::result::{
    RunMetadata, RunResult, SamplingParams, ScenarioOutcome, ServerMetadata, Status, Totals,
    RESULT_SCHEMA_VERSION,
};
use crate::score::score_calls;
use crate::{Message, MessageRole, Scenario};

#[derive(Clone, Debug)]
pub struct RunConfig {
    pub endpoint: String,
    pub model: String,
    pub timeout: Duration,
    pub sampling: SamplingParams,
    pub server: ServerConfig,
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
        }
    }

    pub fn with_server(mut self, server: ServerConfig) -> Self {
        self.server = server;
        self
    }
}

pub async fn preflight(config: &RunConfig) -> Result<(), String> {
    client(config).preflight().await
}

pub async fn run_scenarios(config: &RunConfig, scenarios: &[Scenario]) -> RunResult {
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
    let mut outcomes = Vec::with_capacity(ordered.len());
    for scenario in ordered {
        outcomes.push(run_scenario(&endpoint_client, scenario).await);
    }

    let totals = totals(&outcomes);
    RunResult {
        schema_version: RESULT_SCHEMA_VERSION,
        metadata: RunMetadata {
            timestamp: utc_timestamp(),
            willitcall_version: env!("CARGO_PKG_VERSION").to_owned(),
            endpoint: config.endpoint.clone(),
            model_id: config.model.clone(),
            declared_quant: None,
            server: ServerMetadata {
                preset_name: config.server.preset_name.clone(),
                reported_version,
                quirk_flags: config.server.quirk_flags.clone(),
            },
            sampling: config.sampling.clone(),
        },
        scenarios: outcomes,
        totals,
    }
}

fn client(config: &RunConfig) -> EndpointClient {
    EndpointClient::new(
        config.endpoint.clone(),
        config.model.clone(),
        config.timeout,
        config.sampling.clone(),
    )
}

async fn run_scenario(client: &EndpointClient, scenario: &Scenario) -> ScenarioOutcome {
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
                    );
                }
            }
        }

        let completion = client.complete(scenario, &messages).await;
        let response = match completion {
            CompletionResult::Parsed {
                response,
                raw_bytes,
                retried: completion_retried,
            } => {
                evidence.extend_from_slice(&raw_bytes);
                retried |= completion_retried;
                response
            }
            CompletionResult::Invalid {
                reason,
                raw_bytes,
                retried: completion_retried,
            } => {
                evidence.extend_from_slice(&raw_bytes);
                retried |= completion_retried;
                return outcome(
                    scenario,
                    Status::Fail,
                    Some(turn_reason(scenario, turn_index, reason)),
                    &evidence,
                    retried,
                );
            }
            CompletionResult::Error {
                reason,
                raw_bytes,
                retried: completion_retried,
            } => {
                evidence.extend_from_slice(&raw_bytes);
                retried |= completion_retried;
                return outcome(
                    scenario,
                    Status::Error,
                    Some(turn_reason(scenario, turn_index, reason)),
                    &evidence,
                    retried,
                );
            }
        };

        if let Err(reason) = score_calls(
            &scenario.tools,
            &turn.expected_calls,
            scenario.arguments_match,
            &response.tool_calls,
        ) {
            return outcome(
                scenario,
                Status::Fail,
                Some(turn_reason(scenario, turn_index, reason)),
                &evidence,
                retried,
            );
        }

        previous_calls = response.tool_calls.clone();
        messages.push(assistant_message(&response));
    }

    outcome(scenario, Status::Pass, None, &evidence, retried)
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
    raw_bytes: &[u8],
    retried: bool,
) -> ScenarioOutcome {
    ScenarioOutcome {
        id: scenario.id.clone(),
        category: scenario.category,
        status,
        failure_reason,
        evidence_hash: evidence_hash(raw_bytes),
        retried,
    }
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

fn evidence_hash(raw_bytes: &[u8]) -> Option<String> {
    if raw_bytes.is_empty() {
        return None;
    }
    // Evidence hashes are SHA-256 over the raw response bodies in turn order.
    let hash = digest(&SHA256, raw_bytes);
    Some(format!("sha256:{}", hex(hash.as_ref())))
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
