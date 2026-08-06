use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use wic_core::corpus::{corpus_identity, load_frozen_v1_catalog};
use wic_core::result::{
    parse_and_validate_measurement, Cause, DecodeMode, IdentityStatus, Measurement,
    MeasurementMetadata, ReplicationMode, ScenarioFailure, Status,
};
use wic_core::{load_scenarios_from_dir, Scenario, ScenarioCategory};

const DECODE_MODE_REGISTRY: &[u8] = include_bytes!("../../../../registry/decode-modes-v1.json");

pub(super) const CATEGORIES: [ScenarioCategory; 6] = [
    ScenarioCategory::SingleCall,
    ScenarioCategory::ParallelCalls,
    ScenarioCategory::Streaming,
    ScenarioCategory::ToolChoiceModes,
    ScenarioCategory::MultiTurn,
    ScenarioCategory::NegativeTrap,
];

const CATALOG_UNAVAILABLE: &str = "catalog unavailable";
const REPLICATION_GATE: usize = 5;

#[allow(dead_code)] // Brief 8 consumes the catalog prose and replicated studies.
pub(super) struct SiteDataset {
    pub(super) rows: Vec<StackRow>,
    pub(super) scenario_count: usize,
    pub(super) studies: Vec<StudyView>,
}

pub(super) struct StackRow {
    pub(super) file_name: String,
    pub(super) schema_version: u32,
    pub(super) metadata: MeasurementMetadata,
    pub(super) display_name: String,
    pub(super) endpoint_display: String,
    pub(super) decode_mode: DecodeMode,
    pub(super) decode_mode_source: DecodeModeSource,
    pub(super) scenarios: Vec<ScenarioView>,
    pub(super) category_counts: CategoryCounts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DecodeModeSource {
    Recorded,
    PresetMapping,
    Unknown,
}

struct DecodeModeRegistry {
    by_preset: HashMap<String, DecodeMode>,
}

impl DecodeModeRegistry {
    fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let document: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid decode mode registry: {error}"))?;
        let root = document
            .as_object()
            .ok_or_else(|| "invalid decode mode registry: expected an object".to_owned())?;
        if root.len() != 2 || !root.contains_key("schema_version") || !root.contains_key("entries")
        {
            return Err(
                "invalid decode mode registry: expected schema_version and entries".to_owned(),
            );
        }
        if root
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
        {
            return Err("invalid decode mode registry: schema_version must be 1".to_owned());
        }
        let entries = root
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .filter(|entries| !entries.is_empty())
            .ok_or_else(|| {
                "invalid decode mode registry: entries must be a non-empty array".to_owned()
            })?;
        let mut by_preset = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let entry = entry.as_object().ok_or_else(|| {
                format!("invalid decode mode registry entry {index}: expected an object")
            })?;
            if entry.len() != 3
                || !entry.contains_key("preset_name")
                || !entry.contains_key("decode_mode")
                || !entry.contains_key("provenance_refs")
            {
                return Err(format!(
                    "invalid decode mode registry entry {index}: expected preset_name, decode_mode, and provenance_refs"
                ));
            }
            let preset_name = entry
                .get("preset_name")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "invalid decode mode registry entry {index}: preset_name must be non-empty"
                    )
                })?;
            let decode_mode = match entry.get("decode_mode").and_then(serde_json::Value::as_str) {
                Some("grammar_constrained") => DecodeMode::GrammarConstrained,
                Some("unconstrained_post_hoc") => DecodeMode::UnconstrainedPostHoc,
                _ => {
                    return Err(format!(
                        "invalid decode mode registry entry {index}: decode_mode must establish a known mode"
                    ));
                }
            };
            entry
                .get("provenance_refs")
                .and_then(serde_json::Value::as_array)
                .filter(|references| {
                    !references.is_empty()
                        && references.iter().all(|reference| {
                            reference
                                .as_str()
                                .is_some_and(|value| !value.trim().is_empty())
                        })
                })
                .ok_or_else(|| {
                    format!(
                        "invalid decode mode registry entry {index}: provenance_refs must contain non-empty references"
                    )
                })?;
            if by_preset
                .insert(preset_name.to_owned(), decode_mode)
                .is_some()
            {
                return Err(format!(
                    "invalid decode mode registry: duplicate preset_name {preset_name:?}"
                ));
            }
        }
        Ok(Self { by_preset })
    }
}

