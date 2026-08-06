#[path = "../../wic-core/tests/support/mod.rs"]
mod support;

use std::fs;
use std::process::Command;

use serde_json::{json, Value};
use support::{MockServer, ScriptedResponse};
use wic_core::result::{parse_and_validate_measurement, IdentityStatus, Status};

fn completion(calls: Value, content: Value) -> String {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": content,
                "tool_calls": calls,
            }
        }]
    })
    .to_string()
}

fn call(id: &str, name: &str, arguments: &str) -> Value {
    json!({
        "id": id,
        "type": "function",
        "function": {"name": name, "arguments": arguments}
    })
}

fn streaming_call(id: &str, name: &str, first: &str, second: &str) -> String {
    format!(
        "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "id": id, "function": {"name": name, "arguments": ""}
        }]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "function": {"arguments": first}
        }]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "function": {"arguments": second}
        }]}, "finish_reason": "tool_calls"}]})
    )
}

async fn run_binary(arguments: Vec<String>) -> std::process::Output {
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_willitcall"))
            .args(arguments)
            .output()
            .expect("run willitcall")
    })
    .await
    .expect("join willitcall process")
}

fn scenario_fixture(id: &str, failure_class: Option<&str>, evidence_path: Option<&str>) -> Value {
    let mut scenario = json!({
        "id": id,
        "category": "single_call",
        "status": "fail",
        "failure_reason": "no tool call emitted",
        "evidence_hash": null,
        "evidence_path": evidence_path,
        "retried": false
    });
    if let Some(failure_class) = failure_class {
        scenario["failure_class"] = json!(failure_class);
    }
    scenario
}

fn write_result_fixture(path: &std::path::Path, schema_version: u32, mut scenarios: Vec<Value>) {
    let mut metadata = json!({
        "run_id": "20260720T120000Z-fixture",
        "timestamp": "2026-07-20T12:00:00Z",
        "willitcall_version": "0.1.0",
        "endpoint": "http://127.0.0.1:8080/v1",
        "model_id": "fixture-model",
        "declared_quant": null,
        "server": {
            "preset_name": "custom",
            "reported_version": null,
            "quirk_flags": []
        },
        "sampling": {
            "temperature": 0.0,
            "top_p": 1.0,
            "seed": 42,
            "max_tokens": 1024
        }
    });
    if schema_version == 1 {
        metadata
            .as_object_mut()
            .expect("metadata object")
            .remove("run_id");
        for scenario in &mut scenarios {
            scenario
                .as_object_mut()
                .expect("scenario object")
                .remove("evidence_path");
        }
    }
    let total = scenarios.len() as u32;
    let result = json!({
        "schema_version": schema_version,
        "metadata": metadata,
        "scenarios": scenarios,
        "totals": {
            "total": total,
            "passed": 0,
            "failed": total,
            "errors": 0,
            "skipped": 0
        }
    });
    fs::write(
        path,
        serde_json::to_vec_pretty(&result).expect("encode result fixture"),
    )
    .expect("write result fixture");
}

fn write_v3_result_fixture(path: &std::path::Path, mut scenarios: Vec<Value>) {
    for scenario in &mut scenarios {
        scenario["failure"] = Value::Null;
    }
    let total = scenarios.len() as u32;
    let result = json!({
        "schema_version": 3,
        "metadata": {
            "run_id": "019fd4e8-8e84-7a90-a686-f3caf2e147ef",
            "timestamp": "2026-08-05T12:00:00Z",
            "willitcall_version": "0.1.0",
            "endpoint": "http://127.0.0.1:8080/v1",
            "model": {
                "display_name": "Unresolved model",
                "family_id": null,
                "canonical_id": null,
                "parameter_count_b": null,
                "endpoint_id": "fixture-model",
                "identity_status": "unresolved",
                "artifact": {
                    "source_kind": "other",
                    "source_id": null,
                    "revision": null,
                    "sha256": null,
                    "format": "unknown",
                    "quantization": null
                }
            },
            "corpus": {
                "id": "wic-50",
                "revision": "v1",
                "sha256": "sha256:fixture",
                "scenario_count": total,
                "scoring_version": "v1"
            },
            "server": {
                "preset_name": "custom",
                "reported_version": null,
                "quirk_flags": [],
                "decode_mode": "unknown",
                "chat_template": null,
                "launch_config_sha256": null
            },
            "environment": {
                "display_label": "Fixture workstation; Fixture OS",
                "os_name": "Fixture OS",
                "os_version": null,
                "architecture": null,
                "accelerator": null,
                "memory_bytes": null
            },
            "sampling": {
                "temperature": 0.0,
                "top_p": 1.0,
                "seed": 42,
                "max_tokens": 1024
            },
            "replication": null,
            "arm_fingerprint": null
        },
        "scenarios": scenarios,
        "totals": {
            "total": total,
            "passed": 0,
            "failed": total,
            "errors": 0,
            "skipped": 0
        }
    });
    fs::write(
        path,
        serde_json::to_vec_pretty(&result).expect("encode v3 result fixture"),
    )
    .expect("write v3 result fixture");
}

