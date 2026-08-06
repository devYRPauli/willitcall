mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
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
const PUBLISHED_REGISTRY: &str = include_str!("../../../registry/models-v1.json");
const MIGRATION_MANIFEST: &str = include_str!("../../../migrations/result-v2-to-v3-v1.json");

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

#[test]
fn published_registry_has_32_explicit_provenance_checked_mappings() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let registry_document: Value =
        serde_json::from_str(PUBLISHED_REGISTRY).expect("parse published model registry");
    let manifest: Value =
        serde_json::from_str(MIGRATION_MANIFEST).expect("parse result migration manifest");

    validate_published_registry(&registry_document, &manifest, &repo_root)
        .expect("published registry and migration manifest must be provenance checked");

    let registry = ModelRegistry::from_json(PUBLISHED_REGISTRY.as_bytes())
        .expect("load published model registry");
    let verified = registry
        .entries
        .iter()
        .filter(|entry| entry.identity_status == IdentityStatus::Verified)
        .count();
    let declared = registry
        .entries
        .iter()
        .filter(|entry| entry.identity_status == IdentityStatus::Declared)
        .count();
    let unresolved = registry
        .entries
        .iter()
        .filter(|entry| entry.identity_status == IdentityStatus::Unresolved)
        .count();
    assert_eq!((verified, declared, unresolved), (1, 27, 4));

    let mut missing_provenance = registry_document.clone();
    missing_provenance["entries"][0]["display_name"]
        .as_object_mut()
        .expect("display_name object")
        .remove("provenance_ref");
    assert!(validate_published_registry(&missing_provenance, &manifest, &repo_root).is_err());

    let mut verified_local_without_sha = registry_document.clone();
    let verified_local = verified_local_without_sha["entries"]
        .as_array_mut()
        .expect("registry entries")
        .iter_mut()
        .find(|entry| {
            entry["identity_status"] == "verified"
                && entry["artifact"]["source_kind"]["value"] == "local_file"
        })
        .expect("verified local-file entry");
    verified_local["artifact"]["sha256"] = Value::Null;
    assert!(
        validate_published_registry(&verified_local_without_sha, &manifest, &repo_root).is_err()
    );

    let mut filename_derived = registry_document.clone();
    let first_mapping = &manifest["entries"][0];
    let selector = first_mapping["registry_selector"]
        .as_str()
        .expect("registry selector");
    let result_path = first_mapping["result_path"].as_str().expect("result path");
    let filename_stem = Path::new(result_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("result filename stem");
    let filename_derived_entry = filename_derived["entries"]
        .as_array_mut()
        .expect("registry entries")
        .iter_mut()
        .find(|entry| entry["selectors"][0] == selector)
        .expect("mapped registry entry");
    filename_derived_entry["display_name"]["value"] = json!(filename_stem);
    assert!(validate_published_registry(&filename_derived, &manifest, &repo_root).is_err());
}

