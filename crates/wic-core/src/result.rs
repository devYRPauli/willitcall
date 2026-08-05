use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ScenarioCategory;

pub const RESULT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunResultV1V2 {
    pub schema_version: u32,
    pub metadata: RunMetadata,
    pub scenarios: Vec<ScenarioOutcome>,
    pub totals: Totals,
}

pub type RunResult = RunResultV1V2;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunResultV3 {
    pub schema_version: u32,
    pub metadata: RunMetadataV3,
    pub scenarios: Vec<ScenarioOutcomeV3>,
    pub totals: Totals,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunMetadataV3 {
    pub run_id: String,
    pub timestamp: String,
    pub willitcall_version: String,
    pub endpoint: String,
    pub model: ModelMetadata,
    pub corpus: CorpusMetadata,
    pub server: ServerMetadataV3,
    pub environment: EnvironmentMetadataV3,
    pub sampling: SamplingParams,
    pub replication: ReplicationMetadata,
    pub arm_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_override: Option<PreflightOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_ignored_ports: Option<Vec<u16>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadata {
    pub display_name: String,
    pub family_id: Option<String>,
    pub canonical_id: Option<String>,
    pub parameter_count_b: Option<f64>,
    pub endpoint_id: String,
    pub identity_status: IdentityStatus,
    pub artifact: ArtifactMetadata,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityStatus {
    Verified,
    Declared,
    Unresolved,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactMetadata {
    pub source_kind: ArtifactSourceKind,
    pub source_id: Option<String>,
    pub revision: Option<String>,
    pub sha256: Option<String>,
    pub format: ArtifactFormat,
    pub quantization: Option<QuantizationMetadata>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSourceKind {
    Huggingface,
    Ollama,
    LocalFile,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    Gguf,
    Mlx,
    Safetensors,
    OllamaBlob,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuantizationMetadata {
    pub label: String,
    pub scheme: Option<String>,
    pub bits: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusMetadata {
    pub id: String,
    pub revision: String,
    pub sha256: String,
    pub scenario_count: u32,
    pub scoring_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerMetadataV3 {
    pub preset_name: String,
    pub reported_version: Option<String>,
    pub quirk_flags: Vec<String>,
    pub decode_mode: DecodeMode,
    pub chat_template: Option<ChatTemplateMetadata>,
    pub launch_config_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecodeMode {
    GrammarConstrained,
    UnconstrainedPostHoc,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChatTemplateMetadata {
    pub id: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentMetadataV3 {
    pub display_label: String,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub architecture: Option<String>,
    pub accelerator: Option<String>,
    pub memory_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicationMetadata {
    pub study_id: String,
    pub arm_id: String,
    pub run_index: u32,
    pub mode: ReplicationMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplicationMode {
    GreedyReproducibility,
    SeedVariedVariance,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunMetadata {
    #[serde(default)]
    pub run_id: String,
    pub timestamp: String,
    pub willitcall_version: String,
    pub endpoint: String,
    pub model_id: String,
    pub declared_quant: Option<String>,
    pub server: ServerMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentMetadata>,
    pub sampling: SamplingParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_override: Option<PreflightOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preflight_ignored_ports: Option<Vec<u16>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentMetadata {
    pub host_hardware_class: String,
    pub host_os: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightOverride {
    pub forced: bool,
    pub foreign_endpoints: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerMetadata {
    pub preset_name: String,
    pub reported_version: Option<String>,
    pub quirk_flags: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SamplingParams {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub seed: Option<u64>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioOutcome {
    pub id: String,
    pub category: ScenarioCategory,
    pub status: Status,
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    pub evidence_hash: Option<String>,
    #[serde(default)]
    pub evidence_path: Option<String>,
    pub retried: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioOutcomeV3 {
    pub id: String,
    pub category: ScenarioCategory,
    pub status: Status,
    pub failure_reason: Option<String>,
    pub failure: Option<ScenarioFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    pub evidence_hash: Option<String>,
    pub evidence_path: Option<String>,
    pub retried: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioFailure {
    pub stage: String,
    pub code: String,
    pub http_status: Option<u16>,
    pub failed_turn_index: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Measurement {
    pub schema_version: u32,
    pub metadata: MeasurementMetadata,
    pub scenarios: Vec<MeasurementScenarioOutcome>,
    pub totals: Totals,
}

#[derive(Clone, Debug)]
pub struct MeasurementMetadata {
    pub run_id: String,
    pub timestamp: String,
    pub willitcall_version: String,
    pub endpoint: String,
    pub model: ModelMetadata,
    pub corpus: Option<CorpusMetadata>,
    pub server: MeasurementServerMetadata,
    pub environment: Option<EnvironmentMetadataV3>,
    pub sampling: SamplingParams,
    pub replication: Option<ReplicationMetadata>,
    pub arm_fingerprint: Option<String>,
    pub preflight_override: Option<PreflightOverride>,
    pub preflight_ignored_ports: Option<Vec<u16>>,
}

#[derive(Clone, Debug)]
pub struct MeasurementServerMetadata {
    pub preset_name: String,
    pub reported_version: Option<String>,
    pub quirk_flags: Vec<String>,
    pub decode_mode: DecodeMode,
    pub chat_template: Option<ChatTemplateMetadata>,
    pub launch_config_sha256: Option<String>,
}

#[derive(Clone, Debug)]
pub struct MeasurementScenarioOutcome {
    pub id: String,
    pub category: ScenarioCategory,
    pub status: Status,
    pub failure_reason: Option<String>,
    pub failure: Option<ScenarioFailure>,
    pub failure_class: Option<String>,
    pub cause: Option<Cause>,
    pub evidence_hash: Option<String>,
    pub evidence_path: Option<String>,
    pub retried: bool,
}

impl Measurement {
    pub fn cross_model_key(&self) -> Option<&str> {
        if self.metadata.model.identity_status == IdentityStatus::Unresolved {
            None
        } else {
            self.metadata.model.canonical_id.as_deref()
        }
    }
}

impl From<RunResultV1V2> for Measurement {
    fn from(result: RunResultV1V2) -> Self {
        let metadata = result.metadata;
        let quantization = metadata.declared_quant.map(|label| QuantizationMetadata {
            label,
            scheme: None,
            bits: None,
        });
        let environment = metadata
            .environment
            .map(|environment| EnvironmentMetadataV3 {
                display_label: format!(
                    "{}; {}",
                    environment.host_hardware_class, environment.host_os
                ),
                os_name: None,
                os_version: None,
                architecture: None,
                accelerator: None,
                memory_bytes: None,
            });
        Self {
            schema_version: result.schema_version,
            metadata: MeasurementMetadata {
                run_id: metadata.run_id,
                timestamp: metadata.timestamp,
                willitcall_version: metadata.willitcall_version,
                endpoint: metadata.endpoint,
                model: ModelMetadata {
                    display_name: metadata.model_id.clone(),
                    family_id: None,
                    canonical_id: None,
                    parameter_count_b: None,
                    endpoint_id: metadata.model_id,
                    identity_status: IdentityStatus::Unresolved,
                    artifact: ArtifactMetadata {
                        source_kind: ArtifactSourceKind::Other,
                        source_id: None,
                        revision: None,
                        sha256: None,
                        format: ArtifactFormat::Unknown,
                        quantization,
                    },
                },
                corpus: None,
                server: MeasurementServerMetadata {
                    preset_name: metadata.server.preset_name,
                    reported_version: metadata.server.reported_version,
                    quirk_flags: metadata.server.quirk_flags,
                    decode_mode: DecodeMode::Unknown,
                    chat_template: None,
                    launch_config_sha256: None,
                },
                environment,
                sampling: metadata.sampling,
                replication: None,
                arm_fingerprint: None,
                preflight_override: metadata.preflight_override,
                preflight_ignored_ports: metadata.preflight_ignored_ports,
            },
            scenarios: result
                .scenarios
                .into_iter()
                .map(MeasurementScenarioOutcome::from)
                .collect(),
            totals: result.totals,
        }
    }
}

impl From<RunResultV3> for Measurement {
    fn from(result: RunResultV3) -> Self {
        let metadata = result.metadata;
        Self {
            schema_version: result.schema_version,
            metadata: MeasurementMetadata {
                run_id: metadata.run_id,
                timestamp: metadata.timestamp,
                willitcall_version: metadata.willitcall_version,
                endpoint: metadata.endpoint,
                model: metadata.model,
                corpus: Some(metadata.corpus),
                server: MeasurementServerMetadata {
                    preset_name: metadata.server.preset_name,
                    reported_version: metadata.server.reported_version,
                    quirk_flags: metadata.server.quirk_flags,
                    decode_mode: metadata.server.decode_mode,
                    chat_template: metadata.server.chat_template,
                    launch_config_sha256: metadata.server.launch_config_sha256,
                },
                environment: Some(metadata.environment),
                sampling: metadata.sampling,
                replication: Some(metadata.replication),
                arm_fingerprint: Some(metadata.arm_fingerprint),
                preflight_override: metadata.preflight_override,
                preflight_ignored_ports: metadata.preflight_ignored_ports,
            },
            scenarios: result
                .scenarios
                .into_iter()
                .map(MeasurementScenarioOutcome::from)
                .collect(),
            totals: result.totals,
        }
    }
}

impl From<ScenarioOutcome> for MeasurementScenarioOutcome {
    fn from(outcome: ScenarioOutcome) -> Self {
        Self {
            id: outcome.id,
            category: outcome.category,
            status: outcome.status,
            failure_reason: outcome.failure_reason,
            failure: None,
            failure_class: outcome.failure_class,
            cause: outcome.cause,
            evidence_hash: outcome.evidence_hash,
            evidence_path: outcome.evidence_path,
            retried: outcome.retried,
        }
    }
}

impl From<ScenarioOutcomeV3> for MeasurementScenarioOutcome {
    fn from(outcome: ScenarioOutcomeV3) -> Self {
        Self {
            id: outcome.id,
            category: outcome.category,
            status: outcome.status,
            failure_reason: outcome.failure_reason,
            failure: outcome.failure,
            failure_class: outcome.failure_class,
            cause: outcome.cause,
            evidence_hash: outcome.evidence_hash,
            evidence_path: outcome.evidence_path,
            retried: outcome.retried,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cause {
    pub kind: CauseKind,
    pub reference: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CauseKind {
    ServerDefect,
    Unknown,
}

#[derive(Clone, Debug)]
pub(crate) struct CapturedRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct CapturedResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CapturedTurn {
    pub(crate) request: CapturedRequest,
    pub(crate) response: Option<CapturedResponse>,
    pub(crate) retried: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Transcript {
    pub schema_version: u32,
    pub run_id: String,
    pub scenario_id: String,
    pub turns: Vec<TranscriptTurn>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptTurn {
    pub index: usize,
    pub request: TranscriptRequest,
    pub response: Option<TranscriptResponse>,
    pub retried: bool,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranscriptResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_raw_hex: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    Error,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Totals {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub errors: u32,
    pub skipped: u32,
}

pub fn exit_code_for_totals(totals: &Totals) -> u8 {
    if totals.errors > 0 {
        4
    } else if totals.failed > 0 {
        1
    } else {
        0
    }
}

pub fn parse_and_validate_result(bytes: &[u8]) -> Result<RunResult, String> {
    let (document, schema_version) = inspect_result_document(bytes)?;
    if !matches!(schema_version, 1 | RESULT_SCHEMA_VERSION) {
        return Err(format!(
            "unsupported schema_version {schema_version}; expected 1 or {RESULT_SCHEMA_VERSION}"
        ));
    }
    let result: RunResultV1V2 = serde_json::from_value(document.clone())
        .map_err(|error| format!("invalid result document: {error}"))?;
    validate_result(&result)?;
    if schema_version == 2 {
        validate_v2_required_properties(&document)?;
    }
    Ok(result)
}

pub fn parse_and_validate_measurement(bytes: &[u8]) -> Result<Measurement, String> {
    let (document, schema_version) = inspect_result_document(bytes)?;
    let measurement = match schema_version {
        1 | 2 => {
            let result: RunResultV1V2 = serde_json::from_value(document.clone())
                .map_err(|error| format!("invalid result document: {error}"))?;
            validate_result(&result)?;
            if schema_version == 2 {
                validate_v2_required_properties(&document)?;
            }
            Measurement::from(result)
        }
        3 => {
            let result: RunResultV3 = serde_json::from_value(document)
                .map_err(|error| format!("invalid result document: {error}"))?;
            let measurement = Measurement::from(result);
            validate_measurement(&measurement)?;
            measurement
        }
        _ => {
            return Err(format!(
                "unsupported schema_version {schema_version}; expected 1, 2, or 3"
            ));
        }
    };
    Ok(measurement)
}

fn inspect_result_document(bytes: &[u8]) -> Result<(Value, u32), String> {
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid result document: {error}"))?;
    let schema_version = document
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| {
            "invalid result document: schema_version must be an unsigned 32-bit integer".to_owned()
        })?;
    Ok((document, schema_version))
}

fn validate_v2_required_properties(document: &Value) -> Result<(), String> {
    let metadata = document
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| "invalid result document: metadata must be an object".to_owned())?;
    if !metadata.contains_key("run_id") {
        return Err(
            "invalid result document: metadata.run_id is required for schema_version 2".to_owned(),
        );
    }
    let scenarios = document
        .get("scenarios")
        .and_then(Value::as_array)
        .ok_or_else(|| "invalid result document: scenarios must be an array".to_owned())?;
    if scenarios.iter().any(|scenario| {
        !scenario
            .as_object()
            .is_some_and(|scenario| scenario.contains_key("evidence_path"))
    }) {
        return Err(
            "invalid result document: scenario evidence_path is required for schema_version 2"
                .to_owned(),
        );
    }
    Ok(())
}

pub fn validate_result(result: &RunResult) -> Result<(), String> {
    if !matches!(result.schema_version, 1 | RESULT_SCHEMA_VERSION) {
        return Err(format!(
            "unsupported schema_version {}; expected 1 or {}",
            result.schema_version, RESULT_SCHEMA_VERSION
        ));
    }
    validate_totals(
        &result.totals,
        result.scenarios.iter().map(|outcome| outcome.status),
        result.scenarios.len(),
    )
}

pub fn validate_measurement(measurement: &Measurement) -> Result<(), String> {
    if !matches!(measurement.schema_version, 1..=3) {
        return Err(format!(
            "unsupported schema_version {}; expected 1, 2, or 3",
            measurement.schema_version
        ));
    }
    validate_totals(
        &measurement.totals,
        measurement.scenarios.iter().map(|outcome| outcome.status),
        measurement.scenarios.len(),
    )
}

fn validate_totals(
    totals: &Totals,
    statuses: impl Iterator<Item = Status>,
    scenario_count: usize,
) -> Result<(), String> {
    if totals.total != scenario_count as u32 {
        return Err(format!(
            "totals.total is {} but scenarios contains {} outcome{}",
            totals.total,
            scenario_count,
            if scenario_count == 1 { "" } else { "s" }
        ));
    }

    let mut actual = Totals {
        total: scenario_count as u32,
        passed: 0,
        failed: 0,
        errors: 0,
        skipped: 0,
    };
    for status in statuses {
        match status {
            Status::Pass => actual.passed += 1,
            Status::Fail => actual.failed += 1,
            Status::Error => actual.errors += 1,
            Status::Skipped => actual.skipped += 1,
        }
    }
    for (name, declared, counted) in [
        ("passed", totals.passed, actual.passed),
        ("failed", totals.failed, actual.failed),
        ("errors", totals.errors, actual.errors),
        ("skipped", totals.skipped, actual.skipped),
    ] {
        if declared != counted {
            return Err(format!(
                "totals.{name} is {declared} but scenario outcomes count to {counted}"
            ));
        }
    }
    Ok(())
}

pub fn write_result_atomic(path: &Path, result: &RunResult) -> io::Result<()> {
    atomic_write_with(path, |file| {
        serde_json::to_writer_pretty(&mut *file, result).map_err(io::Error::other)?;
        file.write_all(b"\n")
    })
}

pub(crate) fn write_transcript_atomic(path: &Path, transcript: &Transcript) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(transcript).map_err(io::Error::other)?;
    bytes.push(b'\n');
    atomic_write_with(path, |file| file.write_all(&bytes))?;
    Ok(bytes)
}

pub(crate) fn atomic_write_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write(temporary.as_file_mut())?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub(crate) fn redact_transcript_turn(index: usize, captured: CapturedTurn) -> TranscriptTurn {
    const SENSITIVE_HEADERS: [&str; 7] = [
        "authorization",
        "api-key",
        "x-api-key",
        "openai-api-key",
        "cookie",
        "set-cookie",
        "proxy-authorization",
    ];
    const SENSITIVE_QUERY_PARAMETERS: [&str; 5] =
        ["api_key", "apikey", "key", "access_token", "token"];

    let CapturedTurn {
        request,
        response,
        retried,
    } = captured;
    let mut request_headers = request.headers;
    for (name, value) in &mut request_headers {
        if SENSITIVE_HEADERS
            .iter()
            .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
        {
            *value = "[REDACTED]".to_owned();
        }
    }
    let mut request_url = request.url;
    if let Ok(mut url) = reqwest::Url::parse(&request_url) {
        let query = url
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        if query.iter().any(|(name, _)| {
            SENSITIVE_QUERY_PARAMETERS
                .iter()
                .any(|sensitive| name == sensitive)
        }) {
            url.query_pairs_mut()
                .clear()
                .extend_pairs(query.iter().map(|(name, value)| {
                    (
                        name.as_str(),
                        if SENSITIVE_QUERY_PARAMETERS
                            .iter()
                            .any(|sensitive| name == sensitive)
                        {
                            "REDACTED"
                        } else {
                            value.as_str()
                        },
                    )
                }));
            request_url = url.to_string();
        }
    }

    let response = response.map(|response| {
        let mut headers = response.headers;
        for (name, value) in &mut headers {
            if SENSITIVE_HEADERS
                .iter()
                .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
            {
                *value = "[REDACTED]".to_owned();
            }
        }
        let (body_raw, body_raw_hex) = match String::from_utf8(response.body) {
            Ok(body) => (Some(body), None),
            Err(error) => (None, Some(hex(&error.into_bytes()))),
        };
        TranscriptResponse {
            status: response.status,
            headers,
            body_raw,
            body_raw_hex,
        }
    });

    TranscriptTurn {
        index,
        request: TranscriptRequest {
            method: request.method,
            url: request_url,
            headers: request_headers,
            body: request.body,
        },
        response,
        retried,
    }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::{self, Write};

    use super::{
        atomic_write_with, write_result_atomic, Cause, CauseKind, EnvironmentMetadata, RunMetadata,
        RunResult, SamplingParams, ScenarioOutcome, ServerMetadata, Status, Totals,
    };
    use crate::ScenarioCategory;

    fn sample_result() -> RunResult {
        RunResult {
            schema_version: 2,
            metadata: RunMetadata {
                run_id: "20260719T120000Z-1234abcd".to_owned(),
                timestamp: "2026-07-19T12:00:00Z".to_owned(),
                willitcall_version: "0.1.0".to_owned(),
                endpoint: "http://127.0.0.1:8080/v1".to_owned(),
                model_id: "local-model".to_owned(),
                declared_quant: Some("Q4_K_M".to_owned()),
                server: ServerMetadata {
                    preset_name: "llama.cpp".to_owned(),
                    reported_version: Some("b6000".to_owned()),
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
                preflight_ignored_ports: None,
            },
            scenarios: vec![ScenarioOutcome {
                id: "single-weather".to_owned(),
                category: ScenarioCategory::SingleCall,
                status: Status::Pass,
                failure_reason: None,
                failure_class: None,
                cause: None,
                evidence_hash: Some("sha256:abc123".to_owned()),
                evidence_path: Some(
                    "evidence/20260719T120000Z-1234abcd/single-weather.json".to_owned(),
                ),
                retried: false,
            }],
            totals: Totals {
                total: 1,
                passed: 1,
                failed: 0,
                errors: 0,
                skipped: 0,
            },
        }
    }

    #[test]
    fn schema_version_is_the_first_serialized_field() {
        let json = serde_json::to_string(&sample_result()).expect("serialize result");

        assert!(json.starts_with("{\"schema_version\":2,"));
        assert!(!json.contains("preflight_override"));
        assert!(!json.contains("preflight_ignored_ports"));
    }

    #[test]
    fn status_serializes_all_four_outcomes() {
        let values = [Status::Pass, Status::Fail, Status::Error, Status::Skipped]
            .map(|status| serde_json::to_value(status).expect("serialize status"));

        assert_eq!(values, ["pass", "fail", "error", "skipped"]);
    }

    #[test]
    fn failure_class_and_cause_serialize_and_round_trip() {
        let mut result = sample_result();
        result.scenarios[0].failure_class = Some("empty_response".to_owned());
        result.scenarios[0].cause = Some(Cause {
            kind: CauseKind::ServerDefect,
            reference: Some("ollama/ollama#12345".to_owned()),
            note: None,
        });

        let json = serde_json::to_string(&result).expect("serialize annotated result");
        let parsed: RunResult = serde_json::from_str(&json).expect("parse annotated result");
        let outcome = &parsed.scenarios[0];

        assert_eq!(outcome.failure_class.as_deref(), Some("empty_response"));
        let cause = outcome.cause.as_ref().expect("cause");
        assert_eq!(cause.kind, CauseKind::ServerDefect);
        assert_eq!(cause.reference.as_deref(), Some("ollama/ollama#12345"));
        assert_eq!(cause.note, None);
    }

    #[test]
    fn exit_code_prioritizes_harness_errors_then_model_failures() {
        let mut totals = Totals {
            total: 1,
            passed: 1,
            failed: 0,
            errors: 0,
            skipped: 0,
        };
        assert_eq!(super::exit_code_for_totals(&totals), 0);
        totals.passed = 0;
        totals.failed = 1;
        assert_eq!(super::exit_code_for_totals(&totals), 1);
        totals.errors = 1;
        assert_eq!(super::exit_code_for_totals(&totals), 4);
    }

    #[test]
    fn atomic_write_publishes_a_complete_result() {
        let directory = tempfile::tempdir().expect("temp directory");
        let destination = directory.path().join("result.json");

        write_result_atomic(&destination, &sample_result()).expect("write result");

        let written = fs::read_to_string(destination).expect("read result");
        let parsed: RunResult = serde_json::from_str(&written).expect("parse written result");
        assert_eq!(parsed.schema_version, 2);
        assert_eq!(parsed.scenarios.len(), 1);
    }

    #[test]
    fn generated_result_matches_the_checked_in_json_schema() {
        let directory = tempfile::tempdir().expect("temp directory");
        let destination = directory.path().join("result.json");
        let mut result = sample_result();
        result.metadata.environment = Some(EnvironmentMetadata {
            host_hardware_class: "Apple M4 Max, 64GB".to_owned(),
            host_os: "macOS 15.5".to_owned(),
        });
        result.metadata.preflight_override = Some(super::PreflightOverride {
            forced: true,
            foreign_endpoints: vec!["127.0.0.1:11434".to_owned()],
        });
        result.metadata.preflight_ignored_ports = Some(vec![8000, 9000]);
        result.scenarios = [
            (ScenarioCategory::SingleCall, Status::Pass),
            (ScenarioCategory::ParallelCalls, Status::Fail),
            (ScenarioCategory::Streaming, Status::Error),
            (ScenarioCategory::ToolChoiceModes, Status::Skipped),
            (ScenarioCategory::MultiTurn, Status::Pass),
            (ScenarioCategory::NegativeTrap, Status::Pass),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (category, status))| ScenarioOutcome {
            id: format!("schema-case-{index}"),
            category,
            status,
            failure_reason: matches!(status, Status::Fail | Status::Error)
                .then(|| "fixture failure".to_owned()),
            failure_class: matches!(status, Status::Fail).then(|| "unparsed_tool_call".to_owned()),
            cause: matches!(status, Status::Fail).then_some(Cause {
                kind: CauseKind::Unknown,
                reference: None,
                note: None,
            }),
            evidence_hash: None,
            evidence_path: None,
            retried: false,
        })
        .collect();
        result.totals = Totals {
            total: 6,
            passed: 3,
            failed: 1,
            errors: 1,
            skipped: 1,
        };
        write_result_atomic(&destination, &result).expect("write result");
        let document: serde_json::Value =
            serde_json::from_slice(&fs::read(&destination).expect("read generated result"))
                .expect("parse generated result");
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../schemas/result-v2.schema.json"))
                .expect("parse checked-in schema");
        let validator = jsonschema::validator_for(&schema).expect("compile result schema");

        validator
            .validate(&document)
            .expect("generated result should satisfy checked-in schema");
    }

    #[test]
    fn environment_remains_optional_for_existing_v2_documents() {
        let bytes = serde_json::to_vec(&sample_result()).expect("encode result");

        let result = super::parse_and_validate_result(&bytes)
            .expect("existing v2 result without environment should remain valid");

        assert!(result.metadata.environment.is_none());
    }

    #[test]
    fn parses_v1_v2_v3_into_measurement() {
        let mut v1 = serde_json::to_value(sample_result()).expect("serialize v1 fixture");
        v1["schema_version"] = serde_json::json!(1);
        v1["metadata"]
            .as_object_mut()
            .expect("v1 metadata")
            .remove("run_id");
        v1["scenarios"][0]
            .as_object_mut()
            .expect("v1 scenario")
            .remove("evidence_path");
        let v1 = super::parse_and_validate_measurement(
            &serde_json::to_vec(&v1).expect("encode v1 fixture"),
        )
        .expect("parse v1 measurement");
        assert_eq!(v1.schema_version, 1);
        assert_eq!(v1.metadata.model.display_name, "local-model");
        assert_eq!(
            v1.metadata.model.identity_status,
            super::IdentityStatus::Unresolved
        );
        assert_eq!(v1.cross_model_key(), None);

        let v2_document = serde_json::to_value(sample_result()).expect("serialize v2 fixture");
        let v2 = super::parse_and_validate_measurement(
            &serde_json::to_vec(&v2_document).expect("encode v2 fixture"),
        )
        .expect("parse v2 measurement");
        assert_eq!(v2.schema_version, 2);
        assert_eq!(v2.metadata.model.endpoint_id, "local-model");
        assert_eq!(v2.cross_model_key(), None);

        let v3_document = serde_json::json!({
            "schema_version": 3,
            "metadata": {
                "run_id": "019c8a4a-05b0-7c22-9f44-2df806328a22",
                "timestamp": "2026-08-05T12:00:00Z",
                "willitcall_version": "0.1.0",
                "endpoint": "http://127.0.0.1:8080/v1",
                "model": {
                    "display_name": "Qwen 2.5 7B Instruct",
                    "family_id": "qwen2.5",
                    "canonical_id": "Qwen/Qwen2.5-7B-Instruct",
                    "parameter_count_b": 7.62,
                    "endpoint_id": "qwen2.5-7b-instruct-q4_k_m.gguf",
                    "identity_status": "verified",
                    "artifact": {
                        "source_kind": "huggingface",
                        "source_id": "bartowski/Qwen2.5-7B-Instruct-GGUF",
                        "revision": "0123456789abcdef",
                        "sha256": "sha256:0123456789abcdef",
                        "format": "gguf",
                        "quantization": {
                            "label": "Q4_K_M",
                            "scheme": "k-quant",
                            "bits": 4
                        }
                    }
                },
                "corpus": {
                    "id": "willitcall-core",
                    "revision": "v1",
                    "sha256": "sha256:corpus",
                    "scenario_count": 1,
                    "scoring_version": "v1"
                },
                "server": {
                    "preset_name": "llamacpp",
                    "reported_version": "b6000",
                    "quirk_flags": ["grammar_constrained_decoding"],
                    "decode_mode": "grammar_constrained",
                    "chat_template": {
                        "id": "qwen2.5",
                        "sha256": "sha256:template"
                    },
                    "launch_config_sha256": "sha256:launch"
                },
                "environment": {
                    "display_label": "Apple M4 Max, 64GB; macOS 15.5",
                    "os_name": "macOS",
                    "os_version": "15.5",
                    "architecture": "aarch64",
                    "accelerator": "Apple M4 Max",
                    "memory_bytes": 68719476736_u64
                },
                "sampling": {
                    "temperature": 0.0,
                    "top_p": 1.0,
                    "seed": 42,
                    "max_tokens": 1024
                },
                "replication": {
                    "study_id": "m7-baseline",
                    "arm_id": "qwen2.5-7b-llamacpp",
                    "run_index": 0,
                    "mode": "greedy_reproducibility"
                },
                "arm_fingerprint": "v1:arm"
            },
            "scenarios": [{
                "id": "single-weather",
                "category": "single_call",
                "status": "error",
                "failure_reason": "turn 1: server returned HTTP 400",
                "failure": {
                    "stage": "request",
                    "code": "http_error",
                    "http_status": 400,
                    "failed_turn_index": 1
                },
                "evidence_hash": "sha256:abc123",
                "evidence_path": "evidence/run/single-weather.json",
                "retried": false
            }],
            "totals": {
                "total": 1,
                "passed": 0,
                "failed": 0,
                "errors": 1,
                "skipped": 0
            }
        });
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../schemas/result-v3.schema.json"))
                .expect("parse v3 schema");
        jsonschema::validator_for(&schema)
            .expect("compile v3 schema")
            .validate(&v3_document)
            .expect("v3 fixture should satisfy schema");

        let v3 = super::parse_and_validate_measurement(
            &serde_json::to_vec(&v3_document).expect("encode v3 fixture"),
        )
        .expect("parse v3 measurement");
        assert_eq!(v3.schema_version, 3);
        assert_eq!(v3.cross_model_key(), Some("Qwen/Qwen2.5-7B-Instruct"));
        assert_eq!(
            v3.scenarios[0]
                .failure
                .as_ref()
                .expect("structured failure")
                .http_status,
            Some(400)
        );

        let mut unresolved = v3_document.clone();
        unresolved["metadata"]["model"]["identity_status"] = serde_json::json!("unresolved");
        let unresolved = super::parse_and_validate_measurement(
            &serde_json::to_vec(&unresolved).expect("encode unresolved fixture"),
        )
        .expect("parse unresolved measurement");
        assert_eq!(unresolved.cross_model_key(), None);

        let mut missing_canonical_id = v3_document.clone();
        missing_canonical_id["metadata"]["model"]["canonical_id"] = serde_json::Value::Null;
        let missing_canonical_id = super::parse_and_validate_measurement(
            &serde_json::to_vec(&missing_canonical_id).expect("encode declared fixture"),
        )
        .expect("parse declared measurement");
        assert_eq!(missing_canonical_id.cross_model_key(), None);

        let mut v3_field_in_v2 = v2_document;
        v3_field_in_v2["metadata"]["model"] = v3_document["metadata"]["model"].clone();
        let error = super::parse_and_validate_measurement(
            &serde_json::to_vec(&v3_field_in_v2).expect("encode invalid v2 fixture"),
        )
        .expect_err("v3-only field in v2 must fail");
        assert!(error.contains("unknown field `model`"), "{error}");
    }

    #[test]
    fn failed_mid_write_never_creates_the_destination() {
        let directory = tempfile::tempdir().expect("temp directory");
        let destination = directory.path().join("result.json");

        let error = atomic_write_with(&destination, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        })
        .expect_err("write should fail");

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(!destination.exists());
    }

    #[test]
    fn validator_accepts_v1_and_v2_and_rejects_other_versions() {
        let mut result = sample_result();
        result.schema_version = 1;
        super::validate_result(&result).expect("v1 should validate");
        result.schema_version = 2;
        super::validate_result(&result).expect("v2 should validate");
        result.schema_version = 3;
        assert_eq!(
            super::validate_result(&result).expect_err("wrong version must fail"),
            "unsupported schema_version 3; expected 1 or 2"
        );

        result.schema_version = 2;
        result.totals.total = 2;
        assert_eq!(
            super::validate_result(&result).expect_err("bad totals must fail"),
            "totals.total is 2 but scenarios contains 1 outcome"
        );
    }

    #[test]
    fn validator_rejects_unknown_fields_with_a_precise_message() {
        let mut document = serde_json::to_value(sample_result()).expect("serialize result");
        document
            .as_object_mut()
            .expect("result object")
            .insert("unexpected".to_owned(), serde_json::json!(true));
        let bytes = serde_json::to_vec(&document).expect("encode result");

        let error = super::parse_and_validate_result(&bytes).expect_err("unknown field must fail");

        assert!(error.starts_with("invalid result document:"), "{error}");
        assert!(error.contains("unknown field `unexpected`"), "{error}");
    }

    #[test]
    fn v2_document_requires_run_id_and_evidence_path_properties() {
        let mut missing_run_id = serde_json::to_value(sample_result()).expect("serialize result");
        missing_run_id["metadata"]
            .as_object_mut()
            .expect("metadata object")
            .remove("run_id");
        let error = super::parse_and_validate_result(
            &serde_json::to_vec(&missing_run_id).expect("encode result"),
        )
        .expect_err("v2 run_id is required");
        assert!(error.contains("metadata.run_id is required"), "{error}");

        let mut missing_evidence_path =
            serde_json::to_value(sample_result()).expect("serialize result");
        missing_evidence_path["scenarios"][0]
            .as_object_mut()
            .expect("scenario object")
            .remove("evidence_path");
        let error = super::parse_and_validate_result(
            &serde_json::to_vec(&missing_evidence_path).expect("encode result"),
        )
        .expect_err("v2 evidence_path is required");
        assert!(error.contains("evidence_path is required"), "{error}");
    }

    #[test]
    fn redaction_covers_sensitive_headers_query_parameters_and_raw_bytes() {
        let turn = super::redact_transcript_turn(
            0,
            super::CapturedTurn {
                request: super::CapturedRequest {
                    method: "POST".to_owned(),
                    url: "https://example.test/v1/chat?api_key=one&apikey=two&key=three&access_token=four&token=five&keep=value".to_owned(),
                    headers: BTreeMap::from([
                        ("Authorization".to_owned(), "Bearer secret".to_owned()),
                        ("API-Key".to_owned(), "api-key secret".to_owned()),
                        ("X-API-KEY".to_owned(), "x-api-key secret".to_owned()),
                        (
                            "OpenAI-API-Key".to_owned(),
                            "openai-api-key secret".to_owned(),
                        ),
                        ("Cookie".to_owned(), "cookie secret".to_owned()),
                        ("Set-Cookie".to_owned(), "set-cookie secret".to_owned()),
                        (
                            "Proxy-Authorization".to_owned(),
                            "proxy-authorization secret".to_owned(),
                        ),
                        ("content-type".to_owned(), "application/json".to_owned()),
                    ]),
                    body: serde_json::json!({"messages": []}),
                },
                response: Some(super::CapturedResponse {
                    status: 200,
                    headers: BTreeMap::from([
                        ("Set-Cookie".to_owned(), "session=secret".to_owned()),
                        (
                            "content-type".to_owned(),
                            "application/octet-stream".to_owned(),
                        ),
                    ]),
                    body: vec![0xff, 0x00, 0x7f],
                }),
                retried: false,
            },
        );

        assert_eq!(
            turn.request.url,
            "https://example.test/v1/chat?api_key=REDACTED&apikey=REDACTED&key=REDACTED&access_token=REDACTED&token=REDACTED&keep=value"
        );
        for (name, value) in &turn.request.headers {
            if name == "content-type" {
                assert_eq!(value, "application/json");
            } else {
                assert_eq!(value, "[REDACTED]", "header {name}");
            }
        }
        let response = turn.response.expect("recorded response");
        assert_eq!(response.headers["Set-Cookie"], "[REDACTED]");
        assert_eq!(response.body_raw, None);
        assert_eq!(response.body_raw_hex.as_deref(), Some("ff007f"));
    }
}
