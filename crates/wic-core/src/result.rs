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
pub struct RunResult {
    pub schema_version: u32,
    pub metadata: RunMetadata,
    pub scenarios: Vec<ScenarioOutcome>,
    pub totals: Totals,
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
    pub sampling: SamplingParams,
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
    let result: RunResult = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid result document: {error}"))?;
    validate_result(&result)?;
    if result.schema_version == 2 {
        let document: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid result document: {error}"))?;
        let metadata = document
            .get("metadata")
            .and_then(Value::as_object)
            .ok_or_else(|| "invalid result document: metadata must be an object".to_owned())?;
        if !metadata.contains_key("run_id") {
            return Err(
                "invalid result document: metadata.run_id is required for schema_version 2"
                    .to_owned(),
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
    }
    Ok(result)
}

pub fn validate_result(result: &RunResult) -> Result<(), String> {
    if !matches!(result.schema_version, 1 | RESULT_SCHEMA_VERSION) {
        return Err(format!(
            "unsupported schema_version {}; expected 1 or {}",
            result.schema_version, RESULT_SCHEMA_VERSION
        ));
    }
    if result.totals.total != result.scenarios.len() as u32 {
        return Err(format!(
            "totals.total is {} but scenarios contains {} outcome{}",
            result.totals.total,
            result.scenarios.len(),
            if result.scenarios.len() == 1 { "" } else { "s" }
        ));
    }

    let mut actual = Totals {
        total: result.scenarios.len() as u32,
        passed: 0,
        failed: 0,
        errors: 0,
        skipped: 0,
    };
    for outcome in &result.scenarios {
        match outcome.status {
            Status::Pass => actual.passed += 1,
            Status::Fail => actual.failed += 1,
            Status::Error => actual.errors += 1,
            Status::Skipped => actual.skipped += 1,
        }
    }
    for (name, declared, counted) in [
        ("passed", result.totals.passed, actual.passed),
        ("failed", result.totals.failed, actual.failed),
        ("errors", result.totals.errors, actual.errors),
        ("skipped", result.totals.skipped, actual.skipped),
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
        atomic_write_with, write_result_atomic, Cause, CauseKind, RunMetadata, RunResult,
        SamplingParams, ScenarioOutcome, ServerMetadata, Status, Totals,
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
                sampling: SamplingParams {
                    temperature: Some(0.0),
                    top_p: Some(1.0),
                    seed: Some(42),
                    max_tokens: Some(1024),
                },
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
            failure_class: matches!(status, Status::Fail).then(|| "empty_response".to_owned()),
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
