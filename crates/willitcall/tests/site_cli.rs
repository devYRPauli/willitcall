use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

// Briefs 8 and 9 intentionally change the renderer and will update this golden.
const SITE_CONTRACT_GOLDEN: &[u8] = include_bytes!("golden/site-contract-index.html");

fn scenario(
    id: &str,
    category: &str,
    status: &str,
    failure_reason: Option<&str>,
    evidence_path: Option<&str>,
) -> Value {
    json!({
        "id": id,
        "category": category,
        "status": status,
        "failure_reason": failure_reason,
        "evidence_hash": evidence_path.map(|_| "sha256:fixture"),
        "evidence_path": evidence_path,
        "retried": false
    })
}

fn write_result(
    path: &Path,
    schema_version: u32,
    model_id: &str,
    declared_quant: Option<&str>,
    server: &str,
    environment: Option<(&str, &str)>,
    mut scenarios: Vec<Value>,
) {
    let mut metadata = json!({
        "run_id": "20260720T120000Z-fixture",
        "timestamp": "2026-07-20T12:00:00Z",
        "willitcall_version": "0.1.0",
        "endpoint": "http://127.0.0.1:8080/v1",
        "model_id": model_id,
        "declared_quant": declared_quant,
        "server": {
            "preset_name": server,
            "reported_version": "fixture-version",
            "quirk_flags": []
        },
        "sampling": {
            "temperature": 0.0,
            "top_p": 1.0,
            "seed": 42,
            "max_tokens": 1024
        }
    });
    if let Some((host_hardware_class, host_os)) = environment {
        metadata["environment"] = json!({
            "host_hardware_class": host_hardware_class,
            "host_os": host_os
        });
    }
    if schema_version == 1 {
        metadata
            .as_object_mut()
            .expect("metadata object")
            .remove("run_id");
        for scenario in &mut scenarios {
            scenario
                .as_object_mut()
                .expect("scenario object")
                .remove("evidence_path");
        }
    }
    let passed = scenarios
        .iter()
        .filter(|scenario| scenario["status"] == "pass")
        .count() as u32;
    let failed = scenarios
        .iter()
        .filter(|scenario| scenario["status"] == "fail")
        .count() as u32;
    let errors = scenarios
        .iter()
        .filter(|scenario| scenario["status"] == "error")
        .count() as u32;
    let skipped = scenarios
        .iter()
        .filter(|scenario| scenario["status"] == "skipped")
        .count() as u32;
    let result = json!({
        "schema_version": schema_version,
        "metadata": metadata,
        "scenarios": scenarios,
        "totals": {
            "total": passed + failed + errors + skipped,
            "passed": passed,
            "failed": failed,
            "errors": errors,
            "skipped": skipped
        }
    });
    fs::write(
        path,
        serde_json::to_vec_pretty(&result).expect("encode result fixture"),
    )
    .expect("write result fixture");
}

fn run_site(results: &Path, output: &Path, repo_base: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_willitcall"));
    command
        .arg("site")
        .arg("--results")
        .arg(results)
        .arg("--out")
        .arg(output);
    if let Some(repo_base) = repo_base {
        command.arg("--repo-base").arg(repo_base);
    }
    command.output().expect("run site generator")
}

#[test]
fn site_contract_golden_matches_post_migration_renderer() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/site-contract-results");
    let output = directory.path().join("site");

    let generated = run_site(
        &results,
        &output,
        Some("https://example.invalid/willitcall"),
    );
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let index = fs::read(output.join("index.html")).expect("generated index");
    assert_eq!(
        normalize_build_hash(&index),
        normalize_build_hash(SITE_CONTRACT_GOLDEN)
    );
}

/// The colophon states which build produced the page, so it changes whenever the
/// generator does. Comparing it here would make this golden fail on every code
/// change and train whoever hits it to regenerate without reading the diff, which
/// is the one thing a golden exists to prevent.
fn normalize_build_hash(html: &[u8]) -> String {
    let text = String::from_utf8_lossy(html);
    let mut out = String::with_capacity(text.len());
    let mut rest: &str = &text;
    while let Some(start) = rest.find("Build hash <code>") {
        let after = start + "Build hash <code>".len();
        let Some(end) = rest[after..].find("</code>") else {
            break;
        };
        out.push_str(&rest[..after]);
        out.push_str("NORMALIZED");
        rest = &rest[after + end..];
    }
    out.push_str(rest);
    out
}

