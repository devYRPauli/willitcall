use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

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
fn site_generates_v1_and_v2_rows_ratios_links_and_badges() {
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
    write_result(
        &results.join("llamacpp-blob-model.json"),
        2,
        "/models/blobs/sha256-deadbeef",
        Some("Q4_K_M"),
        "llamacpp",
        vec![
            scenario("single-ok", "single_call", "pass", None, None),
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
    let submit = fs::read_to_string(output.join("submit.html")).expect("generated submit page");
    assert!(output.join("style.css").is_file());
    assert!(output.join("site.js").is_file());
    assert_eq!(index.matches("class=\"result-row\"").count(), 2);
    assert!(index.contains("data-server=\"ollama\""));
    assert!(index.contains("data-server=\"llamacpp\""));
    assert!(index.contains("blob-model"));
    assert!(index.contains("quant: Q4_K_M"));
    assert!(index.contains("server: llama.cpp"));
    assert!(index.contains("/models/blobs/sha256-deadbeef"));
    assert!(index.contains(">1/2<"));
    assert!(index.contains(">0/1<"));
    assert!(index.contains(">1/2<"));
    assert!(index.contains(
        "https://github.com/devYRPauli/willitcall/blob/main/results/evidence/fixture/parallel-bad.json"
    ));
    assert!(index.contains(
        "https://github.com/devYRPauli/willitcall/blob/main/docs/case-studies/server-defect.md"
    ));
    assert!(index.contains("server defect"));
    assert!(index.contains("empty response"));
    assert!(index.contains("A cell measures the whole stack"));
    assert!(index.contains("single run per cell unless stated otherwise"));
    assert!(index.contains("docs/case-studies/"));
    assert!(!index.contains("<script src=\"http"));

    assert!(submit.contains("cargo run -p willitcall -- run"));
    assert!(submit.contains("--server ollama"));
    assert!(submit.contains("--server llamacpp"));
    assert!(submit.contains("preflight clean (no contention override)"));
    assert!(submit.contains("empty responses cross-checked on a second server"));
    assert!(submit.contains("CONTRIBUTING.md"));
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
