mod support;

use std::time::Duration;

use serde_json::{json, Value};
use support::MockServer;
use wic_core::client::EndpointClient;
use wic_core::load_embedded_scenarios;
use wic_core::registry::ModelRegistry;
use wic_core::result::{
    DecodeMode, IdentityStatus, Measurement, MeasurementMetadata, MeasurementServerMetadata,
    SamplingParams, Totals,
};

const LOCAL_EVIDENCE: &str = "registry/evidence/local-file-recovery.json";
const HF_EVIDENCE: &str = "registry/evidence/huggingface-recovery.json";
const SHA256: &str = "sha256:33706b165cd6777e29fdcd777ba1e09bd3b2006b428014e1bad17df3902ec1e7";

#[tokio::test]
async fn absolute_selector_is_sent_raw_but_serialized_safely() {
    let raw_selector = "/Users/alice/private-models/watt-tool-8B.Q4_K_M.gguf";
    let server = MockServer::start_scripted(raw_selector, Vec::new()).await;
    let scenario = load_embedded_scenarios()
        .expect("embedded scenarios")
        .into_iter()
        .find(|scenario| !scenario.stream)
        .expect("non-streaming scenario");
    let client = EndpointClient::new(
        server.endpoint(),
        raw_selector.to_owned(),
        Duration::from_secs(2),
        sampling(),
    );

    let _ = client
        .complete(
            &scenario,
            &[json!({"role": "user", "content": "Use the tool."})],
        )
        .await;

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], raw_selector);

    let registry = registry(local_entry(Some(SHA256), "verified"));
    let model = registry.resolve(raw_selector);
    let serialized_result = serde_json::to_value(json!({
        "schema_version": 3,
        "metadata": {"model": model}
    }))
    .expect("serialize result fixture");
    assert_eq!(
        serialized_result["metadata"]["model"]["endpoint_id"],
        "watt-tool-8B.Q4_K_M.gguf"
    );
    let serialized_bytes =
        serde_json::to_string(&serialized_result).expect("encode result fixture");
    assert!(!serialized_bytes.contains(raw_selector));
    assert!(!serialized_bytes.contains("/Users/"));
    assert!(!serialized_bytes.contains("alice"));
}

#[test]
fn identity_status_requires_corroborated_identity_and_immutable_artifact() {
    let verified =
        registry(local_entry(Some(SHA256), "verified")).resolve("watt-tool-8B.Q4_K_M.gguf");
    assert_eq!(verified.identity_status, IdentityStatus::Verified);

    let mut declared_entry = huggingface_entry("measured_artifact", "verified");
    declared_entry["canonical_id"]["corroboration"] = json!("declared");
    declared_entry["identity_status"] = json!("declared");
    let declared = registry(declared_entry).resolve("Qwen/Qwen2.5-7B-Instruct-GGUF:Q4_K_M");
    assert_eq!(declared.identity_status, IdentityStatus::Declared);
}

#[test]
fn repository_head_at_capture_is_not_an_immutable_measured_revision() {
    let registry = registry(huggingface_entry("repository_head_at_capture", "declared"));

    let model = registry.resolve("Qwen/Qwen2.5-7B-Instruct-GGUF:Q4_K_M");

    assert_eq!(model.identity_status, IdentityStatus::Declared);
    assert_eq!(
        model.artifact.revision.as_deref(),
        Some("91cad51170dc346986eccefdc2dd33a9da36ead9")
    );
}

#[test]
fn local_artifact_without_sha256_is_unresolved_and_cannot_join() {
    let entry = local_entry(None, "unresolved");
    validate_schema(&json!({"schema_version": 1, "entries": [entry.clone()]}))
        .expect("unresolved local entry should satisfy registry schema");
    let model = registry(entry).resolve("watt-tool-8B.Q4_K_M.gguf");
    assert_eq!(model.identity_status, IdentityStatus::Unresolved);
    assert_eq!(measurement_with_model(model).cross_model_key(), None);

    let declared = json!({
        "schema_version": 1,
        "entries": [local_entry(None, "declared")]
    });
    assert!(validate_schema(&declared).is_err());
    assert!(
        ModelRegistry::from_json(&serde_json::to_vec(&declared).expect("encode registry")).is_err()
    );
}

#[test]
fn unknown_selector_is_unresolved_without_name_or_filename_inference() {
    let registry = registry(local_entry(Some(SHA256), "verified"));

    let model = registry.resolve("Qwen/Looks-Like-A-Known-8B-GGUF:Q4_K_M");

    assert_eq!(model.identity_status, IdentityStatus::Unresolved);
    assert_eq!(model.display_name, "Unresolved model");
    assert_eq!(model.canonical_id, None);
    assert_eq!(model.artifact.sha256, None);
}

