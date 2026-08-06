use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};

use crate::result::{
    ArtifactFormat, ArtifactMetadata, ArtifactSourceKind, IdentityStatus, ModelMetadata,
    QuantizationMetadata,
};

static EMBEDDED_EVIDENCE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../registry/evidence");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRegistry {
    pub schema_version: u32,
    pub entries: Vec<ModelRegistryEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRegistryEntry {
    pub selectors: Vec<String>,
    pub display_name: Provenanced<String>,
    pub family_id: Option<Provenanced<String>>,
    pub canonical_id: Option<CanonicalIdClaim>,
    pub parameter_count_b: Option<Provenanced<f64>>,
    pub identity_status: IdentityStatus,
    pub artifact: RegistryArtifact,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryArtifact {
    pub source_kind: Provenanced<ArtifactSourceKind>,
    pub source_id: Option<Provenanced<String>>,
    pub revision: Option<RevisionClaim>,
    pub sha256: Option<Provenanced<String>>,
    pub format: Provenanced<ArtifactFormat>,
    pub quantization: Option<Provenanced<QuantizationMetadata>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenanced<T> {
    pub value: T,
    pub provenance_ref: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalIdClaim {
    pub value: String,
    pub provenance_ref: String,
    pub corroboration: CanonicalCorroboration,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalCorroboration {
    Declared,
    Corroborated,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionClaim {
    pub value: String,
    pub provenance_ref: String,
    pub revision_scope: RevisionScope,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionScope {
    MeasuredArtifact,
    RepositoryHeadAtCapture,
}

#[derive(Debug)]
pub struct RegistryError(String);

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RegistryError {}

impl ModelRegistry {
    pub fn from_json(bytes: &[u8]) -> Result<Self, RegistryError> {
        let registry: Self = serde_json::from_slice(bytes)
            .map_err(|error| RegistryError(format!("invalid model registry: {error}")))?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn resolve(&self, raw_endpoint_selector: &str) -> ModelMetadata {
        let endpoint_id = sanitize_endpoint_id(raw_endpoint_selector);
        let entry = self.entries.iter().find(|entry| {
            entry.selectors.iter().any(|selector| {
                selector == raw_endpoint_selector || selector == endpoint_id.as_str()
            })
        });

        let Some(entry) = entry else {
            return unresolved_model(endpoint_id);
        };

        ModelMetadata {
            display_name: entry.display_name.value.clone(),
            family_id: entry.family_id.as_ref().map(|field| field.value.clone()),
            canonical_id: entry.canonical_id.as_ref().map(|field| field.value.clone()),
            parameter_count_b: entry.parameter_count_b.as_ref().map(|field| field.value),
            endpoint_id,
            identity_status: identity_status(entry),
            artifact: ArtifactMetadata {
                source_kind: entry.artifact.source_kind.value,
                source_id: entry
                    .artifact
                    .source_id
                    .as_ref()
                    .map(|field| field.value.clone()),
                revision: entry
                    .artifact
                    .revision
                    .as_ref()
                    .map(|field| field.value.clone()),
                sha256: entry
                    .artifact
                    .sha256
                    .as_ref()
                    .map(|field| field.value.clone()),
                format: entry.artifact.format.value,
                quantization: entry
                    .artifact
                    .quantization
                    .as_ref()
                    .map(|field| field.value.clone()),
            },
        }
    }

    fn validate(&self) -> Result<(), RegistryError> {
        if self.schema_version != 1 {
            return Err(RegistryError(format!(
                "unsupported model registry schema_version {}; expected 1",
                self.schema_version
            )));
        }

        let mut selectors = HashSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.selectors.is_empty() {
                return Err(RegistryError(format!(
                    "model registry entry {index} has no selectors"
                )));
            }
            for selector in &entry.selectors {
                if !is_safe_registry_selector(selector) {
                    return Err(RegistryError(format!(
                        "model registry entry {index} has unsafe selector {selector:?}"
                    )));
                }
                if !selectors.insert(selector) {
                    return Err(RegistryError(format!(
                        "duplicate model registry selector {selector:?}"
                    )));
                }
            }

            validate_nonempty(index, "display_name", &entry.display_name.value)?;
            validate_provenance(index, "display_name", &entry.display_name.provenance_ref)?;
            validate_optional_provenanced(index, "family_id", entry.family_id.as_ref())?;
            if let Some(canonical_id) = &entry.canonical_id {
                validate_nonempty(index, "canonical_id", &canonical_id.value)?;
                validate_provenance(index, "canonical_id", &canonical_id.provenance_ref)?;
            }
            if let Some(parameter_count_b) = &entry.parameter_count_b {
                if !parameter_count_b.value.is_finite() || parameter_count_b.value < 0.0 {
                    return Err(RegistryError(format!(
                        "model registry entry {index} parameter_count_b must be finite and non-negative"
                    )));
                }
                validate_provenance(
                    index,
                    "parameter_count_b",
                    &parameter_count_b.provenance_ref,
                )?;
            }

            validate_provenance(
                index,
                "artifact.source_kind",
                &entry.artifact.source_kind.provenance_ref,
            )?;
            validate_optional_provenanced(
                index,
                "artifact.source_id",
                entry.artifact.source_id.as_ref(),
            )?;
            if let Some(revision) = &entry.artifact.revision {
                validate_nonempty(index, "artifact.revision", &revision.value)?;
                validate_provenance(index, "artifact.revision", &revision.provenance_ref)?;
            }
            if let Some(sha256) = &entry.artifact.sha256 {
                if !is_sha256(&sha256.value) {
                    return Err(RegistryError(format!(
                        "model registry entry {index} artifact.sha256 must be a sha256: digest"
                    )));
                }
                validate_provenance(index, "artifact.sha256", &sha256.provenance_ref)?;
            }
            validate_provenance(
                index,
                "artifact.format",
                &entry.artifact.format.provenance_ref,
            )?;
            if let Some(quantization) = &entry.artifact.quantization {
                validate_nonempty(
                    index,
                    "artifact.quantization.label",
                    &quantization.value.label,
                )?;
                validate_provenance(index, "artifact.quantization", &quantization.provenance_ref)?;
            }

            let decided_status = identity_status(entry);
            if entry.identity_status != decided_status {
                return Err(RegistryError(format!(
                    "model registry entry {index} declares identity_status {:?}, but its evidence requires {:?}",
                    entry.identity_status, decided_status
                )));
            }
        }
        Ok(())
    }
}

pub fn sanitize_endpoint_id(raw_endpoint_selector: &str) -> String {
    if raw_endpoint_selector.is_empty() || raw_endpoint_selector.chars().any(char::is_control) {
        return "unresolved-endpoint".to_owned();
    }

    if is_local_absolute_path(raw_endpoint_selector) {
        let normalized = raw_endpoint_selector.replace('\\', "/");
        if normalized.ends_with('/') {
            return "unresolved-endpoint".to_owned();
        }
        return normalized
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && !matches!(*name, "." | ".."))
            .unwrap_or("unresolved-endpoint")
            .to_owned();
    }

    raw_endpoint_selector.to_owned()
}

fn identity_status(entry: &ModelRegistryEntry) -> IdentityStatus {
    let Some(canonical_id) = &entry.canonical_id else {
        return IdentityStatus::Unresolved;
    };
    if entry.artifact.source_kind.value == ArtifactSourceKind::LocalFile
        && entry.artifact.sha256.is_none()
    {
        return IdentityStatus::Unresolved;
    }

    let has_immutable_artifact = entry.artifact.sha256.is_some()
        || entry.artifact.revision.as_ref().is_some_and(|revision| {
            revision.revision_scope == RevisionScope::MeasuredArtifact
                && entry.artifact.source_kind.value != ArtifactSourceKind::LocalFile
        });
    if canonical_id.corroboration == CanonicalCorroboration::Corroborated && has_immutable_artifact
    {
        IdentityStatus::Verified
    } else {
        IdentityStatus::Declared
    }
}

fn unresolved_model(endpoint_id: String) -> ModelMetadata {
    ModelMetadata {
        display_name: "Unresolved model".to_owned(),
        family_id: None,
        canonical_id: None,
        parameter_count_b: None,
        endpoint_id,
        identity_status: IdentityStatus::Unresolved,
        artifact: ArtifactMetadata {
            source_kind: ArtifactSourceKind::Other,
            source_id: None,
            revision: None,
            sha256: None,
            format: ArtifactFormat::Unknown,
            quantization: None,
        },
    }
}

fn validate_optional_provenanced(
    index: usize,
    field_name: &str,
    field: Option<&Provenanced<String>>,
) -> Result<(), RegistryError> {
    if let Some(field) = field {
        validate_nonempty(index, field_name, &field.value)?;
        validate_provenance(index, field_name, &field.provenance_ref)?;
    }
    Ok(())
}

fn validate_nonempty(index: usize, field_name: &str, value: &str) -> Result<(), RegistryError> {
    if value.trim().is_empty() {
        return Err(RegistryError(format!(
            "model registry entry {index} {field_name} must not be empty"
        )));
    }
    Ok(())
}

fn validate_provenance(
    index: usize,
    field_name: &str,
    provenance_ref: &str,
) -> Result<(), RegistryError> {
    let file_ref = provenance_ref
        .split_once('#')
        .map_or(provenance_ref, |part| part.0);
    let relative = file_ref
        .strip_prefix("registry/evidence/")
        .filter(|relative| !relative.is_empty())
        .ok_or_else(|| {
            RegistryError(format!(
                "model registry entry {index} {field_name} provenance_ref must point into registry/evidence"
            ))
        })?;
    if Path::new(relative).components().count() != 1
        || !relative.ends_with(".json")
        || EMBEDDED_EVIDENCE.get_file(relative).is_none()
    {
        return Err(RegistryError(format!(
            "model registry entry {index} {field_name} provenance_ref does not name checked-in evidence"
        )));
    }
    if let Some((_, fragment)) = provenance_ref.split_once('#') {
        if !fragment.starts_with('/') || fragment.len() == 1 {
            return Err(RegistryError(format!(
                "model registry entry {index} {field_name} provenance_ref has an invalid JSON pointer"
            )));
        }
    }
    Ok(())
}

fn is_safe_registry_selector(selector: &str) -> bool {
    !selector.is_empty()
        && !selector.chars().any(char::is_control)
        && !is_local_absolute_path(selector)
}

fn is_local_absolute_path(selector: &str) -> bool {
    let bytes = selector.as_bytes();
    selector.starts_with('/')
        || selector.starts_with("~/")
        || selector.starts_with("~\\")
        || selector.starts_with("\\\\")
        || selector.starts_with("file://")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

fn is_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}