impl StackRow {
    pub(super) fn cross_model_key(&self) -> Option<&str> {
        if self.metadata.model.identity_status == IdentityStatus::Unresolved {
            None
        } else {
            self.metadata.model.canonical_id.as_deref()
        }
    }
}

impl SiteDataset {
    pub(super) fn replication_count(&self, row_index: usize) -> usize {
        let row = &self.rows[row_index];
        let (Some(replication), Some(fingerprint)) = (
            row.metadata.replication.as_ref(),
            row.metadata.arm_fingerprint.as_ref(),
        ) else {
            return 1;
        };
        self.rows
            .iter()
            .filter(|candidate| {
                candidate.metadata.arm_fingerprint.as_ref() == Some(fingerprint)
                    && candidate
                        .metadata
                        .replication
                        .as_ref()
                        .is_some_and(|other| {
                            other.study_id == replication.study_id
                                && other.arm_id == replication.arm_id
                                && other.mode == replication.mode
                        })
            })
            .map(|candidate| {
                candidate
                    .metadata
                    .replication
                    .as_ref()
                    .expect("filtered replicated row")
                    .run_index
            })
            .collect::<HashSet<_>>()
            .len()
            .max(1)
    }
}

#[allow(dead_code)] // Aggregate fields are inputs to the analysis views in brief 8.
pub(super) struct CategoryCounts {
    pub(super) passed: usize,
    pub(super) failed: usize,
    pub(super) errors: usize,
    pub(super) skipped: usize,
    pub(super) measurement_coverage: f64,
    pub(super) macro_category_pass_rate: Option<f64>,
    categories: [StatusCounts; CATEGORIES.len()],
}

impl CategoryCounts {
    fn from_scenarios(scenarios: &[ScenarioView]) -> Self {
        let mut categories = [StatusCounts::default(); CATEGORIES.len()];
        for scenario in scenarios {
            let counts = &mut categories[category_index(scenario.category)];
            match scenario.status {
                Status::Pass => counts.passed += 1,
                Status::Fail => counts.failed += 1,
                Status::Error => counts.errors += 1,
                Status::Skipped => counts.skipped += 1,
            }
        }

        let passed = categories.iter().map(|counts| counts.passed).sum();
        let failed = categories.iter().map(|counts| counts.failed).sum();
        let errors = categories.iter().map(|counts| counts.errors).sum();
        let skipped = categories.iter().map(|counts| counts.skipped).sum();
        let total = passed + failed + errors + skipped;
        let measurement_coverage = if total == 0 {
            0.0
        } else {
            (passed + failed) as f64 / total as f64
        };
        let macro_category_pass_rate = if errors > 0 || skipped > 0 {
            None
        } else {
            let rates = categories
                .iter()
                .filter_map(|counts| {
                    let measured = counts.passed + counts.failed;
                    (measured > 0).then_some(counts.passed as f64 / measured as f64)
                })
                .collect::<Vec<_>>();
            (!rates.is_empty()).then(|| rates.iter().sum::<f64>() / rates.len() as f64)
        };

        Self {
            passed,
            failed,
            errors,
            skipped,
            measurement_coverage,
            macro_category_pass_rate,
            categories,
        }
    }

