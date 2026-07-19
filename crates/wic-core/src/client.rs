use std::collections::BTreeMap;
use std::str;
use std::time::Duration;

use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::result::SamplingParams;
use crate::{Scenario, ToolChoice};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssistantResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug)]
pub enum CompletionResult {
    Parsed {
        response: AssistantResponse,
        raw_bytes: Vec<u8>,
        retried: bool,
    },
    Invalid {
        reason: String,
        raw_bytes: Vec<u8>,
        retried: bool,
    },
    Error {
        reason: String,
        raw_bytes: Vec<u8>,
        retried: bool,
    },
}

#[derive(Clone)]
pub struct EndpointClient {
    http: reqwest::Client,
    endpoint: String,
    model: String,
    timeout: Duration,
    sampling: SamplingParams,
}

impl EndpointClient {
    pub fn new(
        endpoint: String,
        model: String,
        timeout: Duration,
        sampling: SamplingParams,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            model,
            timeout,
            sampling,
        }
    }

    pub async fn preflight(&self) -> Result<(), String> {
        let models_url = format!("{}/models", self.endpoint);
        let (status, body) = tokio::time::timeout(self.timeout, self.get_raw(&models_url))
            .await
            .map_err(|_| format!("preflight timed out after {}s", self.timeout.as_secs()))?
            .map_err(|error| format!("endpoint unreachable: {error}"))?;

        if status.is_success() {
            let document: Value = serde_json::from_slice(&body)
                .map_err(|error| format!("invalid /models response: {error}"))?;
            let models = document
                .get("data")
                .and_then(Value::as_array)
                .ok_or_else(|| "invalid /models response: missing data array".to_owned())?;
            let model_found = models.iter().any(|entry| {
                entry.get("id").and_then(Value::as_str) == Some(self.model.as_str())
            });
            return if model_found {
                Ok(())
            } else {
                Err(format!(
                    "model '{}' was not reported by {models_url}",
                    self.model
                ))
            };
        }

        if matches!(status, StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED) {
            return self.preflight_chat_completion().await;
        }

        Err(format!("preflight request failed with HTTP {status}"))
    }

    pub async fn server_version(&self, path: &str, field: &str) -> Option<String> {
        let root = self.endpoint.strip_suffix("/v1").unwrap_or(&self.endpoint);
        let url = format!("{root}{path}");
        let (status, body) = tokio::time::timeout(self.timeout, self.get_raw(&url))
            .await
            .ok()?
            .ok()?;
        if !status.is_success() {
            return None;
        }
        serde_json::from_slice::<Value>(&body)
            .ok()?
            .get(field)?
            .as_str()
            .map(str::to_owned)
    }

    async fn preflight_chat_completion(&self) -> Result<(), String> {
        let payload = json!({
            "model": self.model,
            "messages": [{"role": "user", "content": "Reply OK."}],
            "stream": false,
            "max_tokens": 1
        });
        let (status, body) = tokio::time::timeout(self.timeout, self.send_raw(&payload))
            .await
            .map_err(|_| format!("preflight timed out after {}s", self.timeout.as_secs()))?
            .map_err(|error| format!("endpoint unreachable: {error}"))?;
        if !status.is_success() {
            return Err(format!(
                "preflight chat completion failed with HTTP {status}"
            ));
        }
        parse_non_streaming(&body)
            .map(|_| ())
            .map_err(|error| format!("preflight chat completion was unusable: {error}"))
    }

    pub async fn complete(&self, scenario: &Scenario, messages: &[Value]) -> CompletionResult {
        let payload = build_request_payload(scenario, &self.model, messages, &self.sampling);
        let mut retry_raw = Vec::new();

        for attempt in 0..=1 {
            let attempted = tokio::time::timeout(self.timeout, self.send_raw(&payload)).await;
            let (status, body) = match attempted {
                Err(_) if attempt == 0 => continue,
                Err(_) => {
                    return CompletionResult::Error {
                        reason: format!(
                            "request timed out after {}s (retry also failed)",
                            self.timeout.as_secs()
                        ),
                        raw_bytes: retry_raw,
                        retried: true,
                    };
                }
                Ok(Err(_)) if attempt == 0 => continue,
                Ok(Err(error)) => {
                    return CompletionResult::Error {
                        reason: format!("transport error after retry: {error}"),
                        raw_bytes: retry_raw,
                        retried: true,
                    };
                }
                Ok(Ok(result)) => result,
            };

            if status.is_server_error() {
                retry_raw.extend_from_slice(&body);
                if attempt == 0 {
                    continue;
                }
                return CompletionResult::Error {
                    reason: format!("server returned HTTP {status} after retry"),
                    raw_bytes: retry_raw,
                    retried: true,
                };
            }

            retry_raw.extend_from_slice(&body);
            if !status.is_success() {
                return CompletionResult::Error {
                    reason: format!("server returned HTTP {status}"),
                    raw_bytes: retry_raw,
                    retried: attempt > 0,
                };
            }

            let parsed = if scenario.stream {
                parse_sse_data(&body).and_then(|payloads| reassemble_sse_payloads(&payloads))
            } else {
                parse_non_streaming(&body)
            };
            return match parsed {
                Ok(response) => CompletionResult::Parsed {
                    response,
                    raw_bytes: retry_raw,
                    retried: attempt > 0,
                },
                Err(reason) => CompletionResult::Invalid {
                    reason,
                    raw_bytes: retry_raw,
                    retried: attempt > 0,
                },
            };
        }

        unreachable!("the retry loop always returns")
    }

    async fn send_raw(&self, payload: &Value) -> Result<(StatusCode, Vec<u8>), reqwest::Error> {
        let url = format!("{}/chat/completions", self.endpoint);
        let mut response = self.http.post(url).json(payload).send().await?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }

    async fn get_raw(&self, url: &str) -> Result<(StatusCode, Vec<u8>), reqwest::Error> {
        let mut response = self.http.get(url).send().await?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            body.extend_from_slice(&chunk);
        }
        Ok((status, body))
    }
}

