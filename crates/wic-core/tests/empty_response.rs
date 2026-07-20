#[path = "support/mod.rs"]
mod support;

use std::time::Duration;

use serde_json::{json, Value};
use support::{MockServer, ScriptedResponse};
use wic_core::runner::{run_scenarios, RunConfig};
use wic_core::{load_embedded_scenarios, Scenario};

fn completion(content: Value) -> String {
    json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": content,
                "tool_calls": [],
            }
        }]
    })
    .to_string()
}

fn single_weather() -> Scenario {
    load_embedded_scenarios()
        .expect("embedded scenarios")
        .into_iter()
        .find(|scenario| scenario.id == "single-weather")
        .expect("single-weather scenario")
}

fn negative_greeting() -> Scenario {
    load_embedded_scenarios()
        .expect("embedded scenarios")
        .into_iter()
        .find(|scenario| scenario.id == "negative-greeting")
        .expect("negative-greeting scenario")
}

#[tokio::test]
async fn runner_classifies_only_empty_responses() {
    for (content, expected_class) in [
        (Value::Null, Some("empty_response")),
        (json!(""), Some("empty_response")),
        (json!("I cannot call that tool."), None),
    ] {
        let server = MockServer::start_scripted(
            "fixture-model",
            vec![ScriptedResponse::Json(completion(content))],
        )
        .await;
        let directory = tempfile::tempdir().expect("temp directory");
        let config = RunConfig::new(
            server.endpoint(),
            "fixture-model".to_owned(),
            Duration::from_secs(5),
        );

        let result = run_scenarios(
            &config,
            &[single_weather()],
            &directory.path().join("result.json"),
        )
        .await
        .expect("run scenario");
        let outcome = &result.scenarios[0];

        assert_eq!(outcome.status, wic_core::result::Status::Fail);
        assert_eq!(outcome.failure_class.as_deref(), expected_class);
        assert!(outcome.cause.is_none());
    }
}

#[tokio::test]
async fn empty_response_preserves_negative_trap_pass() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(Value::Null))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let config = RunConfig::new(
        server.endpoint(),
        "fixture-model".to_owned(),
        Duration::from_secs(5),
    );

    let result = run_scenarios(
        &config,
        &[negative_greeting()],
        &directory.path().join("result.json"),
    )
    .await
    .expect("run scenario");

    assert_eq!(result.scenarios[0].status, wic_core::result::Status::Pass);
}

#[tokio::test]
async fn runner_classifies_unparsed_tool_calls_without_changing_status() {
    let server = MockServer::start_scripted(
        "fixture-model",
        vec![ScriptedResponse::Json(completion(json!(
            r#"<tool_call>[{"arguments":{"city":"Boston"},"name":"get_weather"}]"#
        )))],
    )
    .await;
    let directory = tempfile::tempdir().expect("temp directory");
    let config = RunConfig::new(
        server.endpoint(),
        "fixture-model".to_owned(),
        Duration::from_secs(5),
    );

    let result = run_scenarios(
        &config,
        &[single_weather()],
        &directory.path().join("result.json"),
    )
    .await
    .expect("run scenario");

    assert_eq!(result.scenarios[0].status, wic_core::result::Status::Fail);
    assert_eq!(
        result.scenarios[0].failure_class.as_deref(),
        Some("unparsed_tool_call")
    );
}
