use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::{json, Value};

const REGISTRY: &[u8] = include_bytes!("../../../registry/models-v1.json");
const V2_RESULT_FIXTURE: &[u8] = include_bytes!("fixtures/result-v2.json");

struct Fixture {
    directory: tempfile::TempDir,
    result_path: PathBuf,
    manifest_path: PathBuf,
    registry_path: PathBuf,
}

impl Fixture {
    fn new(source: &[u8], selector: &str, existing_model_id: Option<&str>) -> Self {
        let directory = tempfile::tempdir().expect("temp directory");
        let results = directory.path().join("results");
        fs::create_dir(&results).expect("results directory");
        let result_path = results.join("misleading-model-name.json");
        let mut source: Value = serde_json::from_slice(source).expect("source result JSON");
        if let Some(model_id) = existing_model_id {
            source["metadata"]["model_id"] = Value::String(model_id.to_owned());
        }
        fs::write(
            &result_path,
            serde_json::to_vec_pretty(&source).expect("encode source result"),
        )
        .expect("write source result");

        let manifest_path = directory.path().join("manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "source_schema_version": 2,
                "target_schema_version": 3,
                "entries": [{
                    "result_path": "results/misleading-model-name.json",
                    "registry_selector": selector,
                    "provenance_ref": "registry/evidence/huggingface-recovery.json#/records/6"
                }]
            }))
            .expect("encode manifest"),
        )
        .expect("write manifest");
        let registry_path = directory.path().join("registry.json");
        fs::write(&registry_path, REGISTRY).expect("write registry");
        Self {
            directory,
            result_path,
            manifest_path,
            registry_path,
        }
    }

    fn run(&self, extra: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_willitcall"));
        command
            .current_dir(self.directory.path())
            .arg("migrate-v3")
            .arg("--manifest")
            .arg(&self.manifest_path)
            .arg("--registry")
            .arg(&self.registry_path)
            .args(extra);
        command.output().expect("run migrate-v3")
    }

    fn bytes(&self) -> Vec<u8> {
        fs::read(&self.result_path).expect("read result")
    }

    fn document(&self) -> Value {
        serde_json::from_slice(&self.bytes()).expect("result JSON")
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "migrate-v3 failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_measured_facts_preserved(before: &Value, after: &Value) {
    for field in [
        "run_id",
        "timestamp",
        "willitcall_version",
        "endpoint",
        "sampling",
        "preflight_override",
        "preflight_ignored_ports",
    ] {
        assert_eq!(before["metadata"][field], after["metadata"][field]);
    }
    for field in ["preset_name", "reported_version", "quirk_flags"] {
        assert_eq!(
            before["metadata"]["server"][field],
            after["metadata"]["server"][field]
        );
    }
    assert_eq!(before["totals"], after["totals"]);
    assert_eq!(
        before["scenarios"]
            .as_array()
            .expect("source scenarios")
            .len(),
        after["scenarios"]
            .as_array()
            .expect("migrated scenarios")
            .len()
    );
    for (source, migrated) in before["scenarios"]
        .as_array()
        .expect("source scenarios")
        .iter()
        .zip(after["scenarios"].as_array().expect("migrated scenarios"))
    {
        for field in [
            "id",
            "category",
            "status",
            "failure_reason",
            "failure_class",
            "cause",
            "evidence_path",
            "evidence_hash",
            "retried",
        ] {
            assert_eq!(source[field], migrated[field], "changed {field}");
        }
        assert!(migrated["failure"].is_null());
    }
}