    pub(super) fn for_category(&self, category: ScenarioCategory) -> StatusCounts {
        self.categories[category_index(category)]
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct StatusCounts {
    pub(super) passed: usize,
    pub(super) failed: usize,
    pub(super) errors: usize,
    pub(super) skipped: usize,
}

impl StatusCounts {
    pub(super) fn total(self) -> usize {
        self.passed + self.failed + self.errors + self.skipped
    }
}

#[allow(dead_code)] // Description, rationale, and structured failure land in brief 8.
pub(super) struct ScenarioView {
    pub(super) id: String,
    pub(super) category: ScenarioCategory,
    pub(super) definition: Option<Scenario>,
    pub(super) description: String,
    pub(super) rationale: String,
    pub(super) status: Status,
    pub(super) failure_detail: Option<ScenarioFailure>,
    pub(super) failure_reason: Option<String>,
    pub(super) failure_class: Option<String>,
    pub(super) cause: Option<Cause>,
    pub(super) evidence_hash: Option<String>,
    pub(super) evidence_path: Option<String>,
    pub(super) retried: bool,
}

#[allow(dead_code)] // Rendered analysis views are introduced in brief 8.
pub(super) struct StudyView {
    pub(super) study_id: String,
    pub(super) arms: Vec<StudyArm>,
}

#[allow(dead_code)]
pub(super) struct StudyArm {
    pub(super) arm_id: String,
    pub(super) arm_fingerprint: String,
    pub(super) mode: ReplicationMode,
    pub(super) row_indices: Vec<usize>,
}

pub(super) fn load(
    results_directory: &Path,
    catalog_directory: Option<&Path>,
) -> Result<SiteDataset, String> {
    let decode_modes = DecodeModeRegistry::from_json(DECODE_MODE_REGISTRY)?;
    let frozen = Catalog::from_scenarios(
        load_frozen_v1_catalog()
            .map_err(|error| format!("failed to load frozen wic-50-v1 catalog: {error}"))?
            .scenarios,
    )?;
    let custom = catalog_directory
        .map(|directory| {
            let scenarios = load_scenarios_from_dir(directory).map_err(|error| {
                format!(
                    "failed to load catalog directory {}: {error}",
                    directory.display()
                )
            })?;
            Catalog::from_scenarios(scenarios)
        })
        .transpose()?;

    let mut rows = Vec::new();
    for path in result_paths(results_directory)? {
        let bytes = fs::read(&path)
            .map_err(|error| format!("failed to read result {}: {error}", path.display()))?;
        let measurement = parse_and_validate_measurement(&bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("result path {} has no file name", path.display()))?
            .to_string_lossy()
            .into_owned();
        let catalog = select_catalog(&file_name, &measurement, &frozen, custom.as_ref())?;
        rows.push(stack_row(file_name, measurement, catalog, &decode_modes)?);
    }

    let scenario_count = rows
        .iter()
        .flat_map(|row| row.scenarios.iter())
        .map(|scenario| scenario.id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let studies = study_views(&rows);
    Ok(SiteDataset {
        rows,
        scenario_count,
        studies,
    })
}

fn result_paths(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "failed to read results directory {}: {error}",
            directory.display()
        )
    })?;
    let mut paths = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("failed to read results directory entry: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
    });
    paths.sort();
    Ok(paths)
}

fn select_catalog<'a>(
    file_name: &str,
    measurement: &Measurement,
    frozen: &'a Catalog,
    custom: Option<&'a Catalog>,
) -> Result<Option<&'a Catalog>, String> {
    let Some(corpus) = measurement.metadata.corpus.as_ref() else {
        return Ok(custom);
    };
    if corpus.sha256 == frozen.sha256 {
        return Ok(Some(frozen));
    }
    let Some(custom) = custom else {
        return Ok(None);
    };
    if corpus.sha256 != custom.sha256 {
        return Err(format!(
            "result {file_name} corpus hash mismatch: result records {}, catalog computes {}",
            corpus.sha256, custom.sha256
        ));
    }
    Ok(Some(custom))
}

fn stack_row(
    file_name: String,
    measurement: Measurement,
    catalog: Option<&Catalog>,
    decode_modes: &DecodeModeRegistry,
) -> Result<StackRow, String> {
    let scenarios = join_scenarios(&file_name, &measurement.scenarios, catalog)?;
    let category_counts = CategoryCounts::from_scenarios(&scenarios);
    let display_name = safe_display_id(&measurement.metadata.model.display_name);
    let endpoint_display = safe_display_id(&measurement.metadata.model.endpoint_id);
    let (decode_mode, decode_mode_source) = effective_decode_mode(
        measurement.metadata.server.decode_mode,
        &measurement.metadata.server.preset_name,
        decode_modes,
    );
    Ok(StackRow {
        file_name,
        schema_version: measurement.schema_version,
        metadata: measurement.metadata,
        display_name,
        endpoint_display,
        decode_mode,
        decode_mode_source,
        scenarios,
        category_counts,
    })
}

