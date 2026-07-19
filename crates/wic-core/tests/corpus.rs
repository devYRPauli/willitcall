use std::collections::HashSet;
use std::fs;

use wic_core::client::ToolCall;
use wic_core::score::score_calls;
use wic_core::{Scenario, ScenarioCategory, load_embedded_scenarios};

#[test]
fn embedded_corpus_is_integral_and_covers_every_category() {
    let scenarios = load_embedded_scenarios().expect("embedded scenarios should load");
    assert!((45..=55).contains(&scenarios.len()), "scenario count: {}", scenarios.len());

    let ids = scenarios
        .iter()
        .map(|scenario| scenario.id.as_str())
        .collect::<Vec<_>>();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "embedded scenarios must be ordered by id");
    assert_eq!(ids.iter().copied().collect::<HashSet<_>>().len(), ids.len());

    let categories = scenarios
        .iter()
        .map(|scenario| scenario.category)
        .collect::<HashSet<_>>();
    assert_eq!(
        categories,
        HashSet::from([
            ScenarioCategory::SingleCall,
            ScenarioCategory::ParallelCalls,
            ScenarioCategory::Streaming,
            ScenarioCategory::ToolChoiceModes,
            ScenarioCategory::MultiTurn,
            ScenarioCategory::NegativeTrap,
        ])
    );
    for category in categories {
        let count = scenarios
            .iter()
            .filter(|scenario| scenario.category == category)
            .count();
        assert!((7..=13).contains(&count), "{category} count: {count}");
    }

    for scenario in &scenarios {
        assert_filename_matches_id(scenario);
        assert_expected_calls_are_valid(scenario);
    }
}

#[test]
fn every_embedded_scenario_has_a_substantive_rationale() {
    let scenarios = load_embedded_scenarios().expect("embedded scenarios should load");

    for scenario in scenarios {
        assert!(
            scenario.rationale.trim().chars().count() >= 40,
            "{} rationale must contain at least 40 characters",
            scenario.id
        );
    }
}

fn assert_filename_matches_id(scenario: &Scenario) {
    let path = format!(
        "{}/scenarios/{}.toml",
        env!("CARGO_MANIFEST_DIR"),
        scenario.id
    );
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("scenario id has no matching file {path}: {error}"));
    let from_file: Scenario = toml::from_str(&contents)
        .unwrap_or_else(|error| panic!("failed to parse matching file {path}: {error}"));
    assert_eq!(from_file.id, scenario.id, "filename slug must equal scenario id");
    if !contents.is_ascii() {
        assert_eq!(
            scenario.id, "negative-unicode-argument",
            "non-ASCII text is limited to the unicode stress scenario"
        );
    }
}

fn assert_expected_calls_are_valid(scenario: &Scenario) {
    for turn in &scenario.turns {
        for expected in &turn.expected_calls {
            assert!(
                scenario.tools.iter().any(|tool| tool.name == expected.name),
                "{} expects undefined tool {}",
                scenario.id,
                expected.name
            );
        }
        let actual = turn
            .expected_calls
            .iter()
            .enumerate()
            .map(|(index, expected)| ToolCall {
                id: Some(format!("expected-{index}")),
                name: expected.name.clone(),
                arguments: serde_json::to_string(&expected.arguments)
                    .expect("expected arguments should serialize"),
            })
            .collect::<Vec<_>>();
        score_calls(
            &scenario.tools,
            &turn.expected_calls,
            scenario.arguments_match,
            &actual,
        )
        .unwrap_or_else(|error| panic!("{} has invalid expected arguments: {error}", scenario.id));
    }
}