fn validate_published_registry(
    registry_document: &Value,
    manifest: &Value,
    repo_root: &Path,
) -> Result<(), String> {
    validate_schema(registry_document)?;
    ModelRegistry::from_json(
        &serde_json::to_vec(registry_document).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let manifest_object = manifest
        .as_object()
        .ok_or_else(|| "migration manifest must be an object".to_owned())?;
    let expected_manifest_fields = [
        "schema_version",
        "source_schema_version",
        "target_schema_version",
        "entries",
    ];
    if manifest_object.len() != expected_manifest_fields.len()
        || expected_manifest_fields
            .iter()
            .any(|field| !manifest_object.contains_key(*field))
    {
        return Err("migration manifest has unexpected fields".to_owned());
    }
    if manifest["schema_version"] != 1
        || manifest["source_schema_version"] != 2
        || manifest["target_schema_version"] != 3
    {
        return Err("migration manifest versions must be 1, 2, and 3".to_owned());
    }

    let registry_entries = registry_document["entries"]
        .as_array()
        .ok_or_else(|| "registry entries must be an array".to_owned())?;
    let mappings = manifest["entries"]
        .as_array()
        .ok_or_else(|| "migration entries must be an array".to_owned())?;
    if registry_entries.len() != 32 || mappings.len() != 32 {
        return Err("registry and migration manifest must each have 32 entries".to_owned());
    }

    let mut registry_by_selector = BTreeMap::new();
    for entry in registry_entries {
        let selectors = entry["selectors"]
            .as_array()
            .ok_or_else(|| "registry selectors must be an array".to_owned())?;
        if selectors.len() != 1 {
            return Err("each published registry entry must have one explicit selector".to_owned());
        }
        let selector = selectors[0]
            .as_str()
            .ok_or_else(|| "registry selector must be a string".to_owned())?;
        if registry_by_selector.insert(selector, entry).is_some() {
            return Err(format!("duplicate registry selector {selector:?}"));
        }
    }

    let published_paths = published_result_paths(repo_root)?;
    let mut mapped_paths = BTreeSet::new();
    let mut mapped_selectors = BTreeSet::new();
    for mapping in mappings {
        let mapping_object = mapping
            .as_object()
            .ok_or_else(|| "migration entry must be an object".to_owned())?;
        let expected_mapping_fields = ["result_path", "registry_selector", "provenance_ref"];
        if mapping_object.len() != expected_mapping_fields.len()
            || expected_mapping_fields
                .iter()
                .any(|field| !mapping_object.contains_key(*field))
        {
            return Err("migration entry has unexpected fields".to_owned());
        }

        let result_path = mapping["result_path"]
            .as_str()
            .ok_or_else(|| "result_path must be a string".to_owned())?;
        let filename = explicit_published_filename(result_path)?;
        if !mapped_paths.insert(result_path.to_owned()) {
            return Err(format!("duplicate migration path {result_path:?}"));
        }

        let selector = mapping["registry_selector"]
            .as_str()
            .ok_or_else(|| "registry_selector must be a string".to_owned())?;
        if !mapped_selectors.insert(selector.to_owned()) {
            return Err(format!("duplicate mapped selector {selector:?}"));
        }
        let registry_entry = registry_by_selector
            .get(selector)
            .ok_or_else(|| format!("no registry entry for selector {selector:?}"))?;

        let result_bytes = fs::read(repo_root.join(result_path))
            .map_err(|error| format!("read {result_path}: {error}"))?;
        let result: Value = serde_json::from_slice(&result_bytes)
            .map_err(|error| format!("parse {result_path}: {error}"))?;
        if result["schema_version"] != 2 || result["metadata"]["model_id"] != selector {
            return Err(format!(
                "migration selector for {result_path} must equal its v2 metadata.model_id"
            ));
        }

        let mapping_provenance = mapping["provenance_ref"]
            .as_str()
            .ok_or_else(|| "mapping provenance_ref must be a string".to_owned())?;
        let recovery_record = evidence_record(repo_root, mapping_provenance)?;
        if recovery_record["subject"] != filename || recovery_record["model_id"] != selector {
            return Err(format!(
                "mapping provenance for {result_path} must record its subject and selector"
            ));
        }

        validate_registry_entry(registry_entry, filename, repo_root)?;
    }

    if mapped_paths != published_paths {
        return Err(
            "migration manifest must map every published result path exactly once".to_owned(),
        );
    }
    if mapped_selectors.len() != registry_by_selector.len() {
        return Err("every published registry entry must be used by one migration".to_owned());
    }
    Ok(())
}

fn published_result_paths(repo_root: &Path) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    for entry in fs::read_dir(repo_root.join("results")).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
        {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "published result filename must be UTF-8".to_owned())?;
        paths.insert(format!("results/{filename}"));
    }
    Ok(paths)
}

fn explicit_published_filename(result_path: &str) -> Result<&str, String> {
    let filename = result_path
        .strip_prefix("results/")
        .filter(|filename| {
            !filename.is_empty()
                && filename.ends_with(".json")
                && !filename.contains(['/', '\\'])
                && !filename
                    .chars()
                    .any(|character| matches!(character, '*' | '?' | '[' | ']' | '{' | '}'))
        })
        .ok_or_else(|| format!("migration path {result_path:?} is not an explicit result path"))?;
    Ok(filename)
}