fn effective_decode_mode(
    recorded: DecodeMode,
    server: &str,
    decode_modes: &DecodeModeRegistry,
) -> (DecodeMode, DecodeModeSource) {
    if recorded != DecodeMode::Unknown {
        return (recorded, DecodeModeSource::Recorded);
    }
    decode_modes
        .by_preset
        .get(server)
        .copied()
        .map(|mode| (mode, DecodeModeSource::PresetMapping))
        .unwrap_or((DecodeMode::Unknown, DecodeModeSource::Unknown))
}

fn join_scenarios(
    file_name: &str,
    outcomes: &[wic_core::result::MeasurementScenarioOutcome],
    catalog: Option<&Catalog>,
) -> Result<Vec<ScenarioView>, String> {
    outcomes
        .iter()
        .map(|outcome| {
            let definition = catalog
                .map(|catalog| {
                    catalog.scenarios.get(&outcome.id).ok_or_else(|| {
                        format!(
                            "result {file_name} references scenario {:?} absent from catalog",
                            outcome.id
                        )
                    })
                })
                .transpose()?;
            if let Some(definition) = definition {
                if definition.category != outcome.category {
                    return Err(format!(
                        "result {file_name} scenario {:?} category disagreement: result has {}, catalog has {}",
                        outcome.id, outcome.category, definition.category
                    ));
                }
            }
            let description = definition
                .map(|scenario| scenario.description.clone())
                .unwrap_or_else(|| CATALOG_UNAVAILABLE.to_owned());
            let rationale = definition
                .map(|scenario| scenario.rationale.clone())
                .unwrap_or_else(|| CATALOG_UNAVAILABLE.to_owned());
            Ok(ScenarioView {
                id: outcome.id.clone(),
                category: outcome.category,
                definition: definition.cloned(),
                description,
                rationale,
                status: outcome.status,
                failure_detail: outcome.failure.clone(),
                failure_reason: outcome.failure_reason.clone(),
                failure_class: outcome.failure_class.clone(),
                cause: outcome.cause.clone(),
                evidence_hash: outcome.evidence_hash.clone(),
                evidence_path: outcome.evidence_path.clone(),
                retried: outcome.retried,
            })
        })
        .collect()
}

struct Catalog {
    sha256: String,
    scenarios: HashMap<String, Scenario>,
}

impl Catalog {
    fn from_scenarios(scenarios: Vec<Scenario>) -> Result<Self, String> {
        let sha256 = corpus_identity(&scenarios);
        let mut by_id = HashMap::with_capacity(scenarios.len());
        for scenario in scenarios {
            let id = scenario.id.clone();
            if by_id.insert(id.clone(), scenario).is_some() {
                return Err(format!("duplicate scenario id {id:?} in catalog"));
            }
        }
        Ok(Self {
            sha256,
            scenarios: by_id,
        })
    }
}

fn study_views(rows: &[StackRow]) -> Vec<StudyView> {
    let mut candidates = BTreeMap::<(String, String), StudyArmCandidate>::new();
    for (row_index, row) in rows.iter().enumerate() {
        let (Some(replication), Some(fingerprint)) = (
            row.metadata.replication.as_ref(),
            row.metadata.arm_fingerprint.as_ref(),
        ) else {
            continue;
        };
        let candidate = candidates
            .entry((replication.study_id.clone(), replication.arm_id.clone()))
            .or_insert_with(|| StudyArmCandidate {
                fingerprint: fingerprint.clone(),
                mode: replication.mode,
                run_indices: HashSet::new(),
                row_indices: Vec::new(),
                valid: true,
            });
        if candidate.fingerprint != *fingerprint
            || candidate.mode != replication.mode
            || !candidate.run_indices.insert(replication.run_index)
        {
            candidate.valid = false;
        }
        candidate.row_indices.push(row_index);
    }

    let mut studies = BTreeMap::<String, Vec<StudyArm>>::new();
    for ((study_id, arm_id), candidate) in candidates {
        if candidate.valid && candidate.row_indices.len() >= REPLICATION_GATE {
            studies.entry(study_id).or_default().push(StudyArm {
                arm_id,
                arm_fingerprint: candidate.fingerprint,
                mode: candidate.mode,
                row_indices: candidate.row_indices,
            });
        }
    }
    studies
        .into_iter()
        .map(|(study_id, arms)| StudyView { study_id, arms })
        .collect()
}

