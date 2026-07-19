use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};

pub mod result;
pub mod client;
pub mod runner;
pub mod score;

static EMBEDDED_SCENARIOS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/scenarios");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub category: ScenarioCategory,
    pub description: String,
    pub rationale: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub arguments_match: ArgumentsMatch,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: ToolChoice,
    pub turns: Vec<Turn>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioCategory {
    SingleCall,
    ParallelCalls,
    Streaming,
    ToolChoiceModes,
    MultiTurn,
    NegativeTrap,
}

impl fmt::Display for ScenarioCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::SingleCall => "single_call",
            Self::ParallelCalls => "parallel_calls",
            Self::Streaming => "streaming",
            Self::ToolChoiceModes => "tool_choice_modes",
            Self::MultiTurn => "multi_turn",
            Self::NegativeTrap => "negative_trap",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Named { name: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub expected_calls: Vec<ExpectedCall>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_ref: Option<usize>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedCall {
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_match: Option<ArgumentsMatch>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentsMatch {
    #[default]
    Exact,
    Subset,
    Ignore,
}

#[derive(Debug)]
pub struct ScenarioLoadError(String);

impl fmt::Display for ScenarioLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScenarioLoadError {}

pub fn load_embedded_scenarios() -> Result<Vec<Scenario>, ScenarioLoadError> {
    let mut documents = EMBEDDED_SCENARIOS
        .files()
        .filter(|file| file.path().extension().is_some_and(|ext| ext == "toml"))
        .map(|file| {
            let path = file.path().display().to_string();
            let contents = file.contents_utf8().ok_or_else(|| {
                ScenarioLoadError(format!("embedded scenario {path} is not UTF-8"))
            })?;
            Ok((path, contents.to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    parse_scenarios(documents)
}

pub fn load_scenarios_from_dir(path: &Path) -> Result<Vec<Scenario>, ScenarioLoadError> {
    let entries = fs::read_dir(path).map_err(|error| {
        ScenarioLoadError(format!(
            "failed to read scenario directory {}: {error}",
            path.display()
        ))
    })?;
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| ScenarioLoadError(format!("failed to read directory entry: {error}")))
        })
        .collect::<Result<Vec<PathBuf>, _>>()?;
    paths.retain(|path| {
        path.is_file() && path.extension().is_some_and(|extension| extension == "toml")
    });
    paths.sort();

    let documents = paths
        .into_iter()
        .map(|path| {
            let contents = fs::read_to_string(&path).map_err(|error| {
                ScenarioLoadError(format!("failed to read {}: {error}", path.display()))
            })?;
            Ok((path.display().to_string(), contents))
        })
        .collect::<Result<Vec<_>, _>>()?;
    parse_scenarios(documents)
}

fn parse_scenarios(
    documents: Vec<(String, String)>,
) -> Result<Vec<Scenario>, ScenarioLoadError> {
    let mut ids = HashSet::new();
    let mut scenarios = Vec::with_capacity(documents.len());

    for (path, contents) in documents {
        let scenario: Scenario = toml::from_str(&contents)
            .map_err(|error| ScenarioLoadError(format!("invalid scenario {path}: {error}")))?;
        if !is_stable_slug(&scenario.id) {
            return Err(ScenarioLoadError(format!(
                "scenario id {:?} must be a stable slug",
                scenario.id
            )));
        }
        if !ids.insert(scenario.id.clone()) {
            return Err(ScenarioLoadError(format!(
                "duplicate scenario id {:?}",
                scenario.id
            )));
        }
        scenarios.push(scenario);
    }

    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(scenarios)
}

fn is_stable_slug(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;

    use super::{ScenarioCategory, load_embedded_scenarios, load_scenarios_from_dir};

    const VALID_SCENARIO: &str = r#"
id = "single-weather"
category = "single_call"
description = "Call the weather tool."
rationale = "The prompt names Boston verbatim, which pins the exact city argument."

[[tools]]
name = "get_weather"
description = "Get weather."

[tools.parameters]
type = "object"

[tool_choice]
mode = "auto"

[[turns]]
[[turns.messages]]
role = "user"
content = "What is the weather in Boston?"

[[turns.expected_calls]]
name = "get_weather"

[turns.expected_calls.arguments]
city = "Boston"
"#;

    #[test]
    fn embedded_corpus_has_fifty_scenarios_across_all_categories() {
        let scenarios = load_embedded_scenarios().expect("embedded scenarios should load");
        let categories: HashSet<_> = scenarios
            .iter()
            .map(|scenario| scenario.category)
            .collect();

        assert_eq!(scenarios.len(), 50);
        assert_eq!(categories.len(), 6);
        assert!(categories.contains(&ScenarioCategory::ParallelCalls));
        assert!(categories.contains(&ScenarioCategory::ToolChoiceModes));
        assert!(categories.contains(&ScenarioCategory::MultiTurn));
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.arguments_match == super::ArgumentsMatch::Subset)
        );
    }

    #[test]
    fn argument_match_policy_supports_scenario_default_and_call_override() {
        let document = VALID_SCENARIO
            .replace(
                "description = \"Call the weather tool.\"",
                "description = \"Call the weather tool.\"\narguments_match = \"subset\"",
            )
            .replace(
                "name = \"get_weather\"\n\n[turns.expected_calls.arguments]",
                "name = \"get_weather\"\narguments_match = \"ignore\"\n\n[turns.expected_calls.arguments]",
            );
        let directory = tempfile::tempdir().expect("temp directory");
        fs::write(directory.path().join("policy.toml"), document).expect("write scenario");

        let scenario = load_scenarios_from_dir(directory.path())
            .expect("load scenario")
            .remove(0);
        assert_eq!(scenario.arguments_match, super::ArgumentsMatch::Subset);
        assert_eq!(
            scenario.turns[0].expected_calls[0].arguments_match,
            Some(super::ArgumentsMatch::Ignore)
        );
    }

    #[test]
    fn disk_loader_rejects_unknown_fields() {
        let directory = tempfile::tempdir().expect("temp directory");
        fs::write(
            directory.path().join("unknown.toml"),
            format!("unknown = true\n{VALID_SCENARIO}"),
        )
        .expect("write scenario");

        let error = load_scenarios_from_dir(directory.path()).expect_err("unknown field");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn disk_loader_rejects_duplicate_ids() {
        let directory = tempfile::tempdir().expect("temp directory");
        fs::write(directory.path().join("first.toml"), VALID_SCENARIO)
            .expect("write first scenario");
        fs::write(directory.path().join("second.toml"), VALID_SCENARIO)
            .expect("write second scenario");

        let error = load_scenarios_from_dir(directory.path()).expect_err("duplicate id");
        assert!(error.to_string().contains("duplicate scenario id"));
    }

    #[test]
    fn disk_loader_rejects_non_slug_ids() {
        let directory = tempfile::tempdir().expect("temp directory");
        let invalid = VALID_SCENARIO.replace("single-weather", "Single Weather");
        fs::write(directory.path().join("invalid.toml"), invalid).expect("write scenario");

        let error = load_scenarios_from_dir(directory.path()).expect_err("invalid id");
        assert!(error.to_string().contains("stable slug"));
    }
}
