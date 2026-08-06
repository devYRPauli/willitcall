use std::fmt::Write as _;
use std::path::Path;

use serde_json::{json, Value};
use wic_core::result::{
    ArtifactFormat, ArtifactSourceKind, CauseKind, DecodeMode, IdentityStatus, Status,
};

use super::data::{DecodeModeSource, ScenarioView, SiteDataset, StackRow};

pub(super) fn render_json(dataset: &SiteDataset) -> Result<String, String> {
    let stacks = dataset
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| stack_json(dataset, index, row))
        .collect::<Vec<_>>();
    let document = json!({
        "schema_version": 1,
        "row_count": dataset.rows.len(),
        "scenario_count": dataset.scenario_count,
        "stacks": stacks,
    });
    serde_json::to_string(&document)
        .map(|json| ascii_escape(&json))
        .map_err(|error| format!("failed to encode site results.json: {error}"))
}

fn stack_json(dataset: &SiteDataset, index: usize, row: &StackRow) -> Value {
    let metadata = &row.metadata;
    let model = &metadata.model;
    let artifact = &model.artifact;
    let server = &metadata.server;
    json!({
        "stack_index": index + 1,
        "result_file": row.file_name,
        "schema_version": row.schema_version,
        "run_id": metadata.run_id,
        "timestamp": metadata.timestamp,
        "willitcall_version": metadata.willitcall_version,
        "display_name": row.display_name,
        "endpoint_id": row.endpoint_display,
        "canonical_id": row.cross_model_key(),
        "family_id": model.family_id,
        "parameter_count_b": model.parameter_count_b,
        "identity_status": identity_status(model.identity_status),
        "artifact": {
            "source_kind": artifact_source(artifact.source_kind),
            "source_id": artifact.source_id.as_deref().map(safe_path_value),
            "revision": artifact.revision,
            "sha256": artifact.sha256,
            "format": artifact_format(artifact.format),
            "quantization": artifact.quantization,
        },
        "server": {
            "preset_name": server.preset_name,
            "reported_version": server.reported_version,
            "quirk_flags": server.quirk_flags,
            "recorded_decode_mode": decode_mode(server.decode_mode),
            "effective_decode_mode": decode_mode(row.decode_mode),
            "decode_mode_source": decode_source(row.decode_mode_source),
            "chat_template": server.chat_template,
            "launch_config_sha256": server.launch_config_sha256,
        },
        "corpus": metadata.corpus,
        "environment": metadata.environment,
        "sampling": metadata.sampling,
        "replication": metadata.replication,
        "replication_count": dataset.replication_count(index),
        "arm_fingerprint": metadata.arm_fingerprint,
        "preflight_override": metadata.preflight_override,
        "preflight_ignored_ports": metadata.preflight_ignored_ports,
        "scenarios": row.scenarios.iter().map(scenario_json).collect::<Vec<_>>(),
    })
}

fn scenario_json(scenario: &ScenarioView) -> Value {
    json!({
        "id": scenario.id,
        "category": scenario.category.to_string(),
        "description": scenario.description,
        "rationale": scenario.rationale,
        "status": status(scenario.status),
        "failure": scenario.failure_detail,
        "failure_reason": scenario.failure_reason,
        "failure_class": scenario.failure_class,
        "cause": scenario.cause,
        "evidence_path": scenario.evidence_path,
        "evidence_hash": scenario.evidence_hash,
        "retried": scenario.retried,
    })
}