#[test]
fn analysis_views_render_the_published_observation_contract() {
    let directory = tempfile::tempdir().expect("temp directory");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = directory.path().join("site");

    let generated = run_site(&repo.join("results"), &output, None);
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    let outcomes =
        fs::read_to_string(output.join("outcomes.html")).expect("generated outcomes page");
    let appendix =
        fs::read_to_string(output.join("appendix.html")).expect("generated appendix page");
    let results_json =
        fs::read_to_string(output.join("results.json")).expect("generated JSON data");
    let results_csv = fs::read_to_string(output.join("results.csv")).expect("generated CSV data");
    assert_eq!(index.matches("class=\"result-row\"").count(), 32);
    assert_eq!(appendix.matches("class=\"stack-detail\"").count(), 32);
    assert_eq!(index.matches("<h2").count(), 3);
    assert!(index.len() < 150_000, "index is {} bytes", index.len());
    assert_eq!(outcomes.matches("data-figure-number=").count(), 3);
    assert_eq!(index.matches("data-figure-number=").count(), 1);
    assert_eq!(outcomes.matches("What this does not show:").count(), 3);
    assert_eq!(index.matches("What this does not show:").count(), 1);
    assert!(!index.contains("svg-text-fallback"));
    assert!(!outcomes.contains("svg-text-fallback"));
    assert!(!index.contains("Per-capability aggregate"));
    assert!(!outcomes.contains("Per-capability aggregate"));
    assert!(!index.contains("capability-bar"));
    assert!(!outcomes.contains("capability-bar"));

    let figure_pages = format!("{outcomes}{index}");
    let svgs = figure_pages
        .split("<svg ")
        .skip(1)
        .map(|tail| tail.split_once("</svg>").expect("closed svg").0)
        .collect::<Vec<_>>();
    assert_eq!(svgs.len(), 4);
    for svg in &svgs {
        assert!(svg.contains("<title>"));
        assert!(svg.contains("<desc>"));
        assert!(!svg.contains("http://"));
        assert!(!svg.contains("https://"));
    }
    for generated in [&index, &outcomes, &appendix, &results_json, &results_csv] {
        assert!(!generated.contains("/Users/"));
        assert!(generated.is_ascii());
    }
    assert!(!index.contains("<script src=\"http"));
    assert!(!index.contains("View 50 scenarios and row metadata"));
    assert!(!index.contains("class=\"detail-row\""));
    assert_eq!(index.matches("class=\"detail-link\"").count(), 32);
    assert_eq!(outcomes.matches("class=\"mini-raster-panel\"").count(), 6);
    assert_eq!(index.matches("class=\"replication-note\"").count(), 0);
    assert_eq!(index.matches("n=1, no verdict").count(), 1);
    assert_eq!(index.matches("class=\"model-heading\"").count(), 5);
    assert_eq!(index.matches("result-group multi-row").count(), 5);
    assert_eq!(index.matches("result-group single-row").count(), 13);
    assert_eq!(index.matches("class=\"row-meta\"").count(), 32);
    assert!(index.contains("id=\"model-search\" type=\"search\""));
    assert!(index.contains("data-decode-mode=\"grammar_constrained\""));
    assert!(index.contains("data-decode-mode=\"unconstrained_post_hoc\""));
    assert_eq!(index.matches("data-decode-source=\"recorded\"").count(), 6);
    assert_eq!(
        index
            .matches("data-decode-source=\"preset_mapping\"")
            .count(),
        26
    );
    assert_eq!(index.matches("data-decode-source=\"unknown\"").count(), 0);
    assert_eq!(index.matches("class=\"decode-band").count(), 0);
    assert_eq!(index.matches("class=\"decode-badge").count(), 32);
    assert_eq!(index.matches("decode provenance:").count(), 32);
    for (id, label) in [
        ("single_call", "Single call"),
        ("tool_choice_modes", "Tool choice"),
        ("negative_trap", "Correctly declines"),
        ("multi_turn", "Multi-turn"),
        ("parallel_calls", "Parallel calls"),
        ("streaming", "Streaming"),
    ] {
        assert!(index.contains(&format!("title=\"{id}\">{label}</th>")));
    }

    assert!(svgs[0].contains("Figure 1. Scenario-status raster"));
    assert!(svgs[0].contains("<title>single-weather: pass</title>"));
    assert!(!svgs[0].contains("Call one weather tool with a city argument."));
    assert!(!svgs[0].contains("Rationale:"));
    assert!(!svgs[0].contains("no-verdict-overlay"));

    assert!(outcomes.contains(
        "multi_turn passes 37/224 (17%), and 22 of 32 rows pass none of the multi_turn scenarios"
    ));
    assert!(outcomes.contains(
        "The repeated 7-pass signature is shared by 9 stacks, including the granite observations; it contains the same 5 negative_trap and 2 tool_choice_modes passes in every row."
    ));
    assert_eq!(
        figure_pages
            .matches("Data: <a href=\"results.json\"")
            .count(),
        4
    );
    assert_eq!(
        figure_pages
            .matches("Data: <a href=\"results.json\">JSON</a>, <a href=\"results.csv\"")
            .count(),
        4
    );
    assert!(index.contains("Machine-readable observations: <a href=\"results.json\">JSON</a>"));
    let json: Value = serde_json::from_str(&results_json).expect("valid site results JSON");
    assert_eq!(json["row_count"], 32);
    assert_eq!(json["stacks"].as_array().expect("stacks array").len(), 32);
    assert_eq!(results_csv.lines().count(), 1_601);

    assert_eq!(svgs[3].matches("class=\"strip-observation\"").count(), 27);
    assert_eq!(
        svgs[3]
            .matches("class=\"not-fully-measurable-item\"")
            .count(),
        5
    );
    for gemma in ["gemma3:4b", "gemma3:12b"] {
        let item = svgs[3]
            .split(&format!("aria-label=\"{gemma}"))
            .nth(1)
            .expect("gemma row in not-fully-measurable panel")
            .split_once("</g>")
            .expect("closed gemma panel item")
            .0;
        assert!(item.contains("50 errors, 0 skipped; pass count not plotted"));
        assert!(!item.contains("data-pass-count"));
    }
}