fn validate_registry_entry(entry: &Value, filename: &str, repo_root: &Path) -> Result<(), String> {
    let fact_pointers = [
        "/display_name",
        "/family_id",
        "/canonical_id",
        "/parameter_count_b",
        "/artifact/source_kind",
        "/artifact/source_id",
        "/artifact/revision",
        "/artifact/sha256",
        "/artifact/format",
        "/artifact/quantization",
    ];
    for pointer in fact_pointers {
        let fact = entry
            .pointer(pointer)
            .ok_or_else(|| format!("registry fact {pointer} is missing"))?;
        if fact.is_null() {
            continue;
        }
        let provenance_ref = fact["provenance_ref"]
            .as_str()
            .filter(|reference| !reference.is_empty())
            .ok_or_else(|| format!("non-null registry fact {pointer} lacks provenance_ref"))?;
        evidence_record(repo_root, provenance_ref)?;
    }

    let filename_stem = filename.strip_suffix(".json").unwrap_or(filename);
    for pointer in ["/display_name", "/family_id", "/canonical_id"] {
        let fact = entry
            .pointer(pointer)
            .ok_or_else(|| format!("registry identity fact {pointer} is missing"))?;
        if fact.is_null() {
            continue;
        }
        let value = fact["value"]
            .as_str()
            .ok_or_else(|| format!("registry identity fact {pointer} must be a string"))?;
        if value == filename || value == filename_stem {
            return Err(format!(
                "registry identity fact {pointer} was derived from filename"
            ));
        }
        let provenance_ref = fact["provenance_ref"]
            .as_str()
            .ok_or_else(|| format!("registry identity fact {pointer} lacks provenance_ref"))?;
        let record = evidence_record(repo_root, provenance_ref)?;
        let supported = [
            "model_id",
            "upstream_canonical_checkpoint",
            "corroborated_ollama_model",
        ]
        .iter()
        .filter_map(|field| record[*field].as_str())
        .any(|candidate| candidate == value);
        if !supported {
            return Err(format!(
                "registry identity fact {pointer} is not stated by its recovery record"
            ));
        }
    }

    validate_artifact_source_id(entry, repo_root)?;
    validate_artifact_revision(entry, repo_root)?;
    validate_artifact_sha256(entry, repo_root)?;
    validate_artifact_classifications(entry, repo_root)?;
    validate_artifact_quantization(entry, repo_root)?;
    Ok(())
}

fn validate_artifact_source_id(entry: &Value, repo_root: &Path) -> Result<(), String> {
    let fact = &entry["artifact"]["source_id"];
    if fact.is_null() {
        return Ok(());
    }
    let value = fact["value"]
        .as_str()
        .ok_or_else(|| "artifact.source_id must be a string".to_owned())?;
    let record = evidence_record(
        repo_root,
        fact["provenance_ref"]
            .as_str()
            .ok_or_else(|| "artifact.source_id lacks provenance_ref".to_owned())?,
    )?;
    let supported = [
        "model_id",
        "artifact_repository",
        "upstream_artifact_repository",
    ]
    .iter()
    .filter_map(|field| record[*field].as_str())
    .any(|candidate| candidate == value);
    if !supported {
        return Err("artifact.source_id is not stated by its recovery record".to_owned());
    }
    Ok(())
}

fn validate_artifact_revision(entry: &Value, repo_root: &Path) -> Result<(), String> {
    let fact = &entry["artifact"]["revision"];
    if fact.is_null() {
        return Ok(());
    }
    let value = fact["value"]
        .as_str()
        .ok_or_else(|| "artifact.revision must be a string".to_owned())?;
    let record = evidence_record(
        repo_root,
        fact["provenance_ref"]
            .as_str()
            .ok_or_else(|| "artifact.revision lacks provenance_ref".to_owned())?,
    )?;
    let supported = ["revision", "manifest_digest", "upstream_artifact_revision"]
        .iter()
        .filter_map(|field| record[*field].as_str())
        .any(|candidate| candidate == value);
    if !supported {
        return Err("artifact.revision is not stated by its recovery record".to_owned());
    }
    Ok(())
}