struct StudyArmCandidate {
    fingerprint: String,
    mode: ReplicationMode,
    run_indices: HashSet<u32>,
    row_indices: Vec<usize>,
    valid: bool,
}

fn safe_display_id(value: &str) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        path.file_name()
            .and_then(|component| component.to_str())
            .unwrap_or("local model")
            .to_owned()
    } else {
        value.to_owned()
    }
}

fn category_index(category: ScenarioCategory) -> usize {
    match category {
        ScenarioCategory::SingleCall => 0,
        ScenarioCategory::ParallelCalls => 1,
        ScenarioCategory::Streaming => 2,
        ScenarioCategory::ToolChoiceModes => 3,
        ScenarioCategory::MultiTurn => 4,
        ScenarioCategory::NegativeTrap => 5,
    }
}

#[cfg(test)]
mod tests {
    use wic_core::result::{
        CorpusMetadata, DecodeMode, MeasurementScenarioOutcome, ScenarioFailure, Status,
    };
    use wic_core::result::{ReplicationMetadata, ReplicationMode};
    use wic_core::{ArgumentsMatch, Scenario, ScenarioCategory, ToolChoice};

    use super::{
        effective_decode_mode, join_scenarios, load, select_catalog, stack_row, study_views,
        Catalog, CategoryCounts, DecodeModeRegistry, DecodeModeSource, CATALOG_UNAVAILABLE,
        DECODE_MODE_REGISTRY,
    };

    fn scenario(id: &str, category: ScenarioCategory) -> Scenario {
        Scenario {
            id: id.to_owned(),
            category,
            facets: Vec::new(),
            description: format!("description for {id}"),
            rationale: format!("rationale for {id}"),
            stream: false,
            arguments_match: ArgumentsMatch::Exact,
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            turns: Vec::new(),
        }
    }

    fn outcome(id: &str, category: ScenarioCategory) -> MeasurementScenarioOutcome {
        outcome_with_status(id, category, Status::Pass)
    }

    fn outcome_with_status(
        id: &str,
        category: ScenarioCategory,
        status: Status,
    ) -> MeasurementScenarioOutcome {
        MeasurementScenarioOutcome {
            id: id.to_owned(),
            category,
            status,
            failure_reason: None,
            failure: None::<ScenarioFailure>,
            failure_class: None,
            cause: None,
            evidence_hash: None,
            evidence_path: None,
            retried: false,
        }
    }

    #[test]
    fn duplicate_scenario_ids_fail_the_catalog_join() {
        let duplicate = scenario("duplicate", ScenarioCategory::SingleCall);
        let error = Catalog::from_scenarios(vec![duplicate.clone(), duplicate])
            .err()
            .expect("duplicate catalog should fail");
        assert!(error.contains("duplicate scenario id \"duplicate\""));
    }

    #[test]
    fn result_scenario_absent_from_catalog_fails_the_join() {
        let catalog =
            Catalog::from_scenarios(vec![scenario("present", ScenarioCategory::SingleCall)])
                .expect("catalog");
        let error = join_scenarios(
            "result.json",
            &[outcome("missing", ScenarioCategory::SingleCall)],
            Some(&catalog),
        )
        .err()
        .expect("missing scenario should fail");
        assert!(error.contains("scenario \"missing\" absent from catalog"));
    }

    #[test]
    fn result_and_catalog_category_disagreement_fails_the_join() {
        let catalog = Catalog::from_scenarios(vec![scenario(
            "category-mismatch",
            ScenarioCategory::Streaming,
        )])
        .expect("catalog");
        let error = join_scenarios(
            "result.json",
            &[outcome("category-mismatch", ScenarioCategory::SingleCall)],
            Some(&catalog),
        )
        .err()
        .expect("category mismatch should fail");
        assert!(error.contains("category disagreement"));
        assert!(error.contains("result has single_call, catalog has streaming"));
    }

    #[test]
    fn supplied_catalog_hash_mismatch_fails_before_descriptions_are_attached() {
        let frozen =
            Catalog::from_scenarios(vec![scenario("frozen", ScenarioCategory::SingleCall)])
                .expect("frozen catalog");
        let custom =
            Catalog::from_scenarios(vec![scenario("custom", ScenarioCategory::SingleCall)])
                .expect("custom catalog");
        let measurement = measurement_with_corpus("sha256:not-the-custom-catalog");
        let error = select_catalog("result.json", &measurement, &frozen, Some(&custom))
            .err()
            .expect("hash mismatch should fail");
        assert!(error.contains("corpus hash mismatch"));
        assert!(error.contains("sha256:not-the-custom-catalog"));
        assert!(error.contains(&custom.sha256));
    }

