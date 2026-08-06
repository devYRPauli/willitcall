use std::collections::HashSet;
use std::fs;

use wic_core::client::ToolCall;
use wic_core::corpus::{corpus_identity, load_frozen_v1_catalog};
use wic_core::score::score_calls;
use wic_core::{
    load_embedded_scenarios, ResponseRequirement, Scenario, ScenarioCategory, ScenarioFacet,
};

#[test]
fn embedded_corpus_is_integral_and_covers_every_category() {
    let scenarios = load_embedded_scenarios().expect("embedded scenarios should load");
    assert!(
        (45..=55).contains(&scenarios.len()),
        "scenario count: {}",
        scenarios.len()
    );

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
        assert_response_requirements_are_explicit(scenario);
        assert_facets_are_expected(scenario);
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

#[test]
fn corpus_identity_is_order_stable_and_content_sensitive() {
    let scenarios = load_embedded_scenarios().expect("embedded scenarios should load");
    let identity = corpus_identity(&scenarios);

    let mut reordered = scenarios.clone();
    reordered.reverse();
    assert_eq!(identity, corpus_identity(&reordered));

    let mut prompt_changed = scenarios.clone();
    prompt_changed[0].turns[0].messages[0]
        .content
        .push_str(" Identity probe.");
    assert_ne!(identity, corpus_identity(&prompt_changed));

    let mut schema_changed = scenarios.clone();
    schema_changed[0].tools[0]
        .parameters
        .as_object_mut()
        .expect("tool parameters should be an object")
        .insert("identity_probe".to_owned(), serde_json::Value::Bool(true));
    assert_ne!(identity, corpus_identity(&schema_changed));

    let mut expected_argument_changed = scenarios.clone();
    expected_argument_changed[0].turns[0].expected_calls[0]
        .arguments
        .as_object_mut()
        .expect("expected arguments should be an object")
        .insert("identity_probe".to_owned(), serde_json::Value::Bool(true));
    assert_ne!(identity, corpus_identity(&expected_argument_changed));
}

#[test]
fn frozen_v1_catalog_remains_verified_and_distinct_from_the_live_corpus() {
    let live = load_embedded_scenarios().expect("embedded scenarios should load");
    let catalog = load_frozen_v1_catalog().expect("frozen v1 catalog should load and verify");
    let live_hash = corpus_identity(&live);

    assert_eq!(catalog.id, "wic-50");
    assert_eq!(catalog.revision, "v1");
    assert_eq!(catalog.scenario_count, 50);
    assert_eq!(catalog.scenarios.len(), 50);
    assert_ne!(catalog.sha256, live_hash);
    assert_eq!(corpus_identity(&catalog.scenarios), catalog.sha256);
    assert!(catalog
        .scenarios
        .iter()
        .all(|scenario| scenario.facets.is_empty()));
    assert!(catalog
        .scenarios
        .iter()
        .flat_map(|scenario| &scenario.turns)
        .all(|turn| turn.response_requirement == ResponseRequirement::Either));
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
    assert_eq!(
        from_file.id, scenario.id,
        "filename slug must equal scenario id"
    );
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

fn assert_response_requirements_are_explicit(scenario: &Scenario) {
    for turn in &scenario.turns {
        let expected = if turn.expected_calls.is_empty() {
            ResponseRequirement::TextWithoutToolCalls
        } else {
            ResponseRequirement::ToolCalls
        };
        assert_eq!(
            turn.response_requirement, expected,
            "{} has the wrong response requirement",
            scenario.id
        );
    }
}

fn assert_facets_are_expected(scenario: &Scenario) {
    let expected: &[ScenarioFacet] = match scenario.id.as_str() {
        "negative-auto-arithmetic"
        | "negative-auto-knowledge"
        | "negative-greeting"
        | "negative-invalid-schema"
        | "negative-plain-text" => &[ScenarioFacet::Abstention],
        "negative-long-argument" => &[ScenarioFacet::ArgumentFidelity, ScenarioFacet::LongContext],
        "negative-unicode-argument" => &[ScenarioFacet::ArgumentFidelity, ScenarioFacet::Unicode],
        _ => &[],
    };
    assert_eq!(
        scenario.facets.as_slice(),
        expected,
        "{} has the wrong facets",
        scenario.id
    );
}