#[test]
fn site_generates_v1_and_v2_rows_ratios_links_and_plain_annotations() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let output = directory.path().join("site");
    fs::create_dir(&results).expect("results directory");

    write_result(
        &results.join("ollama-legacy-model.json"),
        1,
        "legacy:model",
        None,
        "ollama",
        None,
        vec![
            scenario("legacy-pass", "single_call", "pass", None, None),
            scenario(
                "legacy-fail",
                "single_call",
                "fail",
                Some("wrong tool call"),
                None,
            ),
        ],
    );
    write_result(
        &results.join("mlx_lm-fixture-model.json"),
        2,
        "fixture:model",
        None,
        "mlx_lm",
        None,
        vec![scenario("single-ok", "single_call", "pass", None, None)],
    );

    let mut caused = scenario(
        "parallel-bad",
        "parallel_calls",
        "fail",
        Some("no tool call emitted"),
        Some("evidence/fixture/parallel-bad.json"),
    );
    caused["failure_class"] = json!("empty_response");
    caused["cause"] = json!({
        "kind": "server-defect",
        "reference": "docs/case-studies/server-defect.md",
        "note": "isolated on a second server"
    });
    let mut empty = scenario(
        "stream-empty",
        "streaming",
        "fail",
        Some("assistant returned no content and no tool calls"),
        Some("evidence/fixture/stream-empty.json"),
    );
    empty["failure_class"] = json!("empty_response");
    let mut unparsed = scenario(
        "single-unparsed",
        "single_call",
        "fail",
        Some("no tool call emitted"),
        Some("evidence/fixture/single-unparsed.json"),
    );
    unparsed["failure_class"] = json!("unparsed_tool_call");
    write_result(
        &results.join("llamacpp-blob-model.json"),
        2,
        "/models/blobs/sha256-deadbeef",
        Some("Q4_K_M"),
        "llamacpp",
        Some(("Apple M4 Max, 64GB", "macOS 15.5")),
        vec![
            scenario("single-ok", "single_call", "pass", None, None),
            unparsed,
            caused,
            empty,
            scenario("choice-ok", "tool_choice_modes", "pass", None, None),
            scenario("turn-ok", "multi_turn", "pass", None, None),
            scenario(
                "turn-bad",
                "multi_turn",
                "fail",
                Some("wrong follow-up"),
                Some("evidence/fixture/turn-bad.json"),
            ),
            scenario(
                "negative-bad",
                "negative_trap",
                "fail",
                Some("unexpected tool call"),
                Some("evidence/fixture/negative-bad.json"),
            ),
        ],
    );

    let generated = run_site(&results, &output, None);
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    let appendix =
        fs::read_to_string(output.join("appendix.html")).expect("generated appendix page");
    let submit = fs::read_to_string(output.join("submit.html")).expect("generated submit page");
    assert!(output.join("style.css").is_file());
    assert!(output.join("site.js").is_file());
    assert_eq!(index.matches("class=\"result-row\"").count(), 3);
    assert!(index.contains("id=\"model-search\" type=\"search\""));
    assert!(index.contains("data-server=\"ollama\""));
    assert!(index.contains("data-server=\"llamacpp\""));
    assert!(index.contains("data-server=\"mlx_lm\""));
    assert!(index.contains("data-decode-mode=\"grammar_constrained\""));
    assert!(index.contains("data-decode-mode=\"unconstrained_post_hoc\""));
    assert!(index.contains(" / MLX LM</span>"));
    assert!(appendix.contains("blob-model"));
    assert!(index.contains(">Q4_K_M / llama.cpp</span>"));
    assert!(appendix.contains("sha256-deadbeef"));
    assert!(!appendix.contains("/models/blobs/sha256-deadbeef"));
    assert!(index.contains(">1/2<"));
    assert!(index.contains(">0/1<"));
    assert!(index.contains(">1/2<"));
    assert!(index.contains(
        "https://github.com/devYRPauli/willitcall/blob/main/results/evidence/fixture/parallel-bad.json"
    ));
    assert!(appendix.contains(
        "https://github.com/devYRPauli/willitcall/blob/main/docs/case-studies/server-defect.md"
    ));
    assert!(appendix.contains("server defect"));
    assert!(appendix.contains("empty response"));
    assert!(appendix.contains("<span class=\"annotation\">unparsed tool call</span>"));
    assert!(!appendix.contains("class=\"badge"));
    assert!(index.contains("A cell measures the whole stack"));
    assert!(index.contains("GBNF grammar"));
    assert!(index.contains("Each published cell is one run."));
    assert!(index.contains("replicated across at least five runs per arm"));
    assert!(index.contains("90 runs across 18 quantization arms"));
    assert!(index.contains("40 runs across 8 arms for the peg-native anomaly"));
    assert!(index.contains("Meta-Llama-3.1-8B-Instruct"));
    assert!(index.contains(
        "https://github.com/devYRPauli/willitcall/blob/main/docs/case-studies/2026-07-21-llamacpp-500s-on-llama-3.1-tool-calls.md"
    ));
    assert!(appendix.contains("Host hardware"));
    assert!(appendix.contains("Apple M4 Max, 64GB"));
    assert!(appendix.contains("Host OS"));
    assert!(appendix.contains("macOS 15.5"));
    assert!(index.contains("docs/case-studies/"));
    assert!(!index.contains("class=\"detail-row\""));
    assert!(!index.contains("View 50 scenarios and row metadata"));
    assert_eq!(index.matches("class=\"detail-link\"").count(), 3);
    assert_eq!(appendix.matches("class=\"stack-detail\"").count(), 3);
    assert!(output.join("results.json").is_file());
    assert!(output.join("results.csv").is_file());
    assert!(!index.contains("<script src=\"http"));

    assert!(submit.contains("cargo run -p willitcall -- run"));
    assert!(submit.contains("--server ollama"));
    assert!(submit.contains("--server llamacpp"));
    assert!(submit.contains("preflight clean (no contention override)"));
    assert!(submit.contains("empty responses cross-checked on a second server"));
    assert!(submit.contains("CONTRIBUTING.md"));
}

