#[path = "support/mod.rs"]
mod support;

use std::fs;
use std::time::Duration;

use reqwest::header::{HeaderValue, AUTHORIZATION};
use ring::digest::{digest, SHA256};
use serde_json::{json, Value};
use support::{MockServer, ScriptedResponse};
use wic_core::result::write_result_atomic;
use wic_core::runner::{run_scenarios, RunConfig};
use wic_core::{load_embedded_scenarios, Scenario};

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

fn embedded_scenario(id: &str) -> Scenario {
    load_embedded_scenarios()
        .expect("embedded scenarios")
        .into_iter()
        .find(|scenario| scenario.id == id)
        .expect("named scenario")
}

fn evidence_file(result_path: &std::path::Path, evidence_path: &str) -> std::path::PathBuf {
    result_path
        .parent()
        .expect("result parent")
        .join(evidence_path)
}

#[tokio::test]
async fn written_transcript_redacts_sensitive_request_headers() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(
            json!([call("call-weather", "get_weather", r#"{"city":"Boston"}"#)]),
            Value::Null,
        ))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("results/result.json");
    let mut config = RunConfig::new(
        server.endpoint(),
        "fixture-model".to_owned(),
        Duration::from_secs(5),
        42,
        0.0,
    );
    config.request_headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer sk-secret-value"),
    );

    let result = run_scenarios(
        &config,
        &[embedded_scenario("single-weather")],
        &result_path,
    )
    .await
    .expect("run scenario");
    let path = evidence_file(
        &result_path,
        result.scenarios[0]
            .evidence_path
            .as_deref()
            .expect("evidence path"),
    );
    let bytes = fs::read(path).expect("transcript bytes");

    assert!(!bytes
        .windows(b"sk-secret-value".len())
        .any(|window| window == b"sk-secret-value"));
    assert!(!bytes
        .windows(b"Bearer".len())
        .any(|window| window == b"Bearer"));
    assert!(String::from_utf8(bytes)
        .expect("JSON transcript is UTF-8")
        .contains("\"authorization\": \"[REDACTED]\""));
}

#[tokio::test]
async fn written_transcript_preserves_sse_body_bytes_exactly() {
    let body = streaming_call("call-stream", "get_weather", "{\"city\":\"", r#"Seattle"}"#);
    let server =
        MockServer::start_scripted("fixture-model", vec![ScriptedResponse::Sse(body.clone())])
            .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    let config = RunConfig::new(
        server.endpoint(),
        "fixture-model".to_owned(),
        Duration::from_secs(5),
        42,
        0.0,
    );

    let result = run_scenarios(
        &config,
        &[embedded_scenario("streaming-weather")],
        &result_path,
    )
    .await
    .expect("run scenario");
    let transcript: Value = serde_json::from_slice(
        &fs::read(evidence_file(
            &result_path,
            result.scenarios[0]
                .evidence_path
                .as_deref()
                .expect("evidence path"),
        ))
        .expect("transcript bytes"),
    )
    .expect("transcript JSON");

    assert_eq!(transcript["turns"][0]["response"]["body_raw"], body);
    assert!(transcript["turns"][0]["response"]
        .get("body_raw_hex")
        .is_none());
}

#[tokio::test]
async fn fresh_result_paths_exist_and_hash_transcript_bytes() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(
            json!([call("call-weather", "get_weather", r#"{"city":"Boston"}"#)]),
            Value::Null,
        ))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("nested/result.json");
    let config = RunConfig::new(
        server.endpoint(),
        "fixture-model".to_owned(),
        Duration::from_secs(5),
        42,
        0.0,
    );

    let result = run_scenarios(
        &config,
        &[embedded_scenario("single-weather")],
        &result_path,
    )
    .await
    .expect("run scenario");
    write_result_atomic(&result_path, &result).expect("write result");
    let written: wic_core::result::RunResult =
        serde_json::from_slice(&fs::read(&result_path).expect("result bytes"))
            .expect("result JSON");

    for outcome in &written.scenarios {
        let path = evidence_file(
            &result_path,
            outcome.evidence_path.as_deref().expect("evidence path"),
        );
        let bytes = fs::read(path).expect("referenced transcript exists");
        let expected = format!("sha256:{}", hex(digest(&SHA256, &bytes).as_ref()));
        assert_eq!(outcome.evidence_hash.as_deref(), Some(expected.as_str()));
    }
}

#[tokio::test]
async fn written_transcript_matches_checked_in_schema() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(
            json!([call("call-weather", "get_weather", r#"{"city":"Boston"}"#)]),
            Value::Null,
        ))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("result.json");
    let config = RunConfig::new(
        server.endpoint(),
        "fixture-model".to_owned(),
        Duration::from_secs(5),
        42,
        0.0,
    );

    let result = run_scenarios(
        &config,
        &[embedded_scenario("single-weather")],
        &result_path,
    )
    .await
    .expect("run scenario");
    let document: Value = serde_json::from_slice(
        &fs::read(evidence_file(
            &result_path,
            result.scenarios[0]
                .evidence_path
                .as_deref()
                .expect("evidence path"),
        ))
        .expect("transcript bytes"),
    )
    .expect("transcript JSON");
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/transcript-v1.schema.json"))
            .expect("transcript schema");
    let validator = jsonschema::validator_for(&schema).expect("compile transcript schema");

    validator
        .validate(&document)
        .expect("written transcript should satisfy checked-in schema");
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
