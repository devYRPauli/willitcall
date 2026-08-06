use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Map, Value};
use wic_core::corpus::load_frozen_v1_catalog;
use wic_core::registry::ModelRegistry;
use wic_core::result::parse_and_validate_measurement;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

const LLAMA_A: &[&str] = &[
    "results/llamacpp-granite3.1-dense-8b.json",
    "results/llamacpp-meta-llama-3.1-8b-instruct-q3_k_m.json",
    "results/llamacpp-meta-llama-3.1-8b-instruct-q4_k_m.json",
    "results/llamacpp-meta-llama-3.1-8b-instruct-q8_0.json",
];
const LLAMA_B: &[&str] = &[
    "results/llamacpp-phi4-mini.json",
    "results/llamacpp-qwen2.5-1.5b-instruct-q3_k_m.json",
    "results/llamacpp-qwen2.5-1.5b-instruct-q4_k_m.json",
    "results/llamacpp-qwen2.5-1.5b-instruct-q8_0.json",
];
const LLAMA_C: &[&str] = &[
    "results/llamacpp-qwen2.5-7b-instruct-q3_k_m.json",
    "results/llamacpp-qwen2.5-7b-instruct-q4_k_m.json",
    "results/llamacpp-qwen2.5-7b-instruct-q8_0.json",
    "results/llamacpp-watt-tool-8b-q4_k_m.json",
];
const MLX: &[&str] = &[
    "results/mlx_lm-meta-llama-3.1-8b-instruct-4bit.json",
    "results/mlx_lm-mistral-7b-instruct-v0.3-4bit.json",
    "results/mlx_lm-phi4-mini-4bit.json",
    "results/mlx_lm-qwen2.5-1.5b-instruct-4bit.json",
    "results/mlx_lm-qwen2.5-7b-instruct-4bit.json",
    "results/mlx_lm-qwen2.5-7b-instruct-8bit.json",
];
const OLLAMA_A: &[&str] = &[
    "results/ollama-gemma3-12b.json",
    "results/ollama-gemma3-4b.json",
    "results/ollama-granite3.1-dense-8b.json",
    "results/ollama-hermes3-8b.json",
    "results/ollama-llama3-groq-tool-use-8b.json",
];
const OLLAMA_B: &[&str] = &[
    "results/ollama-llama3.1-8b.json",
    "results/ollama-mistral-7b.json",
    "results/ollama-phi4-mini.json",
    "results/ollama-qwen2.5-7b-instruct.json",
    "results/ollama-qwen3-0.6b.json",
];
const OLLAMA_C: &[&str] = &[
    "results/ollama-qwen3-1.7b.json",
    "results/ollama-qwen3-14b.json",
    "results/ollama-qwen3-4b.json",
    "results/ollama-qwen3-8b.json",
];

#[derive(Debug)]
struct ManifestEntry {
    result_path: String,
    registry_selector: String,
}

#[derive(Debug)]
struct Manifest {
    entries: Vec<ManifestEntry>,
}

#[derive(Debug)]
struct PendingWrite {
    path: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct MigrationSummary {
    pub selected: usize,
    pub changed: usize,
}

pub(crate) fn run(
    manifest_path: &Path,
    registry_path: &Path,
    batch: Option<&str>,
    check: bool,
) -> Result<MigrationSummary, String> {
    let manifest_bytes = read_file(manifest_path, "migration manifest")?;
    let manifest = parse_manifest(&manifest_bytes)?;
    let registry_bytes = read_file(registry_path, "model registry")?;
    let registry = ModelRegistry::from_json(&registry_bytes).map_err(|error| error.to_string())?;

    let resolved_paths = resolve_paths(&manifest)?;
    refuse_unlisted_json_files(&resolved_paths)?;
    let selected = select_entries(&manifest, batch)?;
    let corpus = historical_corpus()?;
    let mut pending = Vec::new();

    for entry in &selected {
        let path = resolved_paths
            .get(entry.result_path.as_str())
            .expect("all manifest entries have resolved paths");
        let expected_model = resolve_manifest_model(&registry, entry)?;
        let bytes = read_file(path, "result")?;
        let document: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("invalid result {}: {error}", path.display()))?;
        match schema_version(&document, path)? {
            2 => {
                let migrated = migrate_v2(&document, &expected_model, &corpus, path)?;
                validate_historical_v3(&migrated, &expected_model, &corpus, path)?;
                let mut output = serde_json::to_vec_pretty(&migrated)
                    .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
                output.push(b'\n');
                pending.push(PendingWrite {
                    path: path.clone(),
                    bytes: output,
                });
            }
            3 => validate_historical_v3(&document, &expected_model, &corpus, path)?,
            version => {
                return Err(format!(
                    "result {} has schema_version {version}; manifest requires source version 2 or an already-migrated v3 file",
                    path.display()
                ));
            }
        }
    }