#[test]
fn site_never_renders_absolute_model_paths() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let output = directory.path().join("site");
    fs::create_dir(&results).expect("results directory");
    write_result(
        &results.join("ollama-local-blob.json"),
        2,
        "/Users/someone/.ollama/models/blobs/sha256-deadbeef",
        None,
        "ollama",
        None,
        vec![scenario("local-blob", "single_call", "pass", None, None)],
    );
    write_result(
        &results.join("llamacpp-local-file.json"),
        2,
        "/Users/someone/models/custom-model.gguf",
        None,
        "llamacpp",
        None,
        vec![scenario("local-file", "single_call", "pass", None, None)],
    );
    for (name, model_id) in [
        ("ollama-qwen3.json", "qwen3:8b"),
        ("llamacpp-qwen.json", "Qwen/Qwen2.5-7B-Instruct-GGUF:Q4_K_M"),
        ("mlx_lm-community.json", "mlx-community/Qwen3-8B-4bit"),
    ] {
        write_result(
            &results.join(name),
            2,
            model_id,
            None,
            name.split_once('-').expect("server-prefixed name").0,
            None,
            vec![scenario("pass-through", "single_call", "pass", None, None)],
        );
    }

    let generated = run_site(&results, &output, None);
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    let appendix =
        fs::read_to_string(output.join("appendix.html")).expect("generated appendix page");
    let results_json =
        fs::read_to_string(output.join("results.json")).expect("generated JSON data");
    for generated in [&index, &appendix, &results_json] {
        assert!(!generated.contains("/Users/"));
    }
    assert!(appendix.contains("sha256-deadbeef"));
    assert!(appendix.contains("custom-model.gguf"));
    assert!(index.contains("qwen3:8b"));
    assert!(index.contains("Qwen/Qwen2.5-7B-Instruct-GGUF:Q4_K_M"));
    assert!(index.contains("mlx-community/Qwen3-8B-4bit"));
}