    #[test]
    fn custom_corpus_without_catalog_never_falls_back_to_frozen_catalog() {
        let frozen_scenario = scenario("same-id", ScenarioCategory::SingleCall);
        let frozen = Catalog::from_scenarios(vec![frozen_scenario]).expect("frozen catalog");
        let measurement = measurement_with_corpus("sha256:custom-corpus");
        let selected = select_catalog("result.json", &measurement, &frozen, None)
            .expect("custom corpus without catalog is supported");
        assert!(selected.is_none());

        let views = join_scenarios(
            "result.json",
            &[outcome("same-id", ScenarioCategory::SingleCall)],
            selected,
        )
        .expect("catalog-unavailable view");
        assert!(views[0].definition.is_none());
        assert_eq!(views[0].description, CATALOG_UNAVAILABLE);
        assert_eq!(views[0].rationale, CATALOG_UNAVAILABLE);
    }

    #[test]
    fn category_counts_keep_four_states_and_withhold_macro_for_incomplete_measurement() {
        let outcomes = [
            outcome_with_status("pass", ScenarioCategory::SingleCall, Status::Pass),
            outcome_with_status("fail", ScenarioCategory::SingleCall, Status::Fail),
            outcome_with_status("error", ScenarioCategory::Streaming, Status::Error),
            outcome_with_status("skip", ScenarioCategory::MultiTurn, Status::Skipped),
        ];
        let scenarios = join_scenarios("result.json", &outcomes, None).expect("scenario views");
        let counts = CategoryCounts::from_scenarios(&scenarios);
        assert_eq!(
            (counts.passed, counts.failed, counts.errors, counts.skipped),
            (1, 1, 1, 1)
        );
        assert_eq!(counts.measurement_coverage, 0.5);
        assert_eq!(counts.macro_category_pass_rate, None);
    }

    #[test]
    fn cited_preset_mapping_resolves_only_explicit_entries() {
        let registry = DecodeModeRegistry::from_json(DECODE_MODE_REGISTRY)
            .expect("published decode mode registry");
        assert_eq!(
            effective_decode_mode(DecodeMode::Unknown, "llamacpp", &registry),
            (
                DecodeMode::GrammarConstrained,
                DecodeModeSource::PresetMapping
            )
        );
        for server in ["ollama", "mlx_lm"] {
            assert_eq!(
                effective_decode_mode(DecodeMode::Unknown, server, &registry),
                (
                    DecodeMode::UnconstrainedPostHoc,
                    DecodeModeSource::PresetMapping
                )
            );
        }
        assert_eq!(
            effective_decode_mode(DecodeMode::Unknown, "custom", &registry),
            (DecodeMode::Unknown, DecodeModeSource::Unknown)
        );
    }

    #[test]
    fn decode_mode_mapping_requires_citations() {
        let mut document: serde_json::Value =
            serde_json::from_slice(DECODE_MODE_REGISTRY).expect("decode mode registry JSON");
        document["entries"][0]
            .as_object_mut()
            .expect("decode mode entry")
            .remove("provenance_refs");
        assert!(DecodeModeRegistry::from_json(
            &serde_json::to_vec(&document).expect("encode registry without provenance")
        )
        .is_err());
    }

    #[test]
    fn recorded_decode_mode_is_not_overridden_by_the_preset_mapping() {
        let registry = DecodeModeRegistry::from_json(DECODE_MODE_REGISTRY)
            .expect("published decode mode registry");
        assert_eq!(
            effective_decode_mode(DecodeMode::UnconstrainedPostHoc, "llamacpp", &registry),
            (DecodeMode::UnconstrainedPostHoc, DecodeModeSource::Recorded)
        );
    }

