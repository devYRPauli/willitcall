#[path = "../../wic-core/tests/support/mod.rs"]
mod support;

use std::fs;
use std::process::Command;

use serde_json::{Value, json};
use support::{MockServer, ScriptedResponse};
use wic_core::result::{RunResult, Status};

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
            json!([call(
                "call-weather",
                "get_weather",
                r#"{"city":"Boston"}"#
            )]),
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
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    let document = fs::read(&output_path).expect("result file");
    let result: RunResult = serde_json::from_slice(&document).expect("schema-valid result");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {} outcomes: {:?}",
        String::from_utf8_lossy(&output.stderr),
        result.scenarios
    );
    assert_eq!(result.schema_version, 1);
    assert_eq!(result.totals.passed, 5);
    assert_eq!(result.totals.failed, 0);
    assert!(result.scenarios.iter().all(|outcome| outcome.status == Status::Pass));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("multi_turn        1 passed  0 failed  0 errors"));
    assert!(report.contains("TOTAL 5 passed  0 failed  0 errors  0 skipped  5 total"));
    assert!(!report.contains('\u{1b}'), "non-TTY output must not contain color");

    let requests = server.requests();
    let second_turn_messages = requests[1]["messages"].as_array().expect("messages array");
    assert!(second_turn_messages.iter().any(|message| {
        message["role"] == "tool" && message["tool_call_id"] == "call-geocode"
    }));

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
        vec![ScriptedResponse::Json(completion(json!([]), json!("ready")))],
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
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
        "--json".to_owned(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    let result: RunResult = serde_json::from_slice(&output.stdout).expect("stdout is only JSON");
    assert_eq!(result.metadata.server.preset_name, "ollama");
    assert_eq!(
        result.metadata.server.reported_version.as_deref(),
        Some("mock-1.0")
    );
    assert!(result.metadata.server.quirk_flags.is_empty());
    assert_eq!(
        fs::read(&output_path).expect("result file"),
        output.stdout,
        "stdout and --out should contain the same result document"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn validator_rejects_a_wrong_schema_version_with_a_precise_message() {
    let directory = tempfile::tempdir().expect("temp directory");
    let result_path = directory.path().join("invalid.json");
    fs::write(
        &result_path,
        r#"{
  "schema_version": 2,
  "metadata": {
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
      "max_tokens": 256
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
    .expect("write invalid result");

    let output = run_binary(vec![
        "validate".to_owned(),
        result_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported schema_version 2; expected 1"),
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
            json!([call(
                "call-broken",
                "get_weather",
                &double_encoded
            )]),
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
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    let result: RunResult =
        serde_json::from_slice(&fs::read(output_path).expect("result file")).expect("valid result");
    assert_eq!(result.scenarios[0].status, Status::Fail);
    assert_eq!(
        result.scenarios[0].failure_reason.as_deref(),
        Some("schema violation: expected object, got string")
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(
            "FAIL  known-broken-double-encoded: schema violation: expected object, got string"
        )
    );
    assert_eq!(server.requests().len(), 1, "parsed bad answers are not retried");
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
        "--scenarios".to_owned(),
        scenario_path.display().to_string(),
        "--out".to_owned(),
        output_path.display().to_string(),
    ])
    .await;

    assert_eq!(output.status.code(), Some(0));
    let result: RunResult =
        serde_json::from_slice(&fs::read(output_path).expect("result file")).expect("valid result");
    assert!(result.scenarios[0].retried);
    assert_eq!(server.requests().len(), 2);
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