    if !check {
        for write in &pending {
            write_atomic(&write.path, &write.bytes).map_err(|error| {
                format!("failed to write result {}: {error}", write.path.display())
            })?;
        }
    }

    Ok(MigrationSummary {
        selected: selected.len(),
        changed: pending.len(),
    })
}

fn read_file(path: &Path, kind: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path)
        .map_err(|error| format!("failed to read {kind} {}: {error}", path.display()))
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let document: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid migration manifest: {error}"))?;
    let object = required_object(&document, "migration manifest")?;
    require_keys(
        object,
        &[
            "schema_version",
            "source_schema_version",
            "target_schema_version",
            "entries",
        ],
        "migration manifest",
    )?;
    require_version(object, "schema_version", 1)?;
    require_version(object, "source_schema_version", 2)?;
    require_version(object, "target_schema_version", 3)?;
    let raw_entries = object
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| "invalid migration manifest: entries must be an array".to_owned())?;
    if raw_entries.is_empty() {
        return Err("invalid migration manifest: entries must not be empty".to_owned());
    }

    let mut entries = Vec::with_capacity(raw_entries.len());
    let mut result_paths = HashSet::new();
    for (index, entry) in raw_entries.iter().enumerate() {
        let context = format!("migration manifest entry {index}");
        let object = required_object(entry, &context)?;
        require_keys(
            object,
            &["result_path", "registry_selector", "provenance_ref"],
            &context,
        )?;
        let result_path = required_nonempty_string(object, "result_path", &context)?;
        let registry_selector = required_nonempty_string(object, "registry_selector", &context)?;
        required_nonempty_string(object, "provenance_ref", &context)?;
        if !result_paths.insert(result_path.clone()) {
            return Err(format!(
                "invalid migration manifest: duplicate result_path {result_path:?}"
            ));
        }
        entries.push(ManifestEntry {
            result_path,
            registry_selector,
        });
    }
    Ok(Manifest { entries })
}

fn required_object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("invalid {context}: expected an object"))
}