    #[test]
    fn published_mlx_rows_use_their_recorded_quirk_fact() {
        let results = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../results");
        let dataset = load(&results, None).expect("published site dataset");
        let mlx_rows = dataset
            .rows
            .iter()
            .filter(|row| row.metadata.server.preset_name == "mlx_lm")
            .collect::<Vec<_>>();
        assert_eq!(mlx_rows.len(), 6);
        assert!(mlx_rows.iter().all(|row| {
            row.metadata
                .server
                .quirk_flags
                .iter()
                .any(|flag| flag == "unconstrained_post_hoc_parse")
                && row.metadata.server.decode_mode == DecodeMode::UnconstrainedPostHoc
                && row.decode_mode == DecodeMode::UnconstrainedPostHoc
                && row.decode_mode_source == DecodeModeSource::Recorded
        }));
    }

    #[test]
    fn study_view_admits_only_arms_with_five_distinct_consistent_runs() {
        let mut rows = (0..4)
            .map(|run_index| replicated_row(run_index, "arm-a", "v1:arm-a"))
            .collect::<Vec<_>>();
        assert!(study_views(&rows).is_empty());

        rows.push(replicated_row(4, "arm-a", "v1:arm-a"));
        let studies = study_views(&rows);
        assert_eq!(studies.len(), 1);
        assert_eq!(studies[0].study_id, "study");
        assert_eq!(studies[0].arms.len(), 1);
        assert_eq!(studies[0].arms[0].arm_id, "arm-a");
        assert_eq!(studies[0].arms[0].row_indices.len(), 5);
    }

    fn replicated_row(run_index: u32, arm_id: &str, fingerprint: &str) -> super::StackRow {
        let mut measurement = measurement_with_corpus("sha256:custom-corpus");
        measurement.metadata.replication = Some(ReplicationMetadata {
            study_id: "study".to_owned(),
            arm_id: arm_id.to_owned(),
            run_index,
            mode: ReplicationMode::GreedyReproducibility,
        });
        measurement.metadata.arm_fingerprint = Some(fingerprint.to_owned());
        let decode_modes = DecodeModeRegistry::from_json(DECODE_MODE_REGISTRY)
            .expect("published decode mode registry");
        stack_row(
            format!("result-{run_index}.json"),
            measurement,
            None,
            &decode_modes,
        )
        .expect("stack row")
    }

    fn measurement_with_corpus(sha256: &str) -> wic_core::result::Measurement {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": 3,
            "metadata": {
                "run_id": "site-data-test",
                "timestamp": "2000-01-01T00:00:00Z",
                "willitcall_version": "test",
                "endpoint": "https://fixture.invalid/v1",
                "model": {
                    "display_name": "fixture",
                    "family_id": null,
                    "canonical_id": null,
                    "parameter_count_b": null,
                    "endpoint_id": "fixture",
                    "identity_status": "unresolved",
                    "artifact": {
                        "source_kind": "other",
                        "source_id": null,
                        "revision": null,
                        "sha256": null,
                        "format": "unknown",
                        "quantization": null
                    }
                },
                "corpus": CorpusMetadata {
                    id: "custom".to_owned(),
                    revision: "v1".to_owned(),
                    sha256: sha256.to_owned(),
                    scenario_count: 1,
                    scoring_version: "v1".to_owned()
                },
                "server": {
                    "preset_name": "custom",
                    "reported_version": null,
                    "quirk_flags": [],
                    "decode_mode": "unknown",
                    "chat_template": null,
                    "launch_config_sha256": null
                },
                "environment": {
                    "display_label": "test",
                    "os_name": null,
                    "os_version": null,
                    "architecture": null,
                    "accelerator": null,
                    "memory_bytes": null
                },
                "sampling": {
                    "temperature": 0.0,
                    "top_p": 1.0,
                    "seed": 1,
                    "max_tokens": 1
                },
                "replication": null,
                "arm_fingerprint": null
            },
            "scenarios": [{
                "id": "same-id",
                "category": "single_call",
                "status": "pass",
                "failure_reason": null,
                "failure": null,
                "evidence_hash": null,
                "evidence_path": null,
                "retried": false
            }],
            "totals": {
                "total": 1,
                "passed": 1,
                "failed": 0,
                "errors": 0,
                "skipped": 0
            }
        }))
        .expect("encode measurement");
        wic_core::result::parse_and_validate_measurement(&bytes).expect("parse measurement")
    }
}