#[test]
fn site_uses_registry_identity_and_excludes_unresolved_rows_from_grouping() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let output = directory.path().join("site");
    fs::create_dir(&results).expect("results directory");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let declared =
        fs::read(repo.join("results/ollama-qwen3-14b.json")).expect("read declared v3 fixture");
    fs::write(results.join("filename-derived-label.json"), declared)
        .expect("write declared fixture");

    let mut unresolved: Value = serde_json::from_slice(
        &fs::read(repo.join("results/ollama-gemma3-4b.json")).expect("read unresolved v3 fixture"),
    )
    .expect("parse unresolved v3 fixture");
    unresolved["metadata"]["model"]["canonical_id"] = json!("Must/NotGroup");
    fs::write(
        results.join("invented-filename-identity.json"),
        serde_json::to_vec_pretty(&unresolved).expect("encode unresolved fixture"),
    )
    .expect("write unresolved fixture");

    let generated = run_site(&results, &output, None);
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    let appendix =
        fs::read_to_string(output.join("appendix.html")).expect("generated appendix page");
    assert!(index.contains("<strong>qwen3:14b</strong>"));
    assert!(!index.contains("<strong>filename-derived-label</strong>"));
    assert!(appendix.contains("<dt>Canonical id</dt><dd><code>Qwen/Qwen3-14B</code>"));
    assert!(index.contains(">Q4_K_M / ollama</span>"));
    assert!(index.contains("title=\"Identity status: declared.\">id: declared</span>"));
    assert!(index.contains("data-cross-model-key=\"Qwen/Qwen3-14B\""));

    let unresolved_group = index
        .lines()
        .find(|line| line.contains("data-identity-status=\"unresolved\""))
        .expect("unresolved result group");
    assert!(!unresolved_group.contains("data-cross-model-key"));
    assert!(index.contains("<strong>gemma3:4b</strong>"));
    assert!(!index.contains("<strong>invented-filename-identity</strong>"));
    assert!(appendix.contains("<dt>Canonical id</dt><dd><code>not established</code>"));
    assert!(index.contains(
        "title=\"Identity status: unresolved. Provenance could not be established; excluded from cross-model comparison.\">id: unresolved</span>"
    ));
}

