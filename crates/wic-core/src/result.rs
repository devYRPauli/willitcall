use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ScenarioCategory;

pub const RESULT_SCHEMA_VERSION: u32 = 1;

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
    pub evidence_hash: Option<String>,
    pub retried: bool,
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
    Ok(result)
}

pub fn validate_result(result: &RunResult) -> Result<(), String> {
    if result.schema_version != RESULT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version {}; expected {}",
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

fn atomic_write_with(
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, Write};

    use super::{
        RunMetadata, RunResult, SamplingParams, ScenarioOutcome, ServerMetadata, Status, Totals,
        atomic_write_with, write_result_atomic,
    };
    use crate::ScenarioCategory;

    fn sample_result() -> RunResult {
        RunResult {
            schema_version: 1,
            metadata: RunMetadata {
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
                    max_tokens: Some(256),
                },
            },
            scenarios: vec![ScenarioOutcome {
                id: "single-weather".to_owned(),
                category: ScenarioCategory::SingleCall,
                status: Status::Pass,
                failure_reason: None,
                evidence_hash: Some("sha256:abc123".to_owned()),
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

        assert!(json.starts_with("{\"schema_version\":1,"));
    }

    #[test]
    fn status_serializes_all_four_outcomes() {
        let values = [Status::Pass, Status::Fail, Status::Error, Status::Skipped]
            .map(|status| serde_json::to_value(status).expect("serialize status"));

        assert_eq!(values, ["pass", "fail", "error", "skipped"]);
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
        assert_eq!(parsed.schema_version, 1);
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
            evidence_hash: None,
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
        let document: serde_json::Value = serde_json::from_slice(
            &fs::read(&destination).expect("read generated result"),
        )
        .expect("parse generated result");
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/result-v1.schema.json"
        ))
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
    fn validator_rejects_wrong_schema_version_and_inconsistent_totals() {
        let mut result = sample_result();
        result.schema_version = 2;
        assert_eq!(
            super::validate_result(&result).expect_err("wrong version must fail"),
            "unsupported schema_version 2; expected 1"
        );

        result.schema_version = 1;
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
}