fn require_keys(
    object: &Map<String, Value>,
    expected: &[&str],
    context: &str,
) -> Result<(), String> {
    let actual = object.keys().map(String::as_str).collect::<HashSet<_>>();
    let expected = expected.iter().copied().collect::<HashSet<_>>();
    if actual != expected {
        return Err(format!(
            "invalid {context}: expected exactly the fields {}",
            expected.iter().copied().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(())
}

fn require_version(object: &Map<String, Value>, field: &str, expected: u64) -> Result<(), String> {
    let actual = object.get(field).and_then(Value::as_u64);
    if actual != Some(expected) {
        return Err(format!(
            "invalid migration manifest: {field} must be {expected}"
        ));
    }
    Ok(())
}

fn required_nonempty_string(
    object: &Map<String, Value>,
    field: &str,
    context: &str,
) -> Result<String, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid {context}: {field} must be a non-empty string"))
}

fn resolve_paths(manifest: &Manifest) -> Result<HashMap<&str, PathBuf>, String> {
    let current = std::env::current_dir()
        .map_err(|error| format!("failed to resolve current directory: {error}"))?;
    let mut paths = HashMap::new();
    let mut normalized = HashSet::new();
    for entry in &manifest.entries {
        let raw = Path::new(&entry.result_path);
        if raw.extension().is_none_or(|extension| extension != "json") {
            return Err(format!(
                "invalid migration manifest: result_path {:?} is not a JSON file",
                entry.result_path
            ));
        }
        let path = normalize_path(if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            current.join(raw)
        });
        if !normalized.insert(path.clone()) {
            return Err(format!(
                "invalid migration manifest: multiple result paths resolve to {}",
                path.display()
            ));
        }
        paths.insert(entry.result_path.as_str(), path);
    }
    Ok(paths)
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn refuse_unlisted_json_files(paths: &HashMap<&str, PathBuf>) -> Result<(), String> {
    let listed = paths.values().cloned().collect::<HashSet<_>>();
    let parents = paths
        .values()
        .filter_map(|path| path.parent().map(Path::to_path_buf))
        .collect::<HashSet<_>>();
    for parent in parents {
        let entries = std::fs::read_dir(&parent).map_err(|error| {
            format!(
                "failed to inspect result directory {}: {error}",
                parent.display()
            )
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    format!(
                        "failed to inspect result directory {}: {error}",
                        parent.display()
                    )
                })?
                .path();
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                && !listed.contains(&normalize_path(path.clone()))
            {
                return Err(format!(
                    "refusing unlisted result file {}; add it explicitly to the migration manifest",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn select_entries<'a>(
    manifest: &'a Manifest,
    batch: Option<&str>,
) -> Result<Vec<&'a ManifestEntry>, String> {
    let Some(batch) = batch else {
        return Ok(manifest.entries.iter().collect());
    };
    let expected = match batch {
        "llama-a" => LLAMA_A,
        "llama-b" => LLAMA_B,
        "llama-c" => LLAMA_C,
        "mlx" => MLX,
        "ollama-a" => OLLAMA_A,
        "ollama-b" => OLLAMA_B,
        "ollama-c" => OLLAMA_C,
        _ => {
            return Err(format!(
                "unknown migration batch {batch:?}; expected llama-a, llama-b, llama-c, mlx, ollama-a, ollama-b, or ollama-c"
            ));
        }
    };
    let by_path = manifest
        .entries
        .iter()
        .map(|entry| (entry.result_path.as_str(), entry))
        .collect::<HashMap<_, _>>();
    expected
        .iter()
        .map(|path| {
            by_path.get(path).copied().ok_or_else(|| {
                format!("migration batch {batch:?} requires manifest entry {path:?}")
            })
        })
        .collect()
}

fn resolve_manifest_model(
    registry: &ModelRegistry,
    entry: &ManifestEntry,
) -> Result<Value, String> {
    let is_listed = registry.entries.iter().any(|registry_entry| {
        registry_entry
            .selectors
            .iter()
            .any(|selector| selector == &entry.registry_selector)
    });
    if !is_listed {
        return Err(format!(
            "manifest selector {:?} for {:?} is not explicitly listed in the registry",
            entry.registry_selector, entry.result_path
        ));
    }
    serde_json::to_value(registry.resolve(&entry.registry_selector))
        .map_err(|error| format!("failed to encode registry identity: {error}"))
}

fn historical_corpus() -> Result<Value, String> {
    let catalog = load_frozen_v1_catalog().map_err(|error| error.to_string())?;
    Ok(json!({
        "id": catalog.id,
        "revision": catalog.revision,
        "sha256": catalog.sha256,
        "scenario_count": catalog.scenario_count,
        "scoring_version": "v1",
    }))
}

fn schema_version(document: &Value, path: &Path) -> Result<u64, String> {
    document
        .get("schema_version")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            format!(
                "invalid result {}: schema_version must be an unsigned integer",
                path.display()
            )
        })
}

fn migrate_v2(
    document: &Value,
    model: &Value,
    corpus: &Value,
    path: &Path,
) -> Result<Value, String> {
    let source_bytes = serde_json::to_vec(document)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    wic_core::result::parse_and_validate_result(&source_bytes)
        .map_err(|error| format!("invalid result {}: {error}", path.display()))?;
    let root = required_object(document, &format!("result {}", path.display()))?;
    let metadata = required_object(
        root.get("metadata")
            .ok_or_else(|| format!("invalid result {}: missing metadata", path.display()))?,
        &format!("result {} metadata", path.display()),
    )?;
    let server = required_object(
        metadata
            .get("server")
            .ok_or_else(|| format!("invalid result {}: missing server", path.display()))?,
        &format!("result {} server", path.display()),
    )?;
    let environment = required_object(
        metadata
            .get("environment")
            .ok_or_else(|| format!("result {} has no historical environment", path.display()))?,
        &format!("result {} environment", path.display()),
    )?;
    let hardware = environment
        .get("host_hardware_class")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "invalid result {}: environment.host_hardware_class must be a string",
                path.display()
            )
        })?;
    let host_os = environment
        .get("host_os")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!(
                "invalid result {}: environment.host_os must be a string",
                path.display()
            )
        })?;

    let scenarios = root
        .get("scenarios")
        .and_then(Value::as_array)
        .expect("validated v2 result has scenarios")
        .iter()
        .map(migrate_scenario)
        .collect::<Result<Vec<_>, _>>()?;
    let mut migrated_metadata = Map::new();
    copy_required(metadata, &mut migrated_metadata, "run_id", path)?;
    copy_required(metadata, &mut migrated_metadata, "timestamp", path)?;
    copy_required(metadata, &mut migrated_metadata, "willitcall_version", path)?;
    copy_required(metadata, &mut migrated_metadata, "endpoint", path)?;
    migrated_metadata.insert("model".to_owned(), model.clone());
    migrated_metadata.insert("corpus".to_owned(), corpus.clone());
    migrated_metadata.insert(
        "server".to_owned(),
        json!({
            "preset_name": server.get("preset_name").expect("validated v2 server"),
            "reported_version": server.get("reported_version").expect("validated v2 server"),
            "quirk_flags": server.get("quirk_flags").expect("validated v2 server"),
            "decode_mode": "unknown",
            "chat_template": null,
            "launch_config_sha256": null,
        }),
    );
    migrated_metadata.insert(
        "environment".to_owned(),
        json!({
            "display_label": format!("{hardware}; {host_os}"),
            "os_name": null,
            "os_version": null,
            "architecture": null,
            "accelerator": null,
            "memory_bytes": null,
        }),
    );
    copy_required(metadata, &mut migrated_metadata, "sampling", path)?;
    migrated_metadata.insert("replication".to_owned(), Value::Null);
    migrated_metadata.insert("arm_fingerprint".to_owned(), Value::Null);
    copy_optional(metadata, &mut migrated_metadata, "preflight_override");
    copy_optional(metadata, &mut migrated_metadata, "preflight_ignored_ports");

    Ok(json!({
        "schema_version": 3,
        "metadata": migrated_metadata,
        "scenarios": scenarios,
        "totals": root.get("totals").expect("validated v2 result has totals"),
    }))
}