fn write_transcript_fixture(
    result_path: &std::path::Path,
    evidence_path: &str,
    body_raw: &str,
    tools: Value,
) {
    let path = result_path
        .parent()
        .expect("result parent")
        .join(evidence_path);
    fs::create_dir_all(path.parent().expect("evidence parent")).expect("evidence directory");
    let transcript = json!({
        "schema_version": 1,
        "run_id": "20260720T120000Z-fixture",
        "scenario_id": "fixture",
        "turns": [{
            "request": {
                "body": {"tools": tools}
            },
            "response": {
                "body_raw": body_raw
            }
        }]
    });
    fs::write(
        path,
        serde_json::to_vec_pretty(&transcript).expect("encode transcript fixture"),
    )
    .expect("write transcript fixture");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn annotate_scenario_adds_a_cause() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![scenario_fixture(
            "empty",
            Some("empty_response"),
            Some("evidence/empty.json"),
        )],
    );

    let output = run_binary(vec![
        "annotate".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
        "--scenario".to_owned(),
        "empty".to_owned(),
        "--cause".to_owned(),
        "server-defect".to_owned(),
        "--reference".to_owned(),
        "https://example.test/issue/1".to_owned(),
        "--note".to_owned(),
        "known server bug".to_owned(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1"));
    let result: Value =
        serde_json::from_slice(&fs::read(result_path).expect("result file")).expect("valid JSON");
    assert_eq!(result["scenarios"][0]["cause"]["kind"], "server-defect");
    assert_eq!(
        result["scenarios"][0]["cause"]["reference"],
        "https://example.test/issue/1"
    );
    assert_eq!(result["scenarios"][0]["cause"]["note"], "known server bug");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn annotate_refuses_a_non_empty_scenario_without_force() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![scenario_fixture(
            "other-failure",
            None,
            Some("evidence/other.json"),
        )],
    );
    let before = fs::read(&result_path).expect("result bytes");

    let output = run_binary(vec![
        "annotate".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
        "--scenario".to_owned(),
        "other-failure".to_owned(),
        "--cause".to_owned(),
        "unknown".to_owned(),
    ])
    .await;

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("empty-response"));
    assert_eq!(fs::read(result_path).expect("result bytes"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn annotate_all_empty_targets_only_empty_response_failures() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![
            scenario_fixture(
                "empty-one",
                Some("empty_response"),
                Some("evidence/one.json"),
            ),
            scenario_fixture(
                "empty-two",
                Some("empty_response"),
                Some("evidence/two.json"),
            ),
            scenario_fixture("other", None, Some("evidence/other.json")),
        ],
    );

    let output = run_binary(vec![
        "annotate".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
        "--all-empty".to_owned(),
        "--cause".to_owned(),
        "unknown".to_owned(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2"));
    let result: Value =
        serde_json::from_slice(&fs::read(result_path).expect("result file")).expect("valid JSON");
    assert_eq!(result["scenarios"][0]["cause"]["kind"], "unknown");
    assert_eq!(result["scenarios"][1]["cause"]["kind"], "unknown");
    assert!(result["scenarios"][2].get("cause").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn annotate_refuses_an_absent_scenario_without_writing() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![scenario_fixture(
            "present",
            Some("empty_response"),
            Some("evidence/present.json"),
        )],
    );
    let before = fs::read(&result_path).expect("result bytes");

    let output = run_binary(vec![
        "annotate".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
        "--scenario".to_owned(),
        "absent".to_owned(),
        "--cause".to_owned(),
        "unknown".to_owned(),
    ])
    .await;

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("absent"));
    assert_eq!(fs::read(result_path).expect("result bytes"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescore_classifies_empty_json_and_sse_responses() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![
            scenario_fixture("empty-json", None, Some("evidence/empty-json.json")),
            scenario_fixture("empty-sse", None, Some("evidence/empty-sse.json")),
        ],
    );
    write_transcript_fixture(
        &result_path,
        "evidence/empty-json.json",
        &completion(json!([]), Value::Null),
        json!([]),
    );
    write_transcript_fixture(
        &result_path,
        "evidence/empty-sse.json",
        "data: {\"choices\":[{\"delta\":{}}]}\n\ndata: [DONE]\n\n",
        json!([]),
    );

    let output = run_binary(vec![
        "rescore".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2"));
    let result: Value =
        serde_json::from_slice(&fs::read(result_path).expect("result file")).expect("valid JSON");
    for scenario in result["scenarios"].as_array().expect("scenarios") {
        assert_eq!(scenario["status"], "fail");
        assert_eq!(scenario["failure_class"], "empty_response");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescore_leaves_a_response_with_content_untouched() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![scenario_fixture(
            "content",
            None,
            Some("evidence/content.json"),
        )],
    );
    write_transcript_fixture(
        &result_path,
        "evidence/content.json",
        &completion(json!([]), json!("answer")),
        json!([]),
    );

    let output = run_binary(vec![
        "rescore".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value =
        serde_json::from_slice(&fs::read(result_path).expect("result file")).expect("valid JSON");
    assert!(result["scenarios"][0].get("failure_class").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescore_classifies_a_valid_unparsed_tool_call() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![scenario_fixture(
            "custom-weather",
            None,
            Some("evidence/custom-weather.json"),
        )],
    );
    write_transcript_fixture(
        &result_path,
        "evidence/custom-weather.json",
        &completion(json!([]), json!(r#"[`get_weather` {"city": "Boston"}]"#)),
        json!([{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {
                    "type": "object",
                    "required": ["city"],
                    "properties": {"city": {"type": "string"}}
                }
            }
        }]),
    );

    let output = run_binary(vec![
        "rescore".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value =
        serde_json::from_slice(&fs::read(result_path).expect("result file")).expect("valid JSON");
    assert_eq!(result["scenarios"][0]["status"], "fail");
    assert_eq!(
        result["scenarios"][0]["failure_class"],
        "unparsed_tool_call"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescore_reports_an_unparseable_transcript_without_writing() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        2,
        vec![scenario_fixture(
            "broken",
            None,
            Some("evidence/broken.json"),
        )],
    );
    write_transcript_fixture(
        &result_path,
        "evidence/broken.json",
        "not JSON or SSE",
        json!([]),
    );
    let before = fs::read(&result_path).expect("result bytes");

    let output = run_binary(vec![
        "rescore".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("broken"), "{stderr}");
    assert!(stderr.contains("parse"), "{stderr}");
    assert_eq!(fs::read(result_path).expect("result bytes"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rescore_leaves_v1_without_evidence_paths_alone() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    write_result_fixture(
        &result_path,
        1,
        vec![scenario_fixture("legacy", None, None)],
    );
    let before = fs::read(&result_path).expect("result bytes");

    let output = run_binary(vec![
        "rescore".to_owned(),
        "--result".to_owned(),
        result_path.display().to_string(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(result_path).expect("result bytes"), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_runs_emit_v3_and_legacy_edits_preserve_their_version() {
    let server = MockServer::start_scripted(
        "qwen2.5:7b-instruct",
        vec![ScriptedResponse::Json(completion(
            json!([]),
            json!("ready"),
        ))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let scenario_path = directory.path().join("scenarios");
    fs::create_dir(&scenario_path).expect("scenario directory");
    fs::write(
        scenario_path.join("v3-run.toml"),
        r#"
id = "v3-run"
category = "negative_trap"
description = "Produce no tool call."
rationale = "This fixture asserts v3 emission; tool_choice none forbids the offered tool."

[[tools]]
name = "get_weather"
description = "Get weather."

[tools.parameters]
type = "object"

[tool_choice]
mode = "none"

[[turns]]
[[turns.messages]]
role = "user"
content = "Reply ready."
"#,
    )
    .expect("write scenario");
    let v3_path = directory.path().join("new-run.json");

    let run = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "qwen2.5:7b-instruct".to_owned(),
        "--force".to_owned(),
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        v3_path.display().to_string(),
    ])
    .await;

    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let v3_bytes = fs::read(&v3_path).expect("v3 result");
    let v3 = parse_and_validate_measurement(&v3_bytes).expect("valid v3 measurement");
    assert_eq!(v3.schema_version, 3);
    assert_eq!(v3.metadata.model.endpoint_id, "qwen2.5:7b-instruct");
    assert_eq!(v3.metadata.model.identity_status, IdentityStatus::Declared);
    assert_eq!(
        v3.metadata
            .corpus
            .as_ref()
            .expect("v3 corpus metadata")
            .scoring_version,
        "v2"
    );
    assert!(v3.metadata.replication.is_none());
    assert!(v3.metadata.arm_fingerprint.is_none());

    let validation = run_binary(vec!["validate".to_owned(), v3_path.display().to_string()]).await;
    assert_eq!(
        validation.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&validation.stderr)
    );

    let v2_path = directory.path().join("legacy-v2.json");
    write_result_fixture(
        &v2_path,
        2,
        vec![
            scenario_fixture(
                "annotate-me",
                Some("empty_response"),
                Some("evidence/annotate.json"),
            ),
            scenario_fixture("rescore-me", None, Some("evidence/rescore.json")),
        ],
    );
    write_transcript_fixture(
        &v2_path,
        "evidence/rescore.json",
        &completion(json!([]), Value::Null),
        json!([]),
    );
    let before: Value =
        serde_json::from_slice(&fs::read(&v2_path).expect("v2 result")).expect("valid v2 JSON");

    let annotation = run_binary(vec![
        "annotate".to_owned(),
        "--result".to_owned(),
        v2_path.display().to_string(),
        "--scenario".to_owned(),
        "annotate-me".to_owned(),
        "--cause".to_owned(),
        "unknown".to_owned(),
    ])
    .await;
    assert_eq!(
        annotation.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&annotation.stderr)
    );
    let annotated: Value = serde_json::from_slice(&fs::read(&v2_path).expect("annotated v2"))
        .expect("valid annotated v2 JSON");
    assert_eq!(annotated["schema_version"], 2);
    assert!(annotated["metadata"].get("model").is_none());

    let rescored = run_binary(vec![
        "rescore".to_owned(),
        "--result".to_owned(),
        v2_path.display().to_string(),
    ])
    .await;
    assert_eq!(
        rescored.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&rescored.stderr)
    );
    let after: Value = serde_json::from_slice(&fs::read(&v2_path).expect("rescored v2"))
        .expect("valid rescored v2 JSON");
    assert_eq!(after["schema_version"], 2);
    assert!(after["metadata"].get("model").is_none());

    let mut expected = before;
    expected["scenarios"][0]["cause"] = json!({
        "kind": "unknown",
        "reference": null,
        "note": null
    });
    expected["scenarios"][1]["failure_class"] = json!("empty_response");
    assert_eq!(after, expected, "only the requested v2 fields may change");

    let site_output = directory.path().join("site");
    let site = run_binary(vec![
        "site".to_owned(),
        "--results".to_owned(),
        directory.path().display().to_string(),
        "--out".to_owned(),
        site_output.display().to_string(),
    ])
    .await;
    assert_eq!(
        site.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&site.stderr)
    );
    let index = fs::read_to_string(site_output.join("index.html")).expect("generated site");
    assert!(index.contains("Schema</dt><dd>v2"));
    assert!(index.contains("Schema</dt><dd>v3"));
}

fn write_m1a_scenarios(path: &std::path::Path) {
    fs::create_dir(path).expect("scenario directory");
    for (name, contents) in [
        (
            "multi-turn-route.toml",
            include_str!("../../wic-core/scenarios/multi-turn-route.toml"),
        ),
        (
            "negative-greeting.toml",
            include_str!("../../wic-core/scenarios/negative-greeting.toml"),
        ),
        (
            "parallel-city-time.toml",
            include_str!("../../wic-core/scenarios/parallel-city-time.toml"),
        ),
        (
            "single-weather.toml",
            include_str!("../../wic-core/scenarios/single-weather.toml"),
        ),
        (
            "streaming-weather.toml",
            include_str!("../../wic-core/scenarios/streaming-weather.toml"),
        ),
    ] {
        fs::write(path.join(name), contents).expect("write scenario");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_run_passes_all_scenarios_and_writes_a_valid_result() {
    let responses = vec![
        ScriptedResponse::Json(completion(
            json!([call(
                "call-geocode",
                "geocode",
                r#"{"place":"Fenway Park"}"#
            )]),
            Value::Null,
        )),
        ScriptedResponse::Json(completion(
            json!([call(
                "call-route",
                "get_route",
                r#"{"latitude":42.3467,"longitude":-71.0972}"#
            )]),
            Value::Null,
        )),
        ScriptedResponse::Json(completion(json!([]), json!("Hello."))),
        ScriptedResponse::Json(completion(
            json!([
                call("call-tokyo", "get_time", r#"{"city":"Tokyo"}"#),
                call("call-boston", "get_time", r#"{"city":"Boston"}"#)
            ]),
            Value::Null,
        )),
        ScriptedResponse::Json(completion(
            json!([call("call-weather", "get_weather", r#"{"city":"Boston"}"#)]),
            Value::Null,
        )),
        ScriptedResponse::Sse(streaming_call(
            "call-stream",
            "get_weather",
            "{\"city\":\"",
            r#"Seattle"}"#,
        )),
    ];
    let server = MockServer::start_scripted("fixture-model", responses).await;
    let directory = tempfile::tempdir().expect("temp directory");
    let output_path = directory.path().join("result.json");
    let scenario_path = directory.path().join("scenarios");
    write_m1a_scenarios(&scenario_path);

    let output = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--force".to_owned(),
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    let document = fs::read(&output_path).expect("result file");
    let result = parse_and_validate_measurement(&document).expect("schema-valid result");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {} outcomes: {:?}",
        String::from_utf8_lossy(&output.stderr),
        result.scenarios
    );
    assert_eq!(result.schema_version, 3);
    assert!(result.metadata.model.artifact.quantization.is_none());
    assert_eq!(
        result.metadata.model.identity_status,
        IdentityStatus::Unresolved
    );
    assert_eq!(result.metadata.sampling.seed, Some(42));
    assert_eq!(result.metadata.sampling.temperature, Some(0.0));
    assert_eq!(result.totals.passed, 5);
    assert_eq!(result.totals.failed, 0);
    assert!(result
        .scenarios
        .iter()
        .all(|outcome| outcome.status == Status::Pass));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("multi_turn        1 passed  0 failed  0 errors"));
    assert!(report.contains("TOTAL 5 passed  0 failed  0 errors  0 skipped  5 total"));
    assert!(
        !report.contains('\u{1b}'),
        "non-TTY output must not contain color"
    );

    let requests = server.requests();
    let second_turn_messages = requests[1]["messages"].as_array().expect("messages array");
    assert!(second_turn_messages
        .iter()
        .any(|message| { message["role"] == "tool" && message["tool_call_id"] == "call-geocode" }));

    let validation = run_binary(vec![
        "validate".to_owned(),
        output_path.display().to_string(),
    ])
    .await;
    assert_eq!(
        validation.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&validation.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn json_mode_is_clean_json_and_records_the_selected_preset() {
    let server = MockServer::start_scripted(
        "fixture-model",
        (0..4)
            .map(|_| ScriptedResponse::Json(completion(json!([]), json!("ready"))))
            .collect(),
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let scenario_path = directory.path().join("scenarios");
    fs::create_dir(&scenario_path).expect("scenario directory");
    fs::write(
        scenario_path.join("json-clean.toml"),
        r#"
id = "json-clean"
category = "negative_trap"
description = "Produce no tool call."
rationale = "This fixture asserts a no-call result; tool_choice none forbids the only offered tool."

[[tools]]
name = "get_weather"
description = "Get weather."

[tools.parameters]
type = "object"

[tool_choice]
mode = "none"

[[turns]]
[[turns.messages]]
role = "user"
content = "Reply ready."
"#,
    )
    .expect("write scenario");
    let output_path = directory.path().join("result.json");

    let output = run_binary(vec![
        "run".to_owned(),
        "--server".to_owned(),
        "ollama".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--host-hardware-class".to_owned(),
        "Fixture workstation, 32GB".to_owned(),
        "--quant".to_owned(),
        "Q4_K_M-imatrix".to_owned(),
        "--seed".to_owned(),
        "8675309".to_owned(),
        "--temperature".to_owned(),
        "0.75".to_owned(),
        "--force".to_owned(),
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
        "--json".to_owned(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result =
        parse_and_validate_measurement(&output.stdout).expect("stdout is schema-valid JSON");
    assert_eq!(result.metadata.server.preset_name, "ollama");
    assert_eq!(
        result
            .metadata
            .model
            .artifact
            .quantization
            .as_ref()
            .map(|quantization| quantization.label.as_str()),
        Some("Q4_K_M-imatrix")
    );
    assert_eq!(result.metadata.sampling.seed, Some(8675309));
    assert_eq!(result.metadata.sampling.temperature, Some(0.75));
    assert_eq!(
        result.metadata.server.reported_version.as_deref(),
        Some("mock-1.0")
    );
    assert_eq!(
        result.metadata.server.quirk_flags,
        ["unconstrained_post_hoc_parse"]
    );
    let environment = result
        .metadata
        .environment
        .as_ref()
        .expect("measurement environment");
    assert!(environment
        .display_label
        .starts_with("Fixture workstation, 32GB; "));
    assert_eq!(
        fs::read(&output_path).expect("result file"),
        output.stdout,
        "stdout and --out should contain the same result document"
    );

    // The CLI value and the recorded preset name differ for mlx-lm: the flag follows
    // clap's kebab-case derive, the result field records the package's own spelling.
    for (preset, recorded_name, expected_quirks) in [
        ("llamacpp", "llamacpp", vec!["grammar_constrained_decoding"]),
        ("mlx-lm", "mlx_lm", vec!["unconstrained_post_hoc_parse"]),
        ("custom", "custom", Vec::new()),
    ] {
        let output = run_binary(vec![
            "run".to_owned(),
            "--server".to_owned(),
            preset.to_owned(),
            "--endpoint".to_owned(),
            server.endpoint(),
            "--model".to_owned(),
            "fixture-model".to_owned(),
            "--host-hardware-class".to_owned(),
            "Fixture workstation, 32GB".to_owned(),
            "--force".to_owned(),
            "--scenarios".to_owned(),
            scenario_path.display().to_string(),
            "--out".to_owned(),
            output_path.display().to_string(),
            "--json".to_owned(),
        ])
        .await;

        assert_eq!(output.status.code(), Some(0));
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result =
            parse_and_validate_measurement(&output.stdout).expect("stdout is schema-valid JSON");
        assert_eq!(result.metadata.server.preset_name, recorded_name);
        assert_eq!(result.metadata.server.quirk_flags, expected_quirks);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn validate_directory_ignores_archive() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let archive = results.join("archive");
    fs::create_dir_all(&archive).expect("archive directory");
    let published = results.join("published.json");
    write_result_fixture(&published, 2, Vec::new());
    fs::write(archive.join("invalid.json"), b"not JSON").expect("archived fixture");

    let output = run_binary(vec!["validate".to_owned(), results.display().to_string()]).await;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("valid: {}", published.display())));
    assert!(!stdout.contains("archive"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn validate_accepts_v1_v2_and_v3_fixtures() {
    let directory = tempfile::tempdir().expect("temp directory");
    let v1_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../results/ollama-qwen2.5-7b-instruct.json");
    let v2_path = directory.path().join("v2.json");
    fs::write(
        &v2_path,
        r#"{
  "schema_version": 2,
  "metadata": {
    "run_id": "20260719T120000Z-1234abcd",
    "timestamp": "2026-07-19T12:00:00Z",
    "willitcall_version": "0.1.0",
    "endpoint": "http://127.0.0.1:8080/v1",
    "model_id": "fixture-model",
    "declared_quant": null,
    "server": {
      "preset_name": "custom",
      "reported_version": null,
      "quirk_flags": []
    },
    "sampling": {
      "temperature": 0.0,
      "top_p": 1.0,
      "seed": 42,
      "max_tokens": 1024
    }
  },
  "scenarios": [],
  "totals": {
    "total": 0,
    "passed": 0,
    "failed": 0,
    "errors": 0,
    "skipped": 0
  }
}"#,
    )
    .expect("write v2 result");

    let v3_path = directory.path().join("v3.json");
    write_v3_result_fixture(&v3_path, Vec::new());

    for fixture in [&v1_path, &v2_path, &v3_path] {
        let output = run_binary(vec!["validate".to_owned(), fixture.display().to_string()]).await;
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let unsupported_path = directory.path().join("v4.json");
    let mut unsupported: Value =
        serde_json::from_slice(&fs::read(&v2_path).expect("v2 bytes")).expect("v2 JSON");
    unsupported["schema_version"] = json!(4);
    fs::write(
        &unsupported_path,
        serde_json::to_vec_pretty(&unsupported).expect("encode unsupported result"),
    )
    .expect("write unsupported result");
    let output = run_binary(vec![
        "validate".to_owned(),
        unsupported_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported schema_version 4; expected 1, 2, or 3"),
        "{stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn validate_rejects_a_corrupt_document_with_a_nonzero_exit() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("corrupt.json");
    write_v3_result_fixture(&path, Vec::new());
    let mut corrupt: Value =
        serde_json::from_slice(&fs::read(&path).expect("v3 bytes")).expect("v3 JSON");
    corrupt["totals"]["total"] = json!(1);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&corrupt).expect("encode corrupt result"),
    )
    .expect("write corrupt result");

    let output = run_binary(vec!["validate".to_owned(), path.display().to_string()]).await;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("totals.total is 1 but scenarios contains 0 outcomes"),
        "{stderr}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn known_broken_double_encoded_arguments_are_red_with_a_precise_reason() {
    let encoded_object = serde_json::to_string(&json!({"city": "Boston"})).expect("encode object");
    let double_encoded = serde_json::to_string(&encoded_object).expect("double encode object");
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(
            json!([call("call-broken", "get_weather", &double_encoded)]),
            Value::Null,
        ))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let scenario_path = directory.path().join("scenarios");
    fs::create_dir(&scenario_path).expect("scenario directory");
    fs::write(
        scenario_path.join("broken.toml"),
        r#"
id = "known-broken-double-encoded"
category = "single_call"
description = "Detect double encoded arguments."
rationale = "This fixture asserts schema rejection of a double encoded call, while Boston is literal in the prompt."

[[tools]]
name = "get_weather"
description = "Get weather."

[tools.parameters]
type = "object"
required = ["city"]

[tools.parameters.properties.city]
type = "string"

[tool_choice]
mode = "auto"

[[turns]]
[[turns.messages]]
role = "user"
content = "Weather in Boston?"

[[turns.expected_calls]]
name = "get_weather"

[turns.expected_calls.arguments]
city = "Boston"
"#,
    )
    .expect("write scenario");
    let output_path = directory.path().join("result.json");

    let output = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--force".to_owned(),
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result = parse_and_validate_measurement(&fs::read(output_path).expect("result file"))
        .expect("valid result");
    assert_eq!(result.scenarios[0].status, Status::Fail);
    assert_eq!(
        result.scenarios[0].failure_reason.as_deref(),
        Some("schema violation: expected object, got string")
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(
        "FAIL  known-broken-double-encoded: schema violation: expected object, got string"
    ));
    assert_eq!(
        server.requests().len(),
        1,
        "parsed bad answers are not retried"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_error_is_retried_once_and_recorded_in_the_outcome() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![
            ScriptedResponse::Status(500, "injected failure".to_owned()),
            ScriptedResponse::Json(completion(json!([]), json!("Hello."))),
        ],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let scenario_path = directory.path().join("scenarios");
    fs::create_dir(&scenario_path).expect("scenario directory");
    fs::write(
        scenario_path.join("retry.toml"),
        r#"
id = "retry-negative"
category = "negative_trap"
description = "Retry a transient server failure."
rationale = "This fixture asserts retry behavior; tool_choice none pins the successful response to no tool calls."

[[tools]]
name = "get_weather"
description = "Get weather."

[tools.parameters]
type = "object"

[tool_choice]
mode = "none"

[[turns]]
[[turns.messages]]
role = "user"
content = "Say hello."
"#,
    )
    .expect("write scenario");
    let output_path = directory.path().join("result.json");

    let output = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--force".to_owned(),
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(0));
    let result = parse_and_validate_measurement(&fs::read(output_path).expect("result file"))
        .expect("valid result");
    assert!(result.scenarios[0].retried);
    let evidence_path = directory.path().join(
        result.scenarios[0]
            .evidence_path
            .as_deref()
            .expect("evidence path"),
    );
    let transcript: Value =
        serde_json::from_slice(&fs::read(evidence_path).expect("transcript file"))
            .expect("valid transcript");
    assert_eq!(transcript["turns"].as_array().expect("turns").len(), 2);
    assert_eq!(transcript["turns"][0]["retried"], false);
    assert_eq!(transcript["turns"][1]["retried"], true);
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn result_write_failure_leaves_valid_transcripts_without_partial_result() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(
            json!([]),
            json!("Hello."),
        ))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let scenario_path = directory.path().join("scenarios");
    fs::create_dir(&scenario_path).expect("scenario directory");
    fs::write(
        scenario_path.join("ordering.toml"),
        r#"
id = "ordering-negative"
category = "negative_trap"
description = "Write evidence before publishing the result."
rationale = "This fixture asserts write ordering; tool_choice none requires no tool call."

[[tools]]
name = "get_weather"
description = "Get weather."

[tools.parameters]
type = "object"

[tool_choice]
mode = "none"

[[turns]]
[[turns.messages]]
role = "user"
content = "Say hello."
"#,
    )
    .expect("write scenario");
    let output_path = directory.path().join("result.json");
    fs::create_dir(&output_path).expect("blocking destination directory");

    let output = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--force".to_owned(),
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(4));
    assert!(
        !output_path.is_file(),
        "no partial result file is published"
    );
    let evidence_root = directory.path().join("evidence");
    let run_directory = fs::read_dir(evidence_root)
        .expect("evidence root")
        .next()
        .expect("run directory")
        .expect("run directory entry")
        .path();
    let transcript: Value = serde_json::from_slice(
        &fs::read(run_directory.join("ordering-negative.json")).expect("transcript bytes"),
    )
    .expect("valid transcript JSON");
    assert_eq!(transcript["schema_version"], 1);
    assert_eq!(transcript["scenario_id"], "ordering-negative");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn negative_temperature_exits_two() {
    let output = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        "http://127.0.0.1:1/v1".to_owned(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--temperature".to_owned(),
        "-0.1".to_owned(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("temperature must be non-negative"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preflight_failure_exits_three_without_a_result_file() {
    let server = MockServer::start_preflight_failure().await;
    let directory = tempfile::tempdir().expect("temp directory");
    let output_path = directory.path().join("result.json");

    let output = run_binary(vec![
        "run".to_owned(),
        "--endpoint".to_owned(),
        server.endpoint(),
        "--model".to_owned(),
        "fixture-model".to_owned(),
        "--force".to_owned(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("preflight"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output_path.exists());
}