pub fn build_request_payload(
    scenario: &Scenario,
    model: &str,
    messages: &[Value],
    sampling: &SamplingParams,
) -> Value {
    let tools = scenario
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }
            })
        })
        .collect::<Vec<_>>();
    let tool_choice = match &scenario.tool_choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::Required => json!("required"),
        ToolChoice::None => json!("none"),
        ToolChoice::Named { name } => {
            json!({"type": "function", "function": {"name": name}})
        }
    };
    let mut payload = Map::from_iter([
        ("model".to_owned(), json!(model)),
        ("messages".to_owned(), Value::Array(messages.to_vec())),
        ("tools".to_owned(), Value::Array(tools)),
        ("tool_choice".to_owned(), tool_choice),
        ("stream".to_owned(), json!(scenario.stream)),
    ]);
    if let Some(temperature) = sampling.temperature {
        payload.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = sampling.top_p {
        payload.insert("top_p".to_owned(), json!(top_p));
    }
    if let Some(seed) = sampling.seed {
        payload.insert("seed".to_owned(), json!(seed));
    }
    if let Some(max_tokens) = sampling.max_tokens {
        payload.insert("max_tokens".to_owned(), json!(max_tokens));
    }
    Value::Object(payload)
}

#[derive(Deserialize)]
struct CompletionEnvelope {
    choices: Vec<CompletionChoice>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
struct WireToolCall {
    #[serde(default)]
    id: Option<String>,
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    arguments: String,
}

pub fn parse_non_streaming(bytes: &[u8]) -> Result<AssistantResponse, String> {
    let envelope: CompletionEnvelope = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid chat completion JSON: {error}"))?;
    let choice = envelope
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| "chat completion response has no choices".to_owned())?;
    Ok(AssistantResponse {
        content: choice.message.content,
        tool_calls: choice
            .message
            .tool_calls
            .into_iter()
            .map(|call| ToolCall {
                id: call.id,
                name: call.function.name,
                arguments: call.function.arguments,
            })
            .collect(),
    })
}

pub fn parse_sse_data(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = str::from_utf8(bytes).map_err(|error| format!("SSE response is not UTF-8: {error}"))?;
    let normalized = text.replace("\r\n", "\n");
    let mut payloads = Vec::new();
    for event in normalized.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:").map(|data| data.strip_prefix(' ').unwrap_or(data)))
            .collect::<Vec<_>>();
        if !data.is_empty() {
            payloads.push(data.join("\n"));
        }
    }
    Ok(payloads)
}

#[derive(Default, Deserialize)]
struct StreamEnvelope {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Default, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Default, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<StreamToolCall>,
}

#[derive(Default, Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunction>,
}

#[derive(Default, Deserialize)]
struct StreamFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Default)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

pub fn reassemble_sse_payloads(payloads: &[String]) -> Result<AssistantResponse, String> {
    let mut content = String::new();
    let mut has_content = false;
    let mut calls = BTreeMap::<usize, ToolCallBuilder>::new();

    for payload in payloads {
        if payload.trim() == "[DONE]" {
            break;
        }
        let envelope: StreamEnvelope = serde_json::from_str(payload)
            .map_err(|error| format!("invalid SSE data JSON: {error}"))?;
        let Some(choice) = envelope.choices.into_iter().next() else {
            continue;
        };
        if let Some(fragment) = choice.delta.content {
            has_content = true;
            content.push_str(&fragment);
        }
        let delta_call_count = choice.delta.tool_calls.len();
        for (position, call) in choice.delta.tool_calls.into_iter().enumerate() {
            let index = call.index.unwrap_or_else(|| {
                if delta_call_count == 1 && calls.len() == 1 {
                    *calls.keys().next().expect("one call exists")
                } else {
                    position
                }
            });
            let builder = calls.entry(index).or_default();
            if let Some(id) = call.id {
                builder.id.push_str(&id);
            }
            if let Some(function) = call.function {
                if let Some(name) = function.name {
                    builder.name.push_str(&name);
                }
                if let Some(arguments) = function.arguments {
                    builder.arguments.push_str(&arguments);
                }
            }
        }
    }

    Ok(AssistantResponse {
        content: has_content.then_some(content),
        tool_calls: calls
            .into_values()
            .map(|call| ToolCall {
                id: (!call.id.is_empty()).then_some(call.id),
                name: call.name,
                arguments: call.arguments,
            })
            .collect(),
    })
}