#[test]
fn registry_schema_requires_provenance_and_safe_explicit_selectors() {
    let valid = json!({
        "schema_version": 1,
        "entries": [local_entry(Some(SHA256), "verified")]
    });
    validate_schema(&valid).expect("registry fixture should satisfy schema");

    let mut missing_provenance = valid.clone();
    missing_provenance["entries"][0]["family_id"] = json!({"value": "watt"});
    assert!(validate_schema(&missing_provenance).is_err());

    let mut absolute_key = valid;
    absolute_key["entries"][0]["selectors"] = json!(["/Users/alice/model.gguf"]);
    assert!(validate_schema(&absolute_key).is_err());
}

fn registry(entry: Value) -> ModelRegistry {
    let document = json!({"schema_version": 1, "entries": [entry]});
    validate_schema(&document).expect("registry fixture should satisfy schema");
    ModelRegistry::from_json(&serde_json::to_vec(&document).expect("encode registry"))
        .expect("load registry")
}

fn local_entry(sha256: Option<&str>, identity_status: &str) -> Value {
    json!({
        "selectors": ["watt-tool-8B.Q4_K_M.gguf"],
        "display_name": provenanced("Watt Tool 8B", LOCAL_EVIDENCE),
        "family_id": provenanced("watt-tool", LOCAL_EVIDENCE),
        "canonical_id": {
            "value": "watt-ai/watt-tool-8B",
            "provenance_ref": LOCAL_EVIDENCE,
            "corroboration": "corroborated"
        },
        "parameter_count_b": {
            "value": 8.0,
            "provenance_ref": LOCAL_EVIDENCE
        },
        "identity_status": identity_status,
        "artifact": {
            "source_kind": provenanced("local_file", LOCAL_EVIDENCE),
            "source_id": provenanced("watt-tool-8B.Q4_K_M.gguf", LOCAL_EVIDENCE),
            "revision": null,
            "sha256": sha256.map(|value| provenanced(value, LOCAL_EVIDENCE)),
            "format": provenanced("gguf", LOCAL_EVIDENCE),
            "quantization": {
                "value": {
                    "label": "Q4_K_M",
                    "scheme": "k-quant",
                    "bits": 4
                },
                "provenance_ref": LOCAL_EVIDENCE
            }
        }
    })
}

fn huggingface_entry(revision_scope: &str, identity_status: &str) -> Value {
    json!({
        "selectors": ["Qwen/Qwen2.5-7B-Instruct-GGUF:Q4_K_M"],
        "display_name": provenanced("Qwen 2.5 7B Instruct", HF_EVIDENCE),
        "family_id": provenanced("qwen2.5", HF_EVIDENCE),
        "canonical_id": {
            "value": "Qwen/Qwen2.5-7B-Instruct",
            "provenance_ref": HF_EVIDENCE,
            "corroboration": "corroborated"
        },
        "parameter_count_b": null,
        "identity_status": identity_status,
        "artifact": {
            "source_kind": provenanced("huggingface", HF_EVIDENCE),
            "source_id": provenanced("Qwen/Qwen2.5-7B-Instruct-GGUF", HF_EVIDENCE),
            "revision": {
                "value": "91cad51170dc346986eccefdc2dd33a9da36ead9",
                "provenance_ref": HF_EVIDENCE,
                "revision_scope": revision_scope
            },
            "sha256": null,
            "format": provenanced("gguf", HF_EVIDENCE),
            "quantization": {
                "value": {
                    "label": "Q4_K_M",
                    "scheme": "k-quant",
                    "bits": 4
                },
                "provenance_ref": HF_EVIDENCE
            }
        }
    })
}

fn provenanced(value: &str, provenance_ref: &str) -> Value {
    json!({"value": value, "provenance_ref": provenance_ref})
}

fn validate_schema(document: &Value) -> Result<(), String> {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/model-registry-v1.schema.json"
    ))
    .expect("parse model registry schema");
    jsonschema::validator_for(&schema)
        .expect("compile model registry schema")
        .validate(document)
        .map_err(|error| error.to_string())
}

fn sampling() -> SamplingParams {
    SamplingParams {
        temperature: Some(0.0),
        top_p: Some(1.0),
        seed: Some(42),
        max_tokens: Some(64),
    }
}

fn measurement_with_model(model: wic_core::result::ModelMetadata) -> Measurement {
    Measurement {
        schema_version: 3,
        metadata: MeasurementMetadata {
            run_id: "fixture-run".to_owned(),
            timestamp: "2026-08-05T12:00:00Z".to_owned(),
            willitcall_version: "0.1.0".to_owned(),
            endpoint: "http://127.0.0.1:8080/v1".to_owned(),
            model,
            corpus: None,
            server: MeasurementServerMetadata {
                preset_name: "fixture".to_owned(),
                reported_version: None,
                quirk_flags: Vec::new(),
                decode_mode: DecodeMode::Unknown,
                chat_template: None,
                launch_config_sha256: None,
            },
            environment: None,
            sampling: sampling(),
            replication: None,
            arm_fingerprint: None,
            preflight_override: None,
            preflight_ignored_ports: None,
        },
        scenarios: Vec::new(),
        totals: Totals {
            total: 0,
            passed: 0,
            failed: 0,
            errors: 0,
            skipped: 0,
        },
    }
}