#[test]
fn migrates_v2_from_manifest_and_check_never_writes() {
    let fixture = Fixture::new(
        V2_RESULT_FIXTURE,
        "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF:Q3_K_M",
        Some("must-not-drive-identity"),
    );
    let before_bytes = fixture.bytes();
    let before: Value = serde_json::from_slice(&before_bytes).expect("source result JSON");

    let check = fixture.run(&["--check"]);
    assert_success(&check);
    assert_eq!(before_bytes, fixture.bytes());
    assert!(String::from_utf8_lossy(&check.stdout).contains("no files written"));

    let migration = fixture.run(&[]);
    assert_success(&migration);
    let after = fixture.document();
    assert_eq!(after["schema_version"], 3);
    assert_eq!(
        after["metadata"]["model"]["canonical_id"],
        "meta-llama/Meta-Llama-3.1-8B-Instruct"
    );
    assert_eq!(
        after["metadata"]["model"]["endpoint_id"],
        "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF:Q3_K_M"
    );
    assert_ne!(
        after["metadata"]["model"]["endpoint_id"],
        before["metadata"]["model_id"]
    );
    assert!(after["metadata"]["replication"].is_null());
    assert!(after["metadata"]["arm_fingerprint"].is_null());
    assert!(after["metadata"]["server"]["chat_template"].is_null());
    assert!(after["metadata"]["server"]["launch_config_sha256"].is_null());
    assert_measured_facts_preserved(&before, &after);
}

#[test]
fn second_migration_is_a_byte_identical_no_op() {
    let fixture = Fixture::new(
        V2_RESULT_FIXTURE,
        "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF:Q3_K_M",
        None,
    );
    assert_success(&fixture.run(&[]));
    let once = fixture.bytes();

    let second = fixture.run(&[]);
    assert_success(&second);
    assert_eq!(once, fixture.bytes());
    assert!(String::from_utf8_lossy(&second.stdout).contains("no changes required"));
}

#[test]
fn quirk_flag_carries_decode_mode_forward_and_repairs_the_old_v3_gap() {
    let fixture = Fixture::new(
        V2_RESULT_FIXTURE,
        "mlx-community/Meta-Llama-3.1-8B-Instruct-4bit",
        None,
    );
    let mut source = fixture.document();
    source["metadata"]["server"]["quirk_flags"] = json!(["unconstrained_post_hoc_parse"]);
    fs::write(
        &fixture.result_path,
        serde_json::to_vec_pretty(&source).expect("encode v2 quirk fixture"),
    )
    .expect("write v2 quirk fixture");

    assert_success(&fixture.run(&[]));
    let migrated = fixture.document();
    assert_eq!(
        migrated["metadata"]["server"]["decode_mode"],
        "unconstrained_post_hoc"
    );

    let mut old_v3 = migrated;
    old_v3["metadata"]["server"]["decode_mode"] = json!("unknown");
    fs::write(
        &fixture.result_path,
        serde_json::to_vec_pretty(&old_v3).expect("encode old v3 fixture"),
    )
    .expect("write old v3 fixture");

    assert_success(&fixture.run(&[]));
    assert_eq!(
        fixture.document()["metadata"]["server"]["decode_mode"],
        "unconstrained_post_hoc"
    );
    let repaired = fixture.bytes();
    let second = fixture.run(&[]);
    assert_success(&second);
    assert_eq!(repaired, fixture.bytes());
    assert!(String::from_utf8_lossy(&second.stdout).contains("no changes required"));
}

#[test]
fn refuses_an_unlisted_result_before_writing() {
    let fixture = Fixture::new(
        V2_RESULT_FIXTURE,
        "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF:Q3_K_M",
        None,
    );
    let before = fixture.bytes();
    let unlisted = fixture
        .result_path
        .parent()
        .expect("result parent")
        .join("unlisted.json");
    fs::write(&unlisted, V2_RESULT_FIXTURE).expect("write unlisted result");

    let output = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing unlisted result file"));
    assert_eq!(before, fixture.bytes());
}

#[test]
fn unresolved_registry_row_migrates_successfully() {
    let fixture = Fixture::new(V2_RESULT_FIXTURE, "gemma3:4b", Some("looks-resolved:99b"));

    let output = fixture.run(&[]);
    assert_success(&output);
    let migrated = fixture.document();
    assert_eq!(
        migrated["metadata"]["model"]["identity_status"],
        "unresolved"
    );
    assert!(migrated["metadata"]["model"]["canonical_id"].is_null());
    assert!(migrated["metadata"]["arm_fingerprint"].is_null());
    assert!(migrated["scenarios"]
        .as_array()
        .expect("scenarios")
        .iter()
        .all(|scenario| scenario["failure"].is_null()));
}
