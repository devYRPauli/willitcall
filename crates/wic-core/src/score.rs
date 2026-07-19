use serde_json::Value;

use crate::client::ToolCall;
use crate::{ArgumentsMatch, ExpectedCall, ToolDefinition};

pub fn score_calls(
    tools: &[ToolDefinition],
    expected: &[ExpectedCall],
    default_policy: ArgumentsMatch,
    actual: &[ToolCall],
) -> Result<(), String> {
    if expected.is_empty() {
        return match actual.first() {
            None => Ok(()),
            Some(call) => Err(format!("unexpected tool call emitted: '{}'", call.name)),
        };
    }
    if actual.is_empty() {
        return Err("no tool call emitted".to_owned());
    }

    let mut parsed = Vec::with_capacity(actual.len());
    for call in actual {
        let arguments: Value = serde_json::from_str(&call.arguments).map_err(|error| {
            format!("arguments not valid JSON for '{}': {error}", call.name)
        })?;
        let tool = tools
            .iter()
            .find(|tool| tool.name == call.name)
            .ok_or_else(|| format!("unknown tool called: '{}'", call.name))?;
        validate_schema(&tool.parameters, &arguments, "")?;
        parsed.push(arguments);
    }

    if actual.len() > expected.len() {
        return Err(format!(
            "unexpected extra tool call: '{}'",
            actual[expected.len()].name
        ));
    }
    if actual.len() < expected.len() {
        return Err(format!(
            "missing expected tool call: '{}'",
            expected[actual.len()].name
        ));
    }

    let mut used = vec![false; actual.len()];
    if has_complete_match(
        expected,
        actual,
        &parsed,
        default_policy,
        0,
        &mut used,
    ) {
        return Ok(());
    }

    // Re-run greedily only to select the most local human-readable mismatch.
    let mut available = (0..actual.len()).collect::<Vec<_>>();
    for expected_call in expected {
        let policy = expected_call.arguments_match.unwrap_or(default_policy);
        let same_name = available
            .iter()
            .copied()
            .filter(|index| actual[*index].name == expected_call.name)
            .collect::<Vec<_>>();
        if same_name.is_empty() {
            let got = available
                .first()
                .map(|index| actual[*index].name.as_str())
                .unwrap_or("none");
            return Err(format!(
                "wrong tool call: expected '{}', got '{}'",
                expected_call.name, got
            ));
        }

        let mut first_error = None;
        let matched = same_name.into_iter().find(|index| {
            match compare_arguments(&expected_call.arguments, &parsed[*index], policy, "") {
                Ok(()) => true,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    false
                }
            }
        });
        let Some(matched) = matched else {
            return Err(first_error.expect("same-name candidate produced a mismatch"));
        };
        available.retain(|index| *index != matched);
    }

    unreachable!("a greedy complete match would have been found by backtracking")
}

fn has_complete_match(
    expected: &[ExpectedCall],
    actual: &[ToolCall],
    parsed: &[Value],
    default_policy: ArgumentsMatch,
    expected_index: usize,
    used: &mut [bool],
) -> bool {
    if expected_index == expected.len() {
        return true;
    }
    let expected_call = &expected[expected_index];
    let policy = expected_call.arguments_match.unwrap_or(default_policy);
    for actual_index in 0..actual.len() {
        if used[actual_index]
            || actual[actual_index].name != expected_call.name
            || compare_arguments(
                &expected_call.arguments,
                &parsed[actual_index],
                policy,
                "",
            )
            .is_err()
        {
            continue;
        }
        used[actual_index] = true;
        if has_complete_match(
            expected,
            actual,
            parsed,
            default_policy,
            expected_index + 1,
            used,
        ) {
            return true;
        }
        used[actual_index] = false;
    }
    false
}

fn validate_schema(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(expected_type) = schema.get("type").and_then(Value::as_str) {
        let valid = match expected_type {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !valid {
            let location = if path.is_empty() {
                String::new()
            } else {
                format!(" for '{path}'")
            };
            return Err(format!(
                "schema violation: expected {expected_type}{location}, got {}",
                value_type(value)
            ));
        }
    }

    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|candidate| values_equal(candidate, value)) {
            let location = if path.is_empty() { "value" } else { path };
            return Err(format!(
                "schema violation: value for '{location}' is not in enum"
            ));
        }
    }

    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for field in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(field) {
                    let suffix = if path.is_empty() {
                        String::new()
                    } else {
                        format!(" at '{path}'")
                    };
                    return Err(format!(
                        "schema violation: missing required field '{field}'{suffix}"
                    ));
                }
            }
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (field, property_schema) in properties {
                if let Some(property_value) = object.get(field) {
                    validate_schema(property_schema, property_value, &join_path(path, field))?;
                }
            }
        }
    }

    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (index, item) in array.iter().enumerate() {
            let item_path = if path.is_empty() {
                format!("[{index}]")
            } else {
                format!("{path}[{index}]")
            };
            validate_schema(items, item, &item_path)?;
        }
    }

    Ok(())
}

fn compare_arguments(
    expected: &Value,
    actual: &Value,
    policy: ArgumentsMatch,
    path: &str,
) -> Result<(), String> {
    if policy == ArgumentsMatch::Ignore {
        return Ok(());
    }

    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, expected_value) in expected {
                let field_path = join_path(path, key);
                let actual_value = actual
                    .get(key)
                    .ok_or_else(|| format!("missing argument '{field_path}'"))?;
                compare_arguments(expected_value, actual_value, policy, &field_path)?;
            }
            if policy == ArgumentsMatch::Exact {
                if let Some(extra) = actual.keys().find(|key| !expected.contains_key(*key)) {
                    return Err(format!("unexpected argument '{}'", join_path(path, extra)));
                }
            }
            Ok(())
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                return Err(format!(
                    "wrong value for '{}': expected {}, got {}",
                    display_path(path),
                    Value::Array(expected.clone()),
                    Value::Array(actual.clone())
                ));
            }
            for (index, (expected_item, actual_item)) in
                expected.iter().zip(actual).enumerate()
            {
                let item_path = if path.is_empty() {
                    format!("[{index}]")
                } else {
                    format!("{path}[{index}]")
                };
                compare_arguments(expected_item, actual_item, policy, &item_path)?;
            }
            Ok(())
        }
        _ if values_equal(expected, actual) => Ok(()),
        _ => Err(format!(
            "wrong value for '{}': expected {}, got {}",
            display_path(path),
            display_value(expected),
            display_value(actual)
        )),
    }
}

fn values_equal(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Number(expected), Value::Number(actual)) => {
            expected.as_f64() == actual.as_f64()
        }
        (Value::String(expected), Value::String(actual)) => expected.trim() == actual.trim(),
        _ => expected == actual,
    }
}

fn join_path(path: &str, field: &str) -> String {
    if path.is_empty() {
        field.to_owned()
    } else {
        format!("{path}.{field}")
    }
}

fn display_path(path: &str) -> &str {
    if path.is_empty() { "arguments" } else { path }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.trim().to_owned(),
        _ => value.to_string(),
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
