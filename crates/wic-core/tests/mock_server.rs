mod support;

use support::MockServer;

const JSON_FIXTURE: &str = include_str!("fixtures/chat-completion.json");
const SSE_FIXTURE: &str = include_str!("fixtures/chat-completion.sse");
const MALFORMED_FIXTURE: &str = include_str!("fixtures/malformed-tool-call.json");

#[tokio::test]
async fn mock_server_replays_json_fixture_intact() {
    let server = MockServer::start().await;
    let response = reqwest::Client::new()
        .post(server.chat_completions_url())
        .json(&serde_json::json!({"model": "fixture-model", "stream": false}))
        .send()
        .await
        .expect("request JSON fixture");

    assert!(response.status().is_success());
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .expect("content type"),
        "application/json"
    );
    assert_eq!(response.text().await.expect("response body"), JSON_FIXTURE);
}

#[tokio::test]
async fn mock_server_replays_sse_fixture_intact() {
    let server = MockServer::start().await;
    let response = reqwest::Client::new()
        .post(server.chat_completions_url())
        .json(&serde_json::json!({"model": "fixture-model", "stream": true}))
        .send()
        .await
        .expect("request SSE fixture");

    assert!(response.status().is_success());
    assert!(response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .expect("content type")
        .to_str()
        .expect("ASCII content type")
        .starts_with("text/event-stream"));
    assert_eq!(response.text().await.expect("response body"), SSE_FIXTURE);
}

#[test]
fn known_broken_fixture_contains_malformed_tool_arguments() {
    let fixture: serde_json::Value =
        serde_json::from_str(MALFORMED_FIXTURE).expect("valid response envelope");
    let arguments = fixture["choices"][0]["message"]["tool_calls"][0]["function"]
        ["arguments"]
        .as_str()
        .expect("arguments string");

    assert!(serde_json::from_str::<serde_json::Value>(arguments).is_err());
}