#[test]
fn site_uses_the_configured_repo_base_for_evidence_links() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let output = directory.path().join("site");
    fs::create_dir(&results).expect("results directory");
    write_result(
        &results.join("ollama-fixture.json"),
        2,
        "fixture:model",
        None,
        "ollama",
        Some(("Apple M4 Max, 64GB", "macOS 15.5")),
        vec![scenario(
            "single-bad",
            "single_call",
            "fail",
            Some("wrong tool call"),
            Some("evidence/fixture/single-bad.json"),
        )],
    );

    let generated = run_site(
        &results,
        &output,
        Some("https://github.com/example/willitcall/"),
    );
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    assert!(index.contains(
        "https://github.com/example/willitcall/blob/main/results/evidence/fixture/single-bad.json"
    ));
}

#[test]
fn site_uses_one_global_environment_statement_when_uniform() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let output = directory.path().join("site");
    fs::create_dir(&results).expect("results directory");
    for name in ["ollama-one.json", "ollama-two.json"] {
        write_result(
            &results.join(name),
            2,
            "fixture:model",
            None,
            "ollama",
            Some(("Apple M4 Max, 64GB", "macOS 15.5")),
            vec![scenario("single-ok", "single_call", "pass", None, None)],
        );
    }

    let generated = run_site(&results, &output, None);
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    let appendix =
        fs::read_to_string(output.join("appendix.html")).expect("generated appendix page");
    assert_eq!(index.matches("Measurement environment:").count(), 1);
    assert!(index.contains("Measurement environment: Apple M4 Max, 64GB; macOS 15.5."));
    assert_eq!(appendix.matches("<dt>Host hardware</dt>").count(), 2);
    assert_eq!(appendix.matches("<dt>Host OS</dt>").count(), 2);
}

#[test]
fn site_ignores_archive_directory() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let archive = results.join("archive");
    let output = directory.path().join("site");
    fs::create_dir_all(&archive).expect("archive directory");
    write_result(
        &results.join("ollama-published.json"),
        2,
        "fixture:model",
        None,
        "ollama",
        Some(("Apple M4 Max, 64GB", "macOS 15.5")),
        vec![scenario("single-ok", "single_call", "pass", None, None)],
    );
    fs::write(archive.join("invalid.json"), b"not JSON").expect("archived fixture");

    let generated = run_site(&results, &output, None);

    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    assert_eq!(index.matches("class=\"result-row\"").count(), 1);
    assert!(!index.contains("invalid.json"));
}

#[test]
fn all_error_category_is_not_measurable() {
    let directory = tempfile::tempdir().expect("temp directory");
    let results = directory.path().join("results");
    let output = directory.path().join("site");
    fs::create_dir(&results).expect("results directory");

    write_result(
        &results.join("ollama-gemma3-4b.json"),
        2,
        "gemma3:4b",
        None,
        "ollama",
        None,
        (0..13)
            .map(|index| {
                scenario(
                    &format!("all-error-{index}"),
                    "single_call",
                    "error",
                    Some("server returned HTTP 400"),
                    None,
                )
            })
            .collect(),
    );
    write_result(
        &results.join("llamacpp-granite3.1-dense.json"),
        2,
        "granite3.1-dense",
        None,
        "llamacpp",
        None,
        (0..13)
            .map(|index| {
                scenario(
                    &format!("all-fail-{index}"),
                    "single_call",
                    "fail",
                    Some("wrong tool call"),
                    None,
                )
            })
            .collect(),
    );

    let generated = run_site(&results, &output, None);
    assert_eq!(
        generated.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );

    let index = fs::read_to_string(output.join("index.html")).expect("generated index");
    assert!(index.contains("class=\"score not-measurable low-replication\""));
    assert!(index.contains(
        "aria-label=\"single_call: 0 passed, 0 failed, 13 errors, 0 skipped; n=1; no verdict\""
    ));
    assert!(index.contains("class=\"score none-pass low-replication\""));
    assert!(index.contains(
        "aria-label=\"single_call: 0 passed, 13 failed, 0 errors, 0 skipped; n=1; no verdict\""
    ));
    assert!(index.contains("<span class=\"measurement-state\">not measurable</span>"));
    assert!(index.contains("<i class=\"state-key state-error\"></i>execution / server error"));
}