pub(super) fn render_csv(dataset: &SiteDataset) -> String {
    const HEADERS: [&str; 31] = [
        "stack_index",
        "result_file",
        "schema_version",
        "run_id",
        "timestamp",
        "display_name",
        "endpoint_id",
        "canonical_id",
        "identity_status",
        "quantization",
        "server",
        "server_version",
        "decode_mode",
        "decode_mode_source",
        "replication_count",
        "scenario_id",
        "category",
        "description",
        "rationale",
        "status",
        "failure_reason",
        "failure_class",
        "failure_stage",
        "failure_code",
        "http_status",
        "failed_turn_index",
        "cause_kind",
        "cause_reference",
        "cause_note",
        "evidence_path",
        "evidence_hash",
    ];
    let mut csv = String::new();
    write_csv_row(&mut csv, &HEADERS);
    for (index, row) in dataset.rows.iter().enumerate() {
        for scenario in &row.scenarios {
            let model = &row.metadata.model;
            let quantization = model
                .artifact
                .quantization
                .as_ref()
                .map(|value| value.label.as_str())
                .unwrap_or("");
            let failure = scenario.failure_detail.as_ref();
            let cause = scenario.cause.as_ref();
            let values = [
                (index + 1).to_string(),
                row.file_name.clone(),
                row.schema_version.to_string(),
                row.metadata.run_id.clone(),
                row.metadata.timestamp.clone(),
                row.display_name.clone(),
                row.endpoint_display.clone(),
                row.cross_model_key().unwrap_or("").to_owned(),
                identity_status(model.identity_status).to_owned(),
                quantization.to_owned(),
                row.metadata.server.preset_name.clone(),
                row.metadata
                    .server
                    .reported_version
                    .clone()
                    .unwrap_or_default(),
                decode_mode(row.decode_mode).to_owned(),
                decode_source(row.decode_mode_source).to_owned(),
                dataset.replication_count(index).to_string(),
                scenario.id.clone(),
                scenario.category.to_string(),
                scenario.description.clone(),
                scenario.rationale.clone(),
                status(scenario.status).to_owned(),
                scenario.failure_reason.clone().unwrap_or_default(),
                scenario.failure_class.clone().unwrap_or_default(),
                failure.map(|value| value.stage.clone()).unwrap_or_default(),
                failure.map(|value| value.code.clone()).unwrap_or_default(),
                failure
                    .and_then(|value| value.http_status)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                failure
                    .and_then(|value| value.failed_turn_index)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                cause
                    .map(|value| cause_kind(value.kind))
                    .unwrap_or("")
                    .to_owned(),
                cause
                    .and_then(|value| value.reference.clone())
                    .unwrap_or_default(),
                cause
                    .and_then(|value| value.note.clone())
                    .unwrap_or_default(),
                scenario.evidence_path.clone().unwrap_or_default(),
                scenario.evidence_hash.clone().unwrap_or_default(),
            ];
            write_csv_row(&mut csv, &values);
        }
    }
    csv
}

fn write_csv_row<S: AsRef<str>>(csv: &mut String, values: &[S]) {
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            csv.push(',');
        }
        csv.push('"');
        csv.push_str(&ascii_escape(value.as_ref()).replace('"', "\"\""));
        csv.push('"');
    }
    csv.push('\n');
}

fn ascii_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii() {
            escaped.push(character);
        } else {
            for unit in character.encode_utf16(&mut [0; 2]) {
                write!(escaped, "\\u{unit:04x}").expect("write ASCII escape");
            }
        }
    }
    escaped
}

fn safe_path_value(value: &str) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        path.file_name()
            .and_then(|component| component.to_str())
            .unwrap_or("local artifact")
            .to_owned()
    } else {
        value.to_owned()
    }
}

fn identity_status(value: IdentityStatus) -> &'static str {
    match value {
        IdentityStatus::Verified => "verified",
        IdentityStatus::Declared => "declared",
        IdentityStatus::Unresolved => "unresolved",
    }
}

fn decode_mode(value: DecodeMode) -> &'static str {
    match value {
        DecodeMode::GrammarConstrained => "grammar_constrained",
        DecodeMode::UnconstrainedPostHoc => "unconstrained_post_hoc",
        DecodeMode::Unknown => "unknown",
    }
}

fn decode_source(value: DecodeModeSource) -> &'static str {
    match value {
        DecodeModeSource::Recorded => "recorded",
        DecodeModeSource::PresetMapping => "preset_mapping",
        DecodeModeSource::Unknown => "unknown",
    }
}

fn status(value: Status) -> &'static str {
    match value {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::Error => "error",
        Status::Skipped => "skipped",
    }
}

fn cause_kind(value: CauseKind) -> &'static str {
    match value {
        CauseKind::ServerDefect => "server_defect",
        CauseKind::Unknown => "unknown",
    }
}

fn artifact_source(value: ArtifactSourceKind) -> &'static str {
    match value {
        ArtifactSourceKind::Huggingface => "huggingface",
        ArtifactSourceKind::Ollama => "ollama",
        ArtifactSourceKind::LocalFile => "local_file",
        ArtifactSourceKind::Other => "other",
    }
}

fn artifact_format(value: ArtifactFormat) -> &'static str {
    match value {
        ArtifactFormat::Gguf => "gguf",
        ArtifactFormat::Mlx => "mlx",
        ArtifactFormat::Safetensors => "safetensors",
        ArtifactFormat::OllamaBlob => "ollama_blob",
        ArtifactFormat::Unknown => "unknown",
    }
}