fn validate_artifact_sha256(entry: &Value, repo_root: &Path) -> Result<(), String> {
    let fact = &entry["artifact"]["sha256"];
    if fact.is_null() {
        return Ok(());
    }
    let value = fact["value"]
        .as_str()
        .ok_or_else(|| "artifact.sha256 must be a string".to_owned())?;
    let record = evidence_record(
        repo_root,
        fact["provenance_ref"]
            .as_str()
            .ok_or_else(|| "artifact.sha256 lacks provenance_ref".to_owned())?,
    )?;
    let supported = ["sha256", "model_layer_blob_digest"]
        .iter()
        .filter_map(|field| record[*field].as_str())
        .map(|candidate| {
            if candidate.starts_with("sha256:") {
                candidate.to_owned()
            } else {
                format!("sha256:{candidate}")
            }
        })
        .any(|candidate| candidate == value);
    if !supported {
        return Err("artifact.sha256 is not stated by its recovery record".to_owned());
    }
    Ok(())
}

fn validate_artifact_classifications(entry: &Value, repo_root: &Path) -> Result<(), String> {
    let source_kind = &entry["artifact"]["source_kind"];
    let source_reference = source_kind["provenance_ref"]
        .as_str()
        .ok_or_else(|| "artifact.source_kind lacks provenance_ref".to_owned())?;
    evidence_record(repo_root, source_reference)?;
    let expected_source_kind = if source_reference.contains("huggingface-recovery.json") {
        "huggingface"
    } else if source_reference.contains("ollama-recovery.json") {
        "ollama"
    } else {
        "local_file"
    };
    if source_kind["value"] != expected_source_kind {
        return Err("artifact.source_kind disagrees with its recovery record".to_owned());
    }

    let format = &entry["artifact"]["format"];
    let format_reference = format["provenance_ref"]
        .as_str()
        .ok_or_else(|| "artifact.format lacks provenance_ref".to_owned())?;
    let record = evidence_record(repo_root, format_reference)?;
    let expected_format = if record["model_layer_blob_digest"].is_string() {
        "ollama_blob"
    } else if record["artifact_repository"]
        .as_str()
        .is_some_and(|repository| repository.starts_with("mlx-community/"))
    {
        "mlx"
    } else if record["artifact_repository"]
        .as_str()
        .or_else(|| record["upstream_artifact_repository"].as_str())
        .is_some_and(|repository| repository.ends_with("-GGUF"))
    {
        "gguf"
    } else {
        return Err("artifact.format is not supported by its recovery record".to_owned());
    };
    if format["value"] != expected_format {
        return Err("artifact.format disagrees with its recovery record".to_owned());
    }
    Ok(())
}

fn validate_artifact_quantization(entry: &Value, repo_root: &Path) -> Result<(), String> {
    let fact = &entry["artifact"]["quantization"];
    if fact.is_null() {
        return Ok(());
    }
    let label = fact["value"]["label"]
        .as_str()
        .ok_or_else(|| "artifact.quantization.label must be a string".to_owned())?;
    let record = evidence_record(
        repo_root,
        fact["provenance_ref"]
            .as_str()
            .ok_or_else(|| "artifact.quantization lacks provenance_ref".to_owned())?,
    )?;
    let supported = record["declared_quantization"]
        .as_str()
        .is_some_and(|quantization| quantization == label)
        || record["model_id"]
            .as_str()
            .is_some_and(|model_id| model_id.contains(label));
    if !supported {
        return Err("artifact.quantization is not stated by its recovery record".to_owned());
    }
    Ok(())
}

fn evidence_record(repo_root: &Path, provenance_ref: &str) -> Result<Value, String> {
    let (file_ref, pointer) = provenance_ref
        .split_once('#')
        .ok_or_else(|| format!("provenance_ref {provenance_ref:?} must point at a record"))?;
    let record_index = pointer
        .strip_prefix("/records/")
        .filter(|index| !index.is_empty() && !index.contains('/'))
        .and_then(|index| index.parse::<usize>().ok())
        .ok_or_else(|| format!("provenance_ref {provenance_ref:?} must point at records/N"))?;
    let evidence_bytes = fs::read(repo_root.join(file_ref))
        .map_err(|error| format!("read evidence {file_ref}: {error}"))?;
    let evidence: Value = serde_json::from_slice(&evidence_bytes)
        .map_err(|error| format!("parse evidence {file_ref}: {error}"))?;
    evidence["records"]
        .get(record_index)
        .filter(|record| record.is_object())
        .cloned()
        .ok_or_else(|| format!("provenance_ref {provenance_ref:?} does not name a record"))
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
