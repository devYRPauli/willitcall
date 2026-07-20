use serde_json::json;
use wic_core::client::ToolCall;
use wic_core::result::Status;
use wic_core::score::{classify_failure, score_calls, score_response};
use wic_core::{ArgumentsMatch, ExpectedCall, ToolDefinition};

fn weather_tool() -> ToolDefinition {
    ToolDefinition {
        name: "get_weather".to_owned(),
        description: "Get weather".to_owned(),
        parameters: json!({
            "type": "object",
            "required": ["city"],
            "properties": {
                "city": {"type": "string"},
                "days": {"type": "array", "items": {"type": "number"}},
                "options": {
                    "type": "object",
                    "required": ["units"],
                    "properties": {"units": {"type": "string", "enum": ["c", "f"]}}
                }
            }
        }),
    }
}

fn expected(name: &str, arguments: serde_json::Value) -> ExpectedCall {
    ExpectedCall {
        name: name.to_owned(),
        arguments,
        arguments_match: None,
    }
}

fn actual(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: Some(format!("call-{name}")),
        name: name.to_owned(),
        arguments: arguments.to_owned(),
    }
}

#[test]
fn negative_trap_passes_without_calls_and_rejects_a_call() {
    let tools = [weather_tool()];
    assert!(score_calls(&tools, &[], ArgumentsMatch::Exact, &[]).is_ok());

    let error = score_calls(
        &tools,
        &[],
        ArgumentsMatch::Exact,
        &[actual("get_weather", r#"{"city":"Boston"}"#)],
    )
    .expect_err("negative trap must reject calls");
    assert_eq!(error, "unexpected tool call emitted: 'get_weather'");
}

#[test]
fn required_call_reports_when_no_call_was_emitted() {
    let error = score_calls(
        &[weather_tool()],
        &[expected("get_weather", json!({"city": "Boston"}))],
        ArgumentsMatch::Exact,
        &[],
    )
    .expect_err("missing call must fail");

    assert_eq!(error, "no tool call emitted");
}

#[test]
fn empty_response_is_distinct_from_text_without_a_tool_call() {
    let tools = [weather_tool()];
    let expected = [expected("get_weather", json!({"city": "Boston"}))];

    for content in [None, Some("")] {
        let failure = score_response(&tools, &expected, ArgumentsMatch::Exact, content, &[])
            .expect_err("empty response must fail");
        assert_eq!(
            failure.reason,
            "empty response: no content and no tool call"
        );
        assert_eq!(failure.failure_class.as_deref(), Some("empty_response"));
    }

    let failure = score_response(
        &tools,
        &expected,
        ArgumentsMatch::Exact,
        Some("I cannot call that tool."),
        &[],
    )
    .expect_err("missing call must fail");
    assert_eq!(failure.reason, "no tool call emitted");
    assert_eq!(failure.failure_class, None);
}

#[test]
fn empty_response_preserves_a_negative_trap_pass() {
    assert!(score_response(&[weather_tool()], &[], ArgumentsMatch::Exact, None, &[]).is_ok());
}

#[test]
fn unparsed_tool_call_shapes_classify_valid_offered_calls() {
    let tools = [weather_tool()];
    let expected = [expected("get_weather", json!({"city": "Boston"}))];

    for content in [
        r#"<tool_call>[{"arguments":{"city":"Boston"},"name":"get_weather"}]"#,
        r#"<tool_call>{"name":"get_weather","arguments":{"city":"Boston"}}"#,
        r#"[`get_weather` {"city": "Boston"}]"#,
        r#"`get_weather` {"city": "Boston"}"#,
    ] {
        let failure = score_response(&tools, &expected, ArgumentsMatch::Exact, Some(content), &[])
            .expect_err("unparsed call must remain a failing verdict");
        assert_eq!(
            failure.failure_class.as_deref(),
            Some("unparsed_tool_call"),
            "{content}"
        );
    }
}

#[test]
fn unparsed_tool_call_near_misses_remain_plain_failures() {
    let tools = [weather_tool()];
    let expected = [expected("get_weather", json!({"city": "Boston"}))];

    for content in [
        r#"<tool_call>[{"arguments":{"city":"Boston"},"name":"get_weather"}"#,
        r#"<tool_call>[{"arguments":{"city":"Boston"},"name":"get_forecast"}]"#,
        r#"<tool_call>[{"arguments":{},"name":"get_weather"}]"#,
        r#"[`get_weather` {"city": 42}]"#,
        "I would use `get_weather` for that",
        "```text\n[`get_weather` {\"city\": \"Boston\"}]\n```",
    ] {
        let failure = score_response(&tools, &expected, ArgumentsMatch::Exact, Some(content), &[])
            .expect_err("missing parsed call must fail");
        assert_ne!(
            failure.failure_class.as_deref(),
            Some("unparsed_tool_call"),
            "{content}"
        );
    }
}

#[test]
fn unparsed_tool_call_preserves_a_negative_trap_pass() {
    assert!(score_response(
        &[weather_tool()],
        &[],
        ArgumentsMatch::Exact,
        Some(r#"[`get_weather` {"city": "Boston"}]"#),
        &[]
    )
    .is_ok());
}

#[test]
fn failure_class_precedence_is_explicit_and_status_preserving() {
    let tools = [weather_tool()];
    let unparsed = Some(r#"[`get_weather` {"city": "Boston"}]"#);

    assert_eq!(classify_failure(Status::Error, &tools, None, &[]), None);
    assert_eq!(
        classify_failure(Status::Fail, &tools, None, &[]),
        Some("empty_response")
    );
    assert_eq!(
        classify_failure(Status::Fail, &tools, unparsed, &[]),
        Some("unparsed_tool_call")
    );
    assert_eq!(
        classify_failure(
            Status::Fail,
            &tools,
            unparsed,
            &[actual("get_weather", r#"{"city":"Boston"}"#)]
        ),
        None
    );
    assert_eq!(classify_failure(Status::Pass, &tools, unparsed, &[]), None);
    assert_eq!(
        classify_failure(Status::Fail, &tools, Some("I cannot call that tool."), &[]),
        None
    );
}

#[test]
fn invalid_argument_json_has_a_precise_reason() {
    let error = score_calls(
        &[weather_tool()],
        &[expected("get_weather", json!({"city": "Boston"}))],
        ArgumentsMatch::Exact,
        &[actual("get_weather", r#"{"city":"Boston""#)],
    )
    .expect_err("invalid arguments must fail");

    assert!(
        error.starts_with("arguments not valid JSON for 'get_weather':"),
        "{error}"
    );
}

#[test]
fn validates_required_nested_array_and_enum_schema_keywords() {
    let expected = [expected(
        "get_weather",
        json!({"city": "Boston", "days": [1, 2], "options": {"units": "c"}}),
    )];
    let tools = [weather_tool()];
    let valid = [actual(
        "get_weather",
        r#"{"city":"Boston","days":[1.0,2],"options":{"units":"c"}}"#,
    )];
    assert!(score_calls(&tools, &expected, ArgumentsMatch::Exact, &valid).is_ok());

    let missing = [actual("get_weather", r#"{"days":[]}"#)];
    assert_eq!(
        score_calls(&tools, &expected, ArgumentsMatch::Exact, &missing)
            .expect_err("required field must fail"),
        "schema violation: missing required field 'city'"
    );

    let bad_enum = [actual(
        "get_weather",
        r#"{"city":"Boston","days":[1,2],"options":{"units":"kelvin"}}"#,
    )];
    assert_eq!(
        score_calls(&tools, &expected, ArgumentsMatch::Exact, &bad_enum)
            .expect_err("enum must fail"),
        "schema violation: value for 'options.units' is not in enum"
    );
}

#[test]
fn exact_subset_ignore_and_per_call_override_are_honored() {
    let tools = [weather_tool()];
    let expected_call = expected("get_weather", json!({"city": "Boston"}));
    let with_extra = [actual("get_weather", r#"{"city":"Boston","days":[1]}"#)];

    assert_eq!(
        score_calls(
            &tools,
            std::slice::from_ref(&expected_call),
            ArgumentsMatch::Exact,
            &with_extra,
        )
        .expect_err("exact rejects extra keys"),
        "unexpected argument 'days'"
    );
    assert!(score_calls(
        &tools,
        std::slice::from_ref(&expected_call),
        ArgumentsMatch::Subset,
        &with_extra,
    )
    .is_ok());

    let mut ignored = expected_call.clone();
    ignored.arguments_match = Some(ArgumentsMatch::Ignore);
    assert!(score_calls(
        &tools,
        &[ignored],
        ArgumentsMatch::Exact,
        &[actual("get_weather", r#"{"city":"Seattle"}"#)],
    )
    .is_ok());
}

#[test]
fn strings_trim_numbers_compare_numerically_and_case_remains_significant() {
    let tools = [weather_tool()];
    let expected = [expected(
        "get_weather",
        json!({"city": "Boston", "days": [1]}),
    )];
    assert!(score_calls(
        &tools,
        &expected,
        ArgumentsMatch::Exact,
        &[actual("get_weather", r#"{"city":" Boston ","days":[1.0]}"#,)],
    )
    .is_ok());

    let error = score_calls(
        &tools,
        &expected,
        ArgumentsMatch::Exact,
        &[actual("get_weather", r#"{"city":"boston","days":[1]}"#)],
    )
    .expect_err("case mismatch must fail");
    assert_eq!(error, "wrong value for 'city': expected Boston, got boston");
}

#[test]
fn parallel_calls_match_as_an_unordered_set_and_reject_extras() {
    let tools = [weather_tool()];
    let expected = [
        expected("get_weather", json!({"city": "Boston"})),
        expected("get_weather", json!({"city": "Tokyo"})),
    ];
    let reversed = [
        actual("get_weather", r#"{"city":"Tokyo"}"#),
        actual("get_weather", r#"{"city":"Boston"}"#),
    ];
    assert!(score_calls(&tools, &expected, ArgumentsMatch::Exact, &reversed).is_ok());

    let extra = [
        actual("get_weather", r#"{"city":"Tokyo"}"#),
        actual("get_weather", r#"{"city":"Boston"}"#),
        actual("get_weather", r#"{"city":"Paris"}"#),
    ];
    assert_eq!(
        score_calls(&tools, &expected, ArgumentsMatch::Exact, &extra)
            .expect_err("extra call must fail"),
        "unexpected extra tool call: 'get_weather'"
    );
}

#[test]
fn unordered_matching_finds_a_complete_match_with_mixed_policies() {
    let tools = [weather_tool()];
    let mut subset = expected("get_weather", json!({"city": "Boston"}));
    subset.arguments_match = Some(ArgumentsMatch::Subset);
    let exact = expected("get_weather", json!({"city": "Boston", "days": [1]}));
    let actual = [
        actual("get_weather", r#"{"city":"Boston","days":[1]}"#),
        actual("get_weather", r#"{"city":"Boston"}"#),
    ];

    assert!(score_calls(&tools, &[subset, exact], ArgumentsMatch::Exact, &actual,).is_ok());
}
