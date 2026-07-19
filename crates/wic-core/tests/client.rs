use serde_json::json;
use wic_core::client::{
    build_request_payload, parse_non_streaming, parse_sse_data, reassemble_sse_payloads,
};
use wic_core::load_embedded_scenarios;
use wic_core::result::SamplingParams;

const STANDARD_STREAM: &[u8] = include_bytes!("fixtures/chat-completion.sse");
const SPLIT_ARGUMENT_STREAM: &[u8] = include_bytes!("fixtures/stream-mid-argument.sse");
const MALFORMED_STREAM: &[u8] = include_bytes!("fixtures/malformed-stream.sse");

#[test]
fn reassembles_recorded_stream_fixture() {
    let payloads = parse_sse_data(STANDARD_STREAM).expect("valid SSE events");
    let response = reassemble_sse_payloads(&payloads).expect("valid stream payloads");

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id.as_deref(), Some("call_weather_2"));
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].arguments, r#"{"city":"Seattle"}"#);
}

#[test]
fn streaming_reassembly_equals_the_equivalent_non_streaming_parse() {
    let payloads = parse_sse_data(STANDARD_STREAM).expect("valid SSE events");
    let streaming = reassemble_sse_payloads(&payloads).expect("valid stream payloads");
    let non_streaming = parse_non_streaming(
        br#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"call_weather_2","function":{"name":"get_weather","arguments":"{\"city\":\"Seattle\"}"}}]}}]}"#,
    )
    .expect("valid non-streaming response");

    assert_eq!(streaming, non_streaming);
}

#[test]
fn reassembles_missing_indices_and_mid_argument_splits_byte_exactly() {
    let payloads = parse_sse_data(SPLIT_ARGUMENT_STREAM).expect("valid SSE events");
    let response = reassemble_sse_payloads(&payloads).expect("valid stream payloads");

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id.as_deref(), Some("call_weather_split"));
    assert_eq!(response.tool_calls[0].arguments.as_bytes(), br#"{"city":"Boston"}"#);
}

#[test]
fn malformed_stream_payload_returns_a_specific_error() {
    let payloads = parse_sse_data(MALFORMED_STREAM).expect("valid SSE framing");
    let error = reassemble_sse_payloads(&payloads).expect_err("malformed JSON must fail");

    assert!(error.contains("invalid SSE data JSON"), "{error}");
}

#[test]
fn request_payload_contains_tools_choice_messages_stream_and_sampling() {
    let scenario = load_embedded_scenarios()
        .expect("embedded scenarios")
        .into_iter()
        .find(|scenario| scenario.id == "streaming-weather")
        .expect("streaming scenario");
    let payload = build_request_payload(
        &scenario,
        "fixture-model",
        &[json!({"role": "user", "content": "Use get_weather."})],
        &SamplingParams {
            temperature: Some(0.0),
            top_p: Some(1.0),
            seed: Some(42),
            max_tokens: Some(1024),
        },
    );

    assert_eq!(payload["model"], "fixture-model");
    assert_eq!(payload["stream"], true);
    assert_eq!(payload["messages"][0]["role"], "user");
    assert_eq!(payload["tools"][0]["type"], "function");
    assert_eq!(payload["tools"][0]["function"]["name"], "get_weather");
    assert_eq!(payload["tool_choice"]["function"]["name"], "get_weather");
    assert_eq!(payload["temperature"], 0.0);
    assert_eq!(payload["top_p"], 1.0);
    assert_eq!(payload["seed"], 42);
    assert_eq!(payload["max_tokens"], 1024);
}