fn migrate_scenario(scenario: &Value) -> Result<Value, String> {
    let source = required_object(scenario, "scenario outcome")?;
    let mut migrated = Map::new();
    for field in ["id", "category", "status", "failure_reason"] {
        copy_required(source, &mut migrated, field, Path::new("scenario outcome"))?;
    }
    migrated.insert("failure".to_owned(), Value::Null);
    copy_optional(source, &mut migrated, "failure_class");
    copy_optional(source, &mut migrated, "cause");
    for field in ["evidence_hash", "evidence_path", "retried"] {
        copy_required(source, &mut migrated, field, Path::new("scenario outcome"))?;
    }
    Ok(Value::Object(migrated))
}

fn copy_required(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    field: &str,
    path: &Path,
) -> Result<(), String> {
    let value = source
        .get(field)
        .ok_or_else(|| format!("invalid {}: missing {field}", path.display()))?;
    target.insert(field.to_owned(), value.clone());
    Ok(())
}

fn copy_optional(source: &Map<String, Value>, target: &mut Map<String, Value>, field: &str) {
    if let Some(value) = source.get(field) {
        target.insert(field.to_owned(), value.clone());
    }
}

fn validate_historical_v3(
    document: &Value,
    expected_model: &Value,
    expected_corpus: &Value,
    path: &Path,
) -> Result<(), String> {
    if schema_version(document, path)? != 3 {
        return Err(format!("result {} is not schema v3", path.display()));
    }
    let metadata = document
        .get("metadata")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "invalid result {}: metadata must be an object",
                path.display()
            )
        })?;
    if metadata.get("model") != Some(expected_model) {
        return Err(format!(
            "result {} model identity does not match its manifest registry selector",
            path.display()
        ));
    }
    if metadata.get("corpus") != Some(expected_corpus) {
        return Err(format!(
            "result {} corpus metadata is not the frozen historical corpus",
            path.display()
        ));
    }
    for field in ["replication", "arm_fingerprint"] {
        if !metadata.get(field).is_some_and(Value::is_null) {
            return Err(format!(
                "result {} historical metadata.{field} must be null",
                path.display()
            ));
        }
    }
    let server = metadata
        .get("server")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!(
                "invalid result {}: server must be an object",
                path.display()
            )
        })?;
    if server.get("decode_mode").and_then(Value::as_str) != Some("unknown")
        || !server.get("chat_template").is_some_and(Value::is_null)
        || !server
            .get("launch_config_sha256")
            .is_some_and(Value::is_null)
    {
        return Err(format!(
            "result {} invents unrecoverable historical server metadata",
            path.display()
        ));
    }
    let scenarios = document
        .get("scenarios")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            format!(
                "invalid result {}: scenarios must be an array",
                path.display()
            )
        })?;
    if scenarios
        .iter()
        .any(|scenario| !scenario.get("failure").is_some_and(Value::is_null))
    {
        return Err(format!(
            "result {} invents structured historical failure data",
            path.display()
        ));
    }

    let mut wire_compatible = document.clone();
    let compatible_metadata = wire_compatible
        .get_mut("metadata")
        .and_then(Value::as_object_mut)
        .expect("metadata was checked above");
    compatible_metadata.insert(
        "replication".to_owned(),
        json!({
            "study_id": "unresolved:historical",
            "arm_id": "unresolved:historical",
            "run_index": 0,
            "mode": "greedy_reproducibility",
        }),
    );
    compatible_metadata.insert(
        "arm_fingerprint".to_owned(),
        Value::String("unresolved:historical".to_owned()),
    );
    let bytes = serde_json::to_vec(&wire_compatible)
        .map_err(|error| format!("failed to validate {}: {error}", path.display()))?;
    parse_and_validate_measurement(&bytes)
        .map_err(|error| format!("invalid result {}: {error}", path.display()))?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("result.json");
    let suffix = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary_path = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        suffix
    ));
    let write_result = (|| {
        let mut temporary = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)?;
        temporary.write_all(bytes)?;
        temporary.flush()?;
        temporary.sync_all()?;
        std::fs::rename(&temporary_path, path)
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    write_result
}
