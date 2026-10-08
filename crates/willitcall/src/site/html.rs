use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use wic_core::result::{
    ArtifactFormat, ArtifactSourceKind, CauseKind, DecodeMode, EnvironmentMetadataV3,
    IdentityStatus, ReplicationMode, Status,
};
use wic_core::ScenarioCategory;

use super::data::{DecodeModeSource, ScenarioView, SiteDataset, StackRow, CATEGORIES};
use super::svg;

// The replicated source runs behind these two case studies live off-repository, so
// SiteDataset cannot derive their historical run and arm counts.
const CASE_STUDY_SAMPLE_SUMMARY: &str =
    "90 runs across 18 quantization arms, and 40 runs across 8 arms for the peg-native anomaly";

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Page {
    Matrix,
    Outcomes,
    Appendix,
    Submit,
}

pub(super) struct PageShell<'a> {
    pub description: &'a str,
    pub title: &'a str,
    pub current_page: Page,
    pub main_class: Option<&'a str>,
    pub main: &'a str,
    pub footer: &'a str,
    pub script: Option<&'a str>,
}

pub(super) fn render_page_shell(shell: PageShell<'_>) -> String {
    let main_class = shell
        .main_class
        .map(|class| format!(" class=\"{}\"", escape_html(class)))
        .unwrap_or_default();
    let script = shell.script.unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="description" content="{}">
  <title>{}</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <header class="site-header">
    <a class="wordmark" href="index.html">willitcall</a>
{}
  </header>
  <main{main_class}>
{}
  </main>
{}{}</body>
</html>
"#,
        escape_html(shell.description),
        escape_html(shell.title),
        render_nav(shell.current_page),
        shell.main,
        shell.footer,
        script
    )
}

pub(super) fn render_nav(current_page: Page) -> String {
    match current_page {
        Page::Matrix => {
            r#"    <nav aria-label="Primary navigation">
      <a aria-current="page" href="index.html">Matrix</a>
      <a href="outcomes.html">Outcomes</a>
      <a href="appendix.html">Appendix</a>
      <a href="submit.html">Submit a result</a>
    </nav>"#
        }
        Page::Outcomes => {
            r#"    <nav aria-label="Primary navigation">
      <a href="index.html">Matrix</a>
      <a aria-current="page" href="outcomes.html">Outcomes</a>
      <a href="appendix.html">Appendix</a>
      <a href="submit.html">Submit a result</a>
    </nav>"#
        }
        Page::Appendix => {
            r#"    <nav aria-label="Primary navigation">
      <a href="index.html">Matrix</a>
      <a href="outcomes.html">Outcomes</a>
      <a aria-current="page" href="appendix.html">Appendix</a>
      <a href="submit.html">Submit a result</a>
    </nav>"#
        }
        Page::Submit => {
            r#"    <nav aria-label="Primary navigation">
      <a href="index.html">Matrix</a>
      <a href="outcomes.html">Outcomes</a>
      <a href="appendix.html">Appendix</a>
      <a aria-current="page" href="submit.html">Submit a result</a>
    </nav>"#
        }
    }
    .to_owned()
}

/// The only HTML-renderer seam for figures. Every SVG variant carries the shared
/// accessibility contract enforced by `svg::render_figure`.
#[allow(dead_code)] // Brief 8 supplies the first figure after this seam is established.
pub(super) fn render_figure(figure: svg::Figure<'_>) -> String {
    svg::render_figure(figure)
}

pub(super) fn render_index(dataset: &SiteDataset, repo_base: &str) -> String {
    let results = &dataset.rows;
    let uniform_environment = results
        .first()
        .and_then(|result| result.metadata.environment.as_ref())
        .filter(|environment| {
            results.iter().all(|result| {
                result
                    .metadata
                    .environment
                    .as_ref()
                    .is_some_and(|candidate| same_environment(candidate, environment))
            })
        });
    let mut main = String::new();
    write!(
        main,
        r#"    <section class="register-intro" aria-labelledby="page-title">
      <p class="eyebrow">Measurement register</p>
      <h1 id="page-title">Tool-calling support matrix</h1>
      <p class="framing">Each row records one tested stack. The stack includes the model and its artifact. It also includes the server and decode mode. The matrix does not rank models or state verdicts.</p>
    </section>"#,
    )
    .expect("write HTML");
    main.push_str(&render_table(dataset, repo_base));
    main.push_str(&super::analysis::render_pass_counts(dataset));
    main.push_str(&render_methodology(dataset, repo_base, uniform_environment));

    let footer = render_colophon(dataset, &main);

    render_page_shell(PageShell {
        description: "Measured tool-calling support by model, quant, server, and server version.",
        title: "willitcall support matrix",
        current_page: Page::Matrix,
        main_class: None,
        main: &main,
        footer: &footer,
        script: Some("  <script src=\"site.js\"></script>\n"),
    })
}

pub(super) fn render_outcomes(dataset: &SiteDataset) -> String {
    let mut main = String::from(
        r#"    <section class="register-intro" aria-labelledby="page-title">
      <p class="eyebrow">Evidence layer</p>
      <h1 id="page-title">Observed outcomes</h1>
      <p class="framing">These rasters show the scenario-level evidence behind the matrix. They keep the same row order as the matrix. They do not rank observations from a single run.</p>
      <p>Each mark shows one scenario outcome for one published stack. The data files include scenario descriptions and rationales. They also contain status text and evidence paths. They provide citation details. The appendix includes recorded stack metadata and transcript links.</p>
    </section>"#,
    );
    main.push_str(&super::analysis::render_outcomes(dataset));
    let footer = render_colophon(dataset, &main);
    render_page_shell(PageShell {
        description: "Scenario-level evidence behind the willitcall support matrix.",
        title: "Observed outcomes - willitcall",
        current_page: Page::Outcomes,
        main_class: None,
        main: &main,
        footer: &footer,
        script: None,
    })
}

pub(super) fn render_appendix_page(dataset: &SiteDataset, repo_base: &str) -> String {
    let mut main = String::from(
        r#"    <section class="register-intro" aria-labelledby="page-title">
      <p class="eyebrow">Evidence register</p>
      <h1 id="page-title">Per-stack appendix</h1>
      <p class="framing">Recorded metadata and scenario-level transcript links for every published stack.</p>
    </section>"#,
    );
    main.push_str(&render_appendix(dataset, repo_base));
    let footer = render_colophon(dataset, &main);
    render_page_shell(PageShell {
        description: "Recorded metadata and transcript links for published willitcall stacks.",
        title: "Per-stack appendix - willitcall",
        current_page: Page::Appendix,
        main_class: None,
        main: &main,
        footer: &footer,
        script: None,
    })
}

fn render_reading_legend(dataset: &SiteDataset) -> String {
    let replication_note = if !dataset.rows.is_empty()
        && (0..dataset.rows.len()).all(|index| dataset.replication_count(index) == 1)
    {
        "n=1, no verdict: every published arm is a single run, so no cell carries a verdict; hatching marks this."
    } else {
        "Hatching marks cells with fewer than five runs; those cells carry no verdict."
    };
    format!(
        r#"      <div class="reading-key" aria-label="How to read the matrix">
        <p class="reading-caption"><strong>How to read:</strong> Each ratio shows passed scenarios / all scenarios in that category.</p>
        <div class="mark-key" aria-label="Scenario outcome marks">
          <span><i class="state-key state-pass"></i>pass</span>
          <span><i class="state-key state-fail"></i>model / response failure</span>
          <span><i class="state-key state-error"></i>execution / server error</span>
          <span><i class="state-key state-skipped"></i>not tested</span>
        </div>
        <div class="method-key">
          <span><i class="hatch-key"></i>{replication_note}</span>
          <span><i class="identity-status verified"></i>identity status</span>
        </div>
      </div>
"#,
    )
}

pub(super) fn render_table(dataset: &SiteDataset, repo_base: &str) -> String {
    let mut html = String::new();
    write!(
        html,
        r#"    <section class="matrix" aria-labelledby="matrix-title">
      <div class="matrix-heading">
        <div>
          <p class="eyebrow">Observed stacks</p>
          <h2 id="matrix-title">Capability matrix</h2>
        </div>
        <label for="model-search">Find a model
          <input id="model-search" type="search" autocomplete="off" placeholder="e.g. Qwen2.5" aria-describedby="filter-status">
        </label>
      </div>
{}
      <p id="filter-status" class="filter-status" aria-live="polite">Showing {} stacks.</p>
      <div class="table-scroll">
        <table class="matrix-table">
          <thead>
            <tr>
              <th scope="col">Model / quant / server</th>
"#,
        render_reading_legend(dataset),
        dataset.rows.len()
    )
    .expect("write HTML");
    for category in CATEGORIES {
        writeln!(
            html,
            "              <th scope=\"col\" title=\"{}\">{}</th>",
            category,
            category_label(category)
        )
        .expect("write HTML");
    }
    html.push_str(
        r#"            </tr>
          </thead>
"#,
    );
    for (model_key, row_indices) in grouped_row_indices(dataset) {
        let first = &dataset.rows[row_indices[0]];
        let model_label = model_group_label(first);
        let search_text = model_search_text(dataset, &row_indices);
        let group_class = if row_indices.len() > 1 {
            "multi-row"
        } else {
            "single-row"
        };
        writeln!(
            html,
            "          <tbody class=\"model-group result-group {group_class}\" data-model-key=\"{}\" data-model-search=\"{}\">",
            escape_html(&model_key),
            escape_html(&search_text),
        )
        .expect("write model group");
        if row_indices.len() > 1 {
            writeln!(
                html,
                "            <tr class=\"model-heading\"><th colspan=\"{}\" scope=\"rowgroup\"><span>Model</span> {}</th></tr>",
                CATEGORIES.len() + 1,
                escape_html(&model_label),
            )
            .expect("write model heading");
        }
        let spans_decode_strata = model_spans_decode_strata(dataset, &row_indices);
        let mut current_band = None;
        for index in row_indices {
            let row = &dataset.rows[index];
            if spans_decode_strata && current_band != Some(row.decode_mode) {
                render_decode_boundary(&mut html, row.decode_mode);
                current_band = Some(row.decode_mode);
            }
            render_result_row(&mut html, dataset, index, row, repo_base);
        }
        html.push_str("          </tbody>\n");
    }

    html.push_str(
        r#"        </table>
      </div>
      <p class="matrix-note">Machine-readable observations: <a href="results.json">JSON</a> and <a href="results.csv">CSV</a>. Scenario rasters are on the <a href="outcomes.html">outcomes page</a>; recorded metadata and transcripts are in the <a href="appendix.html">appendix</a>.</p>
    </section>"#,
    );
    html
}

fn render_decode_boundary(html: &mut String, decode_mode: DecodeMode) {
    let id = decode_mode_id(decode_mode);
    let note = match decode_mode {
        DecodeMode::GrammarConstrained => "tool grammar constrains generation",
        DecodeMode::UnconstrainedPostHoc => "generated text is parsed after decoding",
        DecodeMode::Unknown => "decode behavior was not established",
    };
    writeln!(
        html,
        "            <tr class=\"decode-band {id}\" data-decode-mode=\"{id}\"><th colspan=\"{}\" scope=\"rowgroup\"><span class=\"decode-badge {id}\">{id}</span><span>{note}</span></th></tr>",
        CATEGORIES.len() + 1,
    )
    .expect("write decode band");
}

fn render_result_row(
    html: &mut String,
    dataset: &SiteDataset,
    index: usize,
    result: &StackRow,
    repo_base: &str,
) {
    let server = &result.metadata.server.preset_name;
    let server_display = display_server(server);
    let model = &result.metadata.model;
    let identity_status = display_identity_status(model.identity_status);
    let row_label = if model.identity_status == IdentityStatus::Unresolved {
        format!("{} (unverified artifact)", result.display_name)
    } else {
        result.display_name.clone()
    };
    let cross_model_attribute = result
        .cross_model_key()
        .map(|key| format!(" data-cross-model-key=\"{}\"", escape_html(key)))
        .unwrap_or_default();
    let identity_title = match model.identity_status {
        IdentityStatus::Unresolved => "Identity status: unresolved. Provenance could not be established; excluded from cross-model comparison.",
        IdentityStatus::Declared => "Identity status: declared.",
        IdentityStatus::Verified => "Identity status: verified.",
    };
    let quant = result
        .metadata
        .model
        .artifact
        .quantization
        .as_ref()
        .map(|quantization| quantization.label.as_str())
        .unwrap_or("not declared");
    let decode_title = format!(
        "Decode mode: {}; decode provenance: {}.",
        decode_mode_label(result.decode_mode),
        decode_mode_source_label(result.decode_mode_source),
    );
    write!(
        html,
        "            <tr class=\"result-row\" data-server=\"{}\" data-identity-status=\"{}\" data-decode-mode=\"{}\" data-decode-source=\"{}\"{}>\n              <th scope=\"row\">\n                <strong>{}</strong>\n                <div class=\"row-meta\"><span>{} / {}</span><span class=\"decode-badge {}\" title=\"{}\">{} ({})</span><span class=\"identity-status {}\" title=\"{}\">id: {}</span><a class=\"detail-link\" href=\"appendix.html#stack-detail-{index}\">detail</a></div>\n              </th>\n",
        escape_html(server),
        identity_status,
        decode_mode_id(result.decode_mode),
        decode_mode_source_id(result.decode_mode_source),
        cross_model_attribute,
        escape_html(&row_label),
        escape_html(quant),
        escape_html(server_display),
        decode_mode_id(result.decode_mode),
        escape_html(&decode_title),
        decode_mode_short_label(result.decode_mode),
        decode_mode_source_short_label(result.decode_mode_source),
        identity_status,
        escape_html(identity_title),
        identity_status,
    )
    .expect("write HTML");

    let replication_count = dataset.replication_count(index);
    for category in CATEGORIES {
        render_category_cell(html, result, category, repo_base, replication_count);
    }
    html.push_str("            </tr>\n");
}

fn grouped_row_indices(dataset: &SiteDataset) -> Vec<(String, Vec<usize>)> {
    let mut groups = BTreeMap::<String, (String, Vec<usize>)>::new();
    for (index, row) in dataset.rows.iter().enumerate() {
        let label = model_group_label(row);
        groups
            .entry(label.to_ascii_lowercase())
            .or_insert_with(|| (label, Vec::new()))
            .1
            .push(index);
    }
    groups
        .into_values()
        .map(|(label, mut indices)| {
            indices.sort_by_key(|index| {
                let row = &dataset.rows[*index];
                let quant = row
                    .metadata
                    .model
                    .artifact
                    .quantization
                    .as_ref()
                    .map(|value| value.label.to_ascii_lowercase())
                    .unwrap_or_default();
                (
                    decode_order(row.decode_mode),
                    row.metadata.server.preset_name.to_ascii_lowercase(),
                    quant,
                    row.display_name.to_ascii_lowercase(),
                    row.file_name.clone(),
                )
            });
            (label, indices)
        })
        .collect()
}

fn model_group_label(row: &StackRow) -> String {
    row.cross_model_key()
        .unwrap_or(&row.display_name)
        .to_owned()
}

fn model_spans_decode_strata(dataset: &SiteDataset, indices: &[usize]) -> bool {
    indices.iter().enumerate().any(|(position, left_index)| {
        let left = &dataset.rows[*left_index];
        indices[position + 1..].iter().any(|right_index| {
            let right = &dataset.rows[*right_index];
            left.decode_mode != right.decode_mode
                && left.metadata.model.artifact.source_kind
                    == right.metadata.model.artifact.source_kind
                && left.metadata.model.artifact.source_id == right.metadata.model.artifact.source_id
                && left.metadata.model.artifact.revision == right.metadata.model.artifact.revision
                && left.metadata.model.artifact.sha256 == right.metadata.model.artifact.sha256
                && left.metadata.model.artifact.format == right.metadata.model.artifact.format
                && left
                    .metadata
                    .model
                    .artifact
                    .quantization
                    .as_ref()
                    .map(|value| (&value.label, &value.scheme, value.bits))
                    == right
                        .metadata
                        .model
                        .artifact
                        .quantization
                        .as_ref()
                        .map(|value| (&value.label, &value.scheme, value.bits))
        })
    })
}

fn model_search_text(dataset: &SiteDataset, indices: &[usize]) -> String {
    let mut terms = BTreeSet::new();
    for index in indices {
        let row = &dataset.rows[*index];
        terms.insert(row.display_name.to_ascii_lowercase());
        terms.insert(row.endpoint_display.to_ascii_lowercase());
        if let Some(key) = row.cross_model_key() {
            terms.insert(key.to_ascii_lowercase());
        }
    }
    terms.into_iter().collect::<Vec<_>>().join(" ")
}

fn decode_order(mode: DecodeMode) -> u8 {
    match mode {
        DecodeMode::GrammarConstrained => 0,
        DecodeMode::UnconstrainedPostHoc => 1,
        DecodeMode::Unknown => 2,
    }
}

fn decode_mode_id(mode: DecodeMode) -> &'static str {
    match mode {
        DecodeMode::GrammarConstrained => "grammar_constrained",
        DecodeMode::UnconstrainedPostHoc => "unconstrained_post_hoc",
        DecodeMode::Unknown => "unknown",
    }
}

fn decode_mode_label(mode: DecodeMode) -> &'static str {
    match mode {
        DecodeMode::GrammarConstrained => "grammar constrained",
        DecodeMode::UnconstrainedPostHoc => "unconstrained post-hoc",
        DecodeMode::Unknown => "unknown",
    }
}

fn decode_mode_short_label(mode: DecodeMode) -> &'static str {
    match mode {
        DecodeMode::GrammarConstrained => "grammar",
        DecodeMode::UnconstrainedPostHoc => "post-hoc",
        DecodeMode::Unknown => "unknown",
    }
}

fn decode_mode_source_id(source: DecodeModeSource) -> &'static str {
    match source {
        DecodeModeSource::Recorded => "recorded",
        DecodeModeSource::PresetMapping => "preset_mapping",
        DecodeModeSource::Unknown => "unknown",
    }
}

fn decode_mode_source_label(source: DecodeModeSource) -> &'static str {
    match source {
        DecodeModeSource::Recorded => "recorded by run",
        DecodeModeSource::PresetMapping => "documented preset mapping",
        DecodeModeSource::Unknown => "unknown (no cited mapping)",
    }
}

fn decode_mode_source_short_label(source: DecodeModeSource) -> &'static str {
    match source {
        DecodeModeSource::Recorded => "run",
        DecodeModeSource::PresetMapping => "mapping",
        DecodeModeSource::Unknown => "source unknown",
    }
}

fn category_label(category: ScenarioCategory) -> &'static str {
    match category {
        ScenarioCategory::SingleCall => "Single call",
        ScenarioCategory::ToolChoiceModes => "Tool choice",
        ScenarioCategory::NegativeTrap => "Correctly declines",
        ScenarioCategory::MultiTurn => "Multi-turn",
        ScenarioCategory::ParallelCalls => "Parallel calls",
        ScenarioCategory::Streaming => "Streaming",
    }
}

fn same_environment(left: &EnvironmentMetadataV3, right: &EnvironmentMetadataV3) -> bool {
    left.display_label == right.display_label
        && left.os_name == right.os_name
        && left.os_version == right.os_version
        && left.architecture == right.architecture
        && left.accelerator == right.accelerator
        && left.memory_bytes == right.memory_bytes
}

fn environment_display_parts(environment: &EnvironmentMetadataV3) -> (&str, &str) {
    environment
        .display_label
        .split_once("; ")
        .unwrap_or((&environment.display_label, "not recorded"))
}

fn render_environment_statement(environment: &EnvironmentMetadataV3) -> String {
    format!(
        "      <p>All measurements used {}.</p>",
        escape_html(&environment.display_label)
    )
}

fn render_methodology(
    dataset: &SiteDataset,
    repo_base: &str,
    uniform_environment: Option<&EnvironmentMetadataV3>,
) -> String {
    let case_studies_url = format!("{repo_base}/tree/main/docs/case-studies");
    let peg_native_case_study_url = format!(
        "{repo_base}/blob/main/docs/case-studies/2026-07-21-llamacpp-500s-on-llama-3.1-tool-calls.md"
    );
    let decode_mapping_url = format!("{repo_base}/blob/main/registry/decode-modes-v1.json");
    let environment_statement = uniform_environment
        .map(render_environment_statement)
        .unwrap_or_default();
    format!(
        r#"    <section class="methods" id="method-limitations" aria-labelledby="method-title">
      <p class="eyebrow">Calibration notes</p>
      <h2 id="method-title">Method and limitations</h2>
      <p>Each cell measures one full stack. The stack combines a model, quant, server, and server version. A cell does not describe the model alone.</p>
      <p>A failed observation applies only to the tested combination. It does not mean the weights are bad. The same weights can pass on one server and fail on another. When the evidence proves that difference, the cell includes a cause annotation.</p>
      <p>When the result includes a transcript path, a failing observation links to the full request and response transcript. Legacy schema v1 results do not record transcript paths. Read the <a href="{}">case studies in docs/case-studies/</a> for controlled comparisons.</p>
      <p>The servers use different decode methods. llama.cpp compiles the supplied tool definitions into a GBNF grammar. It uses that grammar to constrain decoding. MLX LM, and Ollama with a Go template, generate unconstrained text. They parse the tool call after decoding. Cross-band differences therefore reflect the full stack. Compare adjacent models only when they use the same server. Each row states whether the run recorded its decode mode or the site read the mode from the <a href="{}">cited preset mapping</a>. An unmapped preset has an unknown mode.</p>
      <p>The site includes {} distinct scenarios. Each published cell represents one run. Hatching marks that the cell has no verdict.</p>
      <p>The case studies draw a verdict only after at least five runs per arm. The current case studies cover {}.</p>
      <h3>Excluded rows</h3>
      <p>The quantization conclusion excludes Meta-Llama-3.1-8B-Instruct on llama.cpp (Q8_0, Q4_K_M, Q3_K_M). For this model, llama.cpp returns HTTP 500 on 7-9 of 50 scenarios per run ("does not match the expected peg-native format"). These results are server errors. They are not model failures and cannot be compared across arms. Read the <a href="{}">peg-native case study</a>.</p>
{}
    </section>
"#,
        escape_html(&case_studies_url),
        escape_html(&decode_mapping_url),
        dataset.scenario_count,
        CASE_STUDY_SAMPLE_SUMMARY,
        escape_html(&peg_native_case_study_url),
        environment_statement,
    )
}

fn render_appendix(dataset: &SiteDataset, repo_base: &str) -> String {
    let mut html = String::from(
        "    <section class=\"appendix\" id=\"stack-appendix\" aria-labelledby=\"appendix-title\">\n      <h2 id=\"appendix-title\">Recorded stacks</h2>\n      <p>Full recorded metadata and scenario-level evidence, in the same model and decode order as the matrix. Machine-readable observations are available as <a href=\"results.json\">JSON</a> and <a href=\"results.csv\">CSV</a>.</p>\n",
    );
    for (_, row_indices) in grouped_row_indices(dataset) {
        for index in row_indices {
            render_stack_detail(&mut html, dataset, index, &dataset.rows[index], repo_base);
        }
    }
    html.push_str("    </section>\n");
    html
}

fn render_stack_detail(
    html: &mut String,
    dataset: &SiteDataset,
    index: usize,
    result: &StackRow,
    repo_base: &str,
) {
    let metadata = &result.metadata;
    let model = &metadata.model;
    let artifact = &model.artifact;
    let server = &metadata.server;
    let environment = metadata.environment.as_ref();
    writeln!(
        html,
        "      <article class=\"stack-detail\" id=\"stack-detail-{index}\">\n        <header><p class=\"detail-index\">Stack {:02}</p><h3><a href=\"#stack-detail-{index}\">{}</a></h3><span class=\"decode-badge {}\">{}</span></header>\n        <div class=\"stack-content\">\n        <dl class=\"metadata\">",
        index + 1,
        escape_html(&result.display_name),
        decode_mode_id(result.decode_mode),
        decode_mode_id(result.decode_mode),
    )
    .expect("write stack detail");
    metadata_item(html, "Result file", &result.file_name, true);
    metadata_item(
        html,
        "Schema",
        &format!("v{}", result.schema_version),
        false,
    );
    metadata_item(html, "Run id", recorded(&metadata.run_id), true);
    metadata_item(html, "Run time", &metadata.timestamp, false);
    metadata_item(
        html,
        "willitcall version",
        &metadata.willitcall_version,
        true,
    );
    metadata_item(html, "Endpoint id", &result.endpoint_display, true);
    metadata_item(
        html,
        "Canonical id",
        result.cross_model_key().unwrap_or("not established"),
        true,
    );
    metadata_item(
        html,
        "Family id",
        model.family_id.as_deref().unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Parameters",
        &model
            .parameter_count_b
            .map(|value| format!("{value}B"))
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Identity status",
        display_identity_status(model.identity_status),
        false,
    );
    metadata_item(
        html,
        "Artifact source",
        display_artifact_source(artifact.source_kind),
        true,
    );
    metadata_item(
        html,
        "Artifact source id",
        artifact
            .source_id
            .as_deref()
            .map(safe_path_value)
            .as_deref()
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Artifact revision",
        artifact.revision.as_deref().unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Artifact sha256",
        artifact.sha256.as_deref().unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Artifact format",
        display_artifact_format(artifact.format),
        true,
    );
    let quant = artifact.quantization.as_ref();
    metadata_item(
        html,
        "Artifact quant",
        quant
            .map(|value| value.label.as_str())
            .unwrap_or("not declared"),
        true,
    );
    metadata_item(
        html,
        "Quant scheme",
        quant
            .and_then(|value| value.scheme.as_deref())
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Quant bits",
        &quant
            .and_then(|value| value.bits)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(html, "Server", display_server(&server.preset_name), false);
    metadata_item(
        html,
        "Server version",
        server
            .reported_version
            .as_deref()
            .unwrap_or("version not reported"),
        true,
    );
    metadata_item(
        html,
        "Decode class",
        decode_mode_id(result.decode_mode),
        true,
    );
    metadata_item(
        html,
        "Decode provenance",
        decode_mode_source_label(result.decode_mode_source),
        false,
    );
    metadata_item(
        html,
        "Recorded decode mode",
        decode_mode_id(server.decode_mode),
        true,
    );
    metadata_item(
        html,
        "Quirk flags",
        &list_or_not_recorded(&server.quirk_flags),
        true,
    );
    metadata_item(
        html,
        "Chat template id",
        server
            .chat_template
            .as_ref()
            .and_then(|template| template.id.as_deref())
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Chat template sha256",
        server
            .chat_template
            .as_ref()
            .and_then(|template| template.sha256.as_deref())
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Launch config sha256",
        server
            .launch_config_sha256
            .as_deref()
            .unwrap_or("not recorded"),
        true,
    );
    if let Some(corpus) = metadata.corpus.as_ref() {
        metadata_item(html, "Corpus id", &corpus.id, true);
        metadata_item(html, "Corpus revision", &corpus.revision, true);
        metadata_item(html, "Corpus sha256", &corpus.sha256, true);
        metadata_item(
            html,
            "Corpus scenarios",
            &corpus.scenario_count.to_string(),
            false,
        );
        metadata_item(html, "Scoring version", &corpus.scoring_version, true);
    } else {
        metadata_item(html, "Corpus", "not recorded", false);
    }
    metadata_item(
        html,
        "Host hardware",
        environment
            .map(|value| value.display_label.as_str())
            .unwrap_or("not recorded"),
        false,
    );
    metadata_item(
        html,
        "Host OS",
        &environment
            .map(environment_display_parts)
            .map(|(_, os)| os.to_owned())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Architecture",
        environment
            .and_then(|value| value.architecture.as_deref())
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Accelerator",
        environment
            .and_then(|value| value.accelerator.as_deref())
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Memory bytes",
        &environment
            .and_then(|value| value.memory_bytes)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Temperature",
        &metadata
            .sampling
            .temperature
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Top p",
        &metadata
            .sampling
            .top_p
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Seed",
        &metadata
            .sampling
            .seed
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Max tokens",
        &metadata
            .sampling
            .max_tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Replication",
        &replication_summary(result, dataset.replication_count(index)),
        false,
    );
    metadata_item(
        html,
        "Arm fingerprint",
        metadata
            .arm_fingerprint
            .as_deref()
            .unwrap_or("not recorded"),
        true,
    );
    metadata_item(
        html,
        "Preflight override",
        &metadata
            .preflight_override
            .as_ref()
            .map(|override_| {
                format!(
                    "forced={}; foreign endpoints={}",
                    override_.forced,
                    list_or_not_recorded(&override_.foreign_endpoints)
                )
            })
            .unwrap_or_else(|| "none".to_owned()),
        false,
    );
    metadata_item(
        html,
        "Ignored ports",
        &metadata
            .preflight_ignored_ports
            .as_ref()
            .map(|ports| {
                ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|ports| !ports.is_empty())
            .unwrap_or_else(|| "none".to_owned()),
        false,
    );
    html.push_str("        </dl>\n        <ol class=\"scenario-list\">\n");
    for scenario in &result.scenarios {
        render_scenario_detail(html, result.schema_version, scenario, repo_base);
    }
    html.push_str("        </ol>\n        <a class=\"back-link\" href=\"index.html#matrix-title\">Back to matrix</a>\n        </div>\n      </article>\n");
}

fn metadata_item(html: &mut String, label: &str, value: &str, code: bool) {
    if code {
        writeln!(
            html,
            "          <div><dt>{}</dt><dd><code>{}</code></dd></div>",
            escape_html(label),
            escape_html(value)
        )
        .expect("write metadata item");
    } else {
        writeln!(
            html,
            "          <div><dt>{}</dt><dd>{}</dd></div>",
            escape_html(label),
            escape_html(value)
        )
        .expect("write metadata item");
    }
}

fn render_scenario_detail(
    html: &mut String,
    schema_version: u32,
    scenario: &ScenarioView,
    repo_base: &str,
) {
    let status = display_status(scenario.status);
    write!(
        html,
        "          <li class=\"scenario status-{status}\"><div><code>{}</code><span class=\"status-label\">{status}</span></div><p>{}</p><p class=\"rationale\">Rationale: {}</p>",
        escape_html(&scenario.id),
        escape_html(&scenario.description),
        escape_html(&scenario.rationale),
    )
    .expect("write scenario detail");
    if let Some(failure) = scenario.failure_detail.as_ref() {
        write!(
            html,
            " <span class=\"failure-detail\">stage: {}; code: {}; HTTP: {}; failed turn: {}</span>",
            escape_html(&failure.stage),
            escape_html(&failure.code),
            failure
                .http_status
                .map(|value| value.to_string())
                .unwrap_or_else(|| "not recorded".to_owned()),
            failure
                .failed_turn_index
                .map(|value| value.to_string())
                .unwrap_or_else(|| "not recorded".to_owned()),
        )
        .expect("write scenario failure");
    }
    if let Some(reason) = scenario.failure_reason.as_deref() {
        write!(
            html,
            " <span class=\"failure-reason\">{}</span>",
            escape_html(reason)
        )
        .expect("write failure reason");
    }
    if let Some(evidence_path) = scenario.evidence_path.as_deref() {
        write!(
            html,
            " <a class=\"transcript\" href=\"{}\">transcript</a>",
            escape_html(&evidence_url(repo_base, evidence_path))
        )
        .expect("write transcript link");
    }
    if let Some(evidence_hash) = scenario.evidence_hash.as_deref() {
        write!(
            html,
            " <code class=\"evidence-hash\">{}</code>",
            escape_html(evidence_hash)
        )
        .expect("write evidence hash");
    }
    if scenario.retried {
        html.push_str(" <span class=\"annotation\">retried</span>");
    }
    render_annotation(html, schema_version, scenario, repo_base);
    html.push_str("</li>\n");
}

fn recorded(value: &str) -> &str {
    if value.is_empty() {
        "not recorded"
    } else {
        value
    }
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

fn list_or_not_recorded(values: &[String]) -> String {
    if values.is_empty() {
        "not recorded".to_owned()
    } else {
        values.join(", ")
    }
}

fn replication_summary(result: &StackRow, count: usize) -> String {
    result.metadata.replication.as_ref().map_or_else(
        || format!("n={count}, no declared study arm"),
        |replication| {
            format!(
                "n={count}; study={}; arm={}; run={}; mode={}",
                replication.study_id,
                replication.arm_id,
                replication.run_index,
                display_replication_mode(replication.mode)
            )
        },
    )
}

fn display_replication_mode(mode: ReplicationMode) -> &'static str {
    match mode {
        ReplicationMode::GreedyReproducibility => "greedy_reproducibility",
        ReplicationMode::SeedVariedVariance => "seed_varied_variance",
    }
}

fn display_artifact_source(source: ArtifactSourceKind) -> &'static str {
    match source {
        ArtifactSourceKind::Huggingface => "huggingface",
        ArtifactSourceKind::Ollama => "ollama",
        ArtifactSourceKind::LocalFile => "local_file",
        ArtifactSourceKind::Other => "other",
    }
}

fn display_artifact_format(format: ArtifactFormat) -> &'static str {
    match format {
        ArtifactFormat::Gguf => "gguf",
        ArtifactFormat::Mlx => "mlx",
        ArtifactFormat::Safetensors => "safetensors",
        ArtifactFormat::OllamaBlob => "ollama_blob",
        ArtifactFormat::Unknown => "unknown",
    }
}

fn render_colophon(dataset: &SiteDataset, main: &str) -> String {
    let mut revisions = dataset
        .rows
        .iter()
        .filter_map(|row| row.metadata.corpus.as_ref())
        .map(|corpus| format!("{} {}", corpus.id, corpus.revision))
        .collect::<BTreeSet<_>>();
    let revision = if revisions.is_empty() {
        "not recorded".to_owned()
    } else {
        revisions.pop_first().expect("non-empty revisions")
            + &revisions
                .into_iter()
                .map(|value| format!(", {value}"))
                .collect::<String>()
    };
    let date = dataset
        .rows
        .iter()
        .filter_map(|row| row.metadata.timestamp.get(..10))
        .max()
        .unwrap_or("not recorded");
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in main.bytes().chain(STYLE.bytes()).chain(SCRIPT.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!(
        "  <footer class=\"colophon\"><p>Build hash <code>{hash:016x}</code> - corpus revision <code>{}</code> - data date <time datetime=\"{}\">{}</time></p></footer>\n",
        escape_html(&revision),
        escape_html(date),
        escape_html(date),
    )
}

fn render_category_cell(
    html: &mut String,
    result: &StackRow,
    category: ScenarioCategory,
    repo_base: &str,
    replication_count: usize,
) {
    let scenarios = result
        .scenarios
        .iter()
        .filter(|scenario| scenario.category == category)
        .collect::<Vec<_>>();
    let counts = result.category_counts.for_category(category);
    let total = counts.total();
    let passed = counts.passed;
    let failed = counts.failed;
    let errors = counts.errors;
    let skipped = counts.skipped;
    let not_measurable = errors + skipped > 0 && passed + failed == 0;
    let class = if total == 0 {
        "untested"
    } else if not_measurable {
        "not-measurable"
    } else if passed == total {
        "all-pass"
    } else if passed == 0 {
        "none-pass"
    } else {
        "partial"
    };
    let first_evidence = scenarios
        .iter()
        .find(|scenario| {
            scenario.status != Status::Pass && scenario.evidence_path.as_deref().is_some()
        })
        .and_then(|scenario| scenario.evidence_path.as_deref());
    let replication_class = if replication_count < 5 {
        " low-replication"
    } else {
        ""
    };
    write!(
        html,
        "              <td class=\"score {class}{replication_class}\" data-label=\"{}\" aria-label=\"{}: {passed} passed, {failed} failed, {errors} errors, {skipped} skipped; n={replication_count}{}\">",
        category_label(category),
        category,
        if replication_count < 5 { "; no verdict" } else { "" },
    )
    .expect("write HTML");
    if let Some(evidence_path) = first_evidence {
        write!(
            html,
            "<a class=\"ratio\" href=\"{}\" title=\"Open a failing transcript\">{passed}/{total}</a>",
            escape_html(&evidence_url(repo_base, evidence_path))
        )
        .expect("write HTML");
    } else {
        write!(html, "<span class=\"ratio\">{passed}/{total}</span>").expect("write HTML");
        if total > 0 && passed < total && result.schema_version == 1 {
            html.push_str("<span class=\"legacy-evidence\">schema v1: no transcript path</span>");
        }
    }
    if not_measurable {
        html.push_str("<span class=\"measurement-state\">not measurable</span>");
    }
    html.push_str("</td>\n");
}

fn render_annotation(
    html: &mut String,
    schema_version: u32,
    scenario: &ScenarioView,
    repo_base: &str,
) {
    if let Some(cause) = scenario.cause.as_ref() {
        let label = match cause.kind {
            CauseKind::ServerDefect => "server defect",
            CauseKind::Unknown => "cause unknown",
        };
        if let Some(reference) = cause.reference.as_deref() {
            let reference = reference_url(repo_base, reference);
            write!(
                html,
                " <a class=\"annotation\" href=\"{}\">{label}</a>",
                escape_html(&reference),
            )
            .expect("write HTML");
        } else {
            write!(html, " <span class=\"annotation\">{label}</span>").expect("write HTML");
        }
    }
    if scenario.failure_class.as_deref() == Some("empty_response") {
        html.push_str(" <span class=\"annotation\">empty response</span>");
    } else if scenario.failure_class.as_deref() == Some("unparsed_tool_call") {
        html.push_str(" <span class=\"annotation\">unparsed tool call</span>");
    } else if schema_version == 1
        && scenario.cause.is_none()
        && scenario.status != Status::Pass
        && scenario.evidence_hash.is_some()
    {
        html.push_str(" <span class=\"annotation\">legacy evidence hash only</span>");
    }
}

fn display_identity_status(status: IdentityStatus) -> &'static str {
    match status {
        IdentityStatus::Verified => "verified",
        IdentityStatus::Declared => "declared",
        IdentityStatus::Unresolved => "unresolved",
    }
}

pub(super) fn render_submit(repo_base: &str) -> String {
    let contributing_url = format!("{repo_base}/blob/main/CONTRIBUTING.md");
    let main = format!(
        r#"    <section class="methods">
      <p class="eyebrow">Submission method</p>
      <h1>Create a result</h1>
      <p>Run one model at a time. Store its result file and evidence directory together.</p>
    </section>
    <section aria-labelledby="ollama-command">
      <h2 id="ollama-command">Ollama</h2>
      <pre><code>MODEL=qwen2.5:7b-instruct
OUT=results/ollama-qwen2.5-7b-instruct.json
cargo run -p willitcall -- run \
  --model "$MODEL" \
  --server ollama \
  --out "$OUT"
cargo run -p willitcall -- validate "$OUT"</code></pre>
    </section>
    <section aria-labelledby="llamacpp-command">
      <h2 id="llamacpp-command">llama.cpp</h2>
      <pre><code>MODEL_PATH=/absolute/path/to/model.Q4_K_M.gguf
OUT=results/llamacpp-model-q4_k_m.json
cargo run -p willitcall -- run \
  --model "$MODEL_PATH" \
  --server llamacpp \
  --out "$OUT"
cargo run -p willitcall -- validate "$OUT"</code></pre>
    </section>
    <section aria-labelledby="pr-checklist">
      <h2 id="pr-checklist">Pull request checklist</h2>
      <ul class="checklist">
        <li>Confirm that preflight is clean and has no contention override. Explain any override.</li>
        <li>Validate the result file against the schema.</li>
        <li>Include the evidence transcripts.</li>
        <li>Cross-check empty responses on a second server as required by the seeding protocol.</li>
      </ul>
      <p>Read <a href="{}">CONTRIBUTING.md</a> for the complete contribution rules.</p>
    </section>"#,
        escape_html(&contributing_url)
    );
    render_page_shell(PageShell {
        description: "Commands and checks for submitting a willitcall result.",
        title: "Submit a result - willitcall",
        current_page: Page::Submit,
        main_class: Some("submit-page"),
        main: &main,
        footer: "  <footer><p>Reviewers treat each result as measured stack behavior. They do not treat it as a model-only claim.</p></footer>\n",
        script: None,
    })
}

fn display_server(server: &str) -> &str {
    if server == "llamacpp" {
        "llama.cpp"
    } else if server == "mlx_lm" {
        "MLX LM"
    } else {
        server
    }
}

fn display_status(status: Status) -> &'static str {
    match status {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::Error => "error",
        Status::Skipped => "skipped",
    }
}

fn evidence_url(repo_base: &str, evidence_path: &str) -> String {
    format!(
        "{repo_base}/blob/main/results/{}",
        evidence_path.trim_start_matches('/')
    )
}

fn reference_url(repo_base: &str, reference: &str) -> String {
    if reference.starts_with("https://") || reference.starts_with("http://") {
        reference.to_owned()
    } else {
        format!(
            "{repo_base}/blob/main/{}",
            reference.trim_start_matches('/')
        )
    }
}

pub(super) fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character if character.is_ascii() => escaped.push(character),
            character => write!(escaped, "&#{};", character as u32).expect("write entity"),
        }
    }
    escaped
}

pub(super) const SCRIPT: &str = r#"const search = document.getElementById("model-search");
const groups = Array.from(document.querySelectorAll(".model-group"));
const status = document.getElementById("filter-status");

search.addEventListener("input", () => {
  const query = search.value.trim().toLocaleLowerCase();
  let shown = 0;
  for (const group of groups) {
    const visible = !query || group.dataset.modelSearch.includes(query);
    group.hidden = !visible;
    if (visible) shown += group.querySelectorAll(".result-row").length;
  }
  status.textContent = `Showing ${shown} ${shown === 1 ? "stack" : "stacks"}.`;
});
"#;

pub(super) const STYLE: &str = r#":root {
  color-scheme: light;
  /* Instrument-document foundation. These remain unused until the new figures land. */
  --design-paper: #fcfbf9;
  --design-ink: #1c1c1c;
  --design-grey-1: #4f4e4b;
  --design-grey-2: #8c8a85;
  --design-grey-3: #d5d2cc;
  --design-pass: #00513a; /* Okabe-Ito green #009e73, darkened toward L30. */
  --design-fail: #d55e00;
  --design-link-accent: #0f6d6d; /* Underlined links only. */
  --type-prose: 16px;
  --type-table: 13px;
  --type-raster-label: 11px;
  --type-heading-1: 20px;
  --type-heading-2: 17px;
  --type-heading-weight: 600;
  --measure-prose: 46rem;
  --measure-figure: 76rem;
  --ink: #17212b;
  --muted: #52606d;
  --line: #c9d2da;
  --paper: #f7f8f9;
  --panel: #ffffff;
  --accent: #135f69;
  --pass-bg: #d8efdf;
  --pass-ink: #17452a;
  --partial-bg: #fff0bf;
  --partial-ink: #594200;
  --none-bg: #f5d8dc;
  --none-ink: #681f29;
  --neutral-bg: #e8edf1;
  --neutral-ink: #35434f;
  font-family: "IBM Plex Sans", "Helvetica Neue", Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 1.55;
}

* { box-sizing: border-box; }

body {
  margin: 0;
  color: var(--ink);
  background: var(--paper);
}

a { color: #075f8a; text-underline-offset: 0.16em; }
a:hover { text-decoration-thickness: 2px; }
a:focus-visible, select:focus-visible, summary:focus-visible {
  outline: 3px solid #e5901a;
  outline-offset: 3px;
}

.site-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 2rem;
  padding: 1rem max(1.25rem, calc((100vw - 90rem) / 2));
  color: #ffffff;
  background: #15313a;
  border-bottom: 4px solid #4ca1a9;
}

.wordmark {
  color: #ffffff;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 1.1rem;
  font-weight: 800;
  text-decoration: none;
  letter-spacing: 0.04em;
}

nav { display: flex; flex-wrap: wrap; gap: 1.25rem; }
nav a { color: #dcebed; font-weight: 650; text-decoration: none; }
nav a[aria-current="page"] { color: #ffffff; text-decoration: underline; }

main, footer {
  width: min(90rem, calc(100% - 2.5rem));
  margin-inline: auto;
}

.methods {
  max-width: 75rem;
  padding: 4rem 0 2.5rem;
}

.methods p:not(.eyebrow) { max-width: 76ch; font-size: 1.06rem; }

.eyebrow {
  margin: 0 0 0.4rem;
  color: var(--accent);
  font-size: 0.78rem;
  font-weight: 800;
  letter-spacing: 0.12em;
  text-transform: uppercase;
}

h1, h2 { margin: 0 0 1rem; line-height: 1.12; }
h1 { font-size: clamp(2.2rem, 5vw, 4.4rem); letter-spacing: -0.045em; }
h2 { font-size: clamp(1.45rem, 2.5vw, 2.1rem); letter-spacing: -0.025em; }

.analysis { max-width: var(--measure-figure); margin-bottom: 4rem; }
.analysis > p { max-width: var(--measure-prose); }
.svg-figure { margin: 2.5rem 0 3.5rem; color: var(--design-ink); }
.svg-figure figcaption { max-width: var(--measure-prose); margin-bottom: 1rem; }
.svg-figure figcaption > span { display: block; margin-top: 0.45rem; }
.svg-figure figcaption > strong { font-size: var(--type-heading-2); font-weight: var(--type-heading-weight); }
.does-not-show { color: var(--design-grey-1); }
.figure-scroll { overflow-x: auto; padding: 0.5rem; background: var(--design-paper); border: 1px solid var(--design-grey-3); }
.svg-figure svg { display: block; max-width: none; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.raster-row-label, .signature-stack-label,
.plot-panel-label, .not-measurable-label, .strip-tick-label { fill: var(--design-ink); }
.raster-row-label, .signature-stack-label, .raster-column-label,
.not-measurable-label { font-size: var(--type-raster-label); }
.raster-column-label { fill: var(--design-grey-1); }
.raster-group-label, .plot-panel-label { fill: var(--design-ink); font-size: 12px; font-weight: 700; }
.strip-tick-label { font-size: 11px; }

.matrix {
  margin-bottom: 4rem;
  padding: 1.5rem;
  background: var(--panel);
  border: 1px solid var(--line);
  box-shadow: 0 10px 30px rgb(23 33 43 / 8%);
}

.matrix-heading {
  display: flex;
  align-items: end;
  justify-content: space-between;
  gap: 2rem;
}

label { color: var(--muted); font-size: 0.84rem; font-weight: 750; }
select {
  display: block;
  min-width: 11rem;
  margin-top: 0.35rem;
  padding: 0.65rem 2.25rem 0.65rem 0.75rem;
  color: var(--ink);
  background: #ffffff;
  border: 1px solid #81909c;
  border-radius: 0.2rem;
  font: inherit;
}

.legend { display: flex; flex-wrap: wrap; gap: 1.25rem; margin: 1.25rem 0 0; color: var(--muted); font-size: 0.82rem; }
.legend span { display: inline-flex; align-items: center; gap: 0.4rem; }
.swatch { width: 0.85rem; height: 0.85rem; border: 1px solid rgb(23 33 43 / 25%); }
.swatch.all-pass, .score.all-pass { color: var(--pass-ink); background: var(--pass-bg); }
.swatch.partial, .score.partial { color: var(--partial-ink); background: var(--partial-bg); }
.swatch.none-pass, .score.none-pass { color: var(--none-ink); background: var(--none-bg); }
.score.untested { color: var(--neutral-ink); background: var(--neutral-bg); }
/* Not measurable is neutral, never red: the combination produced no measurement,
   which is not the same claim as failing. The hatch keeps it distinct from the
   other states without relying on colour. */
.swatch.not-measurable, .score.not-measurable {
  color: var(--neutral-ink);
  background: repeating-linear-gradient(
    45deg, var(--neutral-bg), var(--neutral-bg) 3px,
    rgb(23 33 43 / 12%) 3px, rgb(23 33 43 / 12%) 6px);
}
.measurement-state { display: block; margin-top: 0.35rem; font-size: 0.68rem; line-height: 1.25; }

.filter-status { margin: 0.7rem 0 1rem; color: var(--muted); font-size: 0.86rem; }
.table-scroll { overflow-x: auto; border: 1px solid var(--line); }
table { width: 100%; min-width: 70rem; border-collapse: collapse; }
th, td { padding: 0.85rem; text-align: left; border: 1px solid var(--line); }
thead th { color: #ffffff; background: #284852; font-size: 0.78rem; }
thead code { color: inherit; }
.result-row > th { width: 18rem; background: #f1f4f6; }
.result-row > th strong, .result-row > th span { display: block; }
.result-row > th strong { margin-bottom: 0.35rem; font-size: 0.98rem; }
.result-row > th span { color: var(--muted); font-size: 0.78rem; font-weight: 500; }
.score { min-width: 8rem; text-align: center; }
.ratio { display: block; color: inherit; font: 800 1.05rem/1.2 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.legacy-evidence { display: block; margin-top: 0.35rem; font-size: 0.68rem; line-height: 1.25; }
.detail-row > td { padding: 0; background: #fbfcfc; }
.detail-row details { padding: 0.8rem 1rem; }
.detail-row summary { width: fit-content; color: #075f8a; cursor: pointer; font-weight: 700; }

.metadata {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
  gap: 0.75rem;
  margin: 1rem 0;
}
.metadata div { min-width: 0; padding: 0.7rem; background: #eef2f4; }
.metadata dt { color: var(--muted); font-size: 0.7rem; font-weight: 800; text-transform: uppercase; }
.metadata dd { margin: 0.2rem 0 0; overflow-wrap: anywhere; }
.scenario-list { margin: 1rem 0 0; padding-left: 1.75rem; }
.scenario { padding: 0.5rem 0 0.5rem 0.25rem; border-bottom: 1px solid #e0e5e9; }
.scenario:last-child { border-bottom: 0; }
.status-label { margin-left: 0.4rem; font-size: 0.72rem; font-weight: 850; text-transform: uppercase; }
.status-pass .status-label { color: #236d3d; }
.status-fail .status-label, .status-error .status-label { color: #9a2535; }
.failure-reason { display: inline; color: var(--muted); }
.failure-reason::before { content: "- "; }
.transcript { margin-left: 0.55rem; font-size: 0.85rem; }
.annotation { margin-left: 0.45rem; font-size: 0.72rem; }

.submit-page { max-width: 58rem; }
.submit-page section { margin-bottom: 2.5rem; }
pre { overflow-x: auto; padding: 1.25rem; color: #eef7f8; background: #18343d; border-left: 4px solid #4ca1a9; }
code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.checklist { padding-left: 1.3rem; }
.checklist li { margin-bottom: 0.65rem; }

footer { padding: 1.5rem 0 3rem; color: var(--muted); border-top: 1px solid var(--line); font-size: 0.86rem; }

@media (prefers-color-scheme: dark) {
  :root {
    --design-paper: #131417;
    --design-ink: #e8e6e1;
    --design-grey-1: #c8c5bf;
    --design-grey-2: #8d8c89;
    --design-grey-3: #4b4c50;
    --design-pass: #39b990;
    --design-fail: #f1843d;
    --design-link-accent: #69b9b9;
  }
}

@media (max-width: 44rem) {
  .site-header, .matrix-heading { align-items: flex-start; flex-direction: column; gap: 1rem; }
  .site-header { padding-inline: 1.25rem; }
  main, footer { width: min(100% - 1.5rem, 90rem); }
  .methods { padding-top: 2.5rem; }
  .matrix { padding: 1rem; }
  nav { gap: 1rem; }
}

@media print {
  body { background: #ffffff; }
  .site-header { color: #000000; background: #ffffff; border-color: #000000; }
  .wordmark, nav a { color: #000000; }
  .matrix { box-shadow: none; }
  label, .filter-status { display: none; }
  .table-scroll { overflow: visible; }
  table { min-width: 0; font-size: 9pt; }
  a { color: inherit; }
}

/* Brief 9: calibration-sheet presentation. Colour is reserved for data marks. */
:root {
  color-scheme: light;
  --paper: #f8f7f3;
  --panel: #f8f7f3;
  --ink: var(--design-ink);
  --muted: var(--design-grey-1);
  --line: var(--design-grey-3);
  --neutral-bg: #ebe9e3;
  --neutral-ink: var(--design-ink);
  --pass-bg: #d8e7df;
  --pass-ink: #073e2e;
  --partial-bg: #eee7d1;
  --partial-ink: #403b28;
  --none-bg: #efdcd3;
  --none-ink: #552b1b;
  font-family: "IBM Plex Sans", "Helvetica Neue", Helvetica, Arial, sans-serif;
  font-size: var(--type-prose);
}

body { background: var(--paper); color: var(--ink); }
a { color: inherit; text-decoration-thickness: 1px; }
a:focus-visible, input:focus-visible, summary:focus-visible {
  outline: 2px solid var(--paper);
  outline-offset: 2px;
  box-shadow: 0 0 0 4px var(--ink);
}
code, .wordmark, .ratio, input { font-family: "IBM Plex Mono", "SFMono-Regular", Consolas, monospace; }

.site-header {
  width: min(var(--measure-figure), calc(100% - 2.5rem));
  margin-inline: auto;
  padding: 0.8rem 0;
  color: var(--ink);
  background: transparent;
  border-bottom: 1px solid var(--ink);
}
.wordmark, nav a, nav a[aria-current="page"] { color: var(--ink); }
.wordmark { font-size: 0.92rem; font-weight: 600; letter-spacing: 0.08em; }
nav a { font-size: 0.82rem; font-weight: 500; }

main, footer { width: 100%; }
.register-intro, .methods, .submit-page, .submit-page section {
  width: min(var(--measure-prose), calc(100% - 2.5rem));
  margin-inline: auto;
}
.register-intro { padding: 3.25rem 0 1.5rem; }
.register-intro h1 { max-width: 38rem; }
.framing { max-width: var(--measure-prose); margin: 0; font-size: 1.05rem; }
.eyebrow { color: var(--ink); font-size: 0.7rem; font-weight: 600; }
h1 { font-size: clamp(2rem, 5vw, 3.65rem); font-weight: 500; }
h2 { font-size: var(--type-heading-1); font-weight: var(--type-heading-weight); letter-spacing: 0; }
h3 { font-size: var(--type-heading-2); font-weight: var(--type-heading-weight); }

.reading-key { margin-top: 0.7rem; padding: 0.6rem 0; border-block: 1px solid var(--line); }
.reading-caption { margin: 0 0 0.5rem; font-size: 0.75rem; }
.mark-key, .method-key { display: flex; flex-wrap: wrap; gap: 0.55rem 1.15rem; }
.method-key { margin-top: 0.5rem; padding-top: 0.5rem; border-top: 1px dotted var(--line); }
.mark-key span, .method-key > span { display: inline-flex; align-items: center; gap: 0.4rem; font-size: 0.72rem; }
.state-key, .mini-mark { position: relative; display: inline-block; width: 0.78rem; height: 0.78rem; flex: none; }
.state-key.state-pass, .mini-mark.pass { background: var(--design-pass); }
.state-key.state-fail, .mini-mark.fail { background: var(--design-fail); }
.state-key.state-fail::after, .mini-mark.fail::after,
.state-key.state-error::after, .mini-mark.error::after {
  content: ""; position: absolute; inset: 48% -1px auto; border-top: 1.5px solid var(--design-paper); transform: rotate(45deg);
}
.state-key.state-error, .mini-mark.error { border: 1px solid var(--design-ink); }
.state-key.state-error::after, .mini-mark.error::after { border-color: var(--design-ink); border-width: 1px; }
.state-key.state-skipped, .mini-mark.skipped { border: 1px dotted var(--design-grey-2); opacity: 0.6; }
.hatch-key { width: 1.15rem; height: 0.78rem; border: 1px dashed var(--ink); background: repeating-linear-gradient(135deg, transparent 0 3px, rgb(28 28 28 / 18%) 3px 4px); }
.decode-badge {
  display: inline-block;
  padding: 0.13rem 0.38rem;
  border: 1px solid var(--ink);
  border-radius: 0;
  color: var(--ink);
  background: transparent;
  font: 500 0.67rem/1.2 "IBM Plex Mono", "SFMono-Regular", Consolas, monospace;
}
.decode-badge.grammar_constrained { color: var(--paper); background: var(--ink); }
.decode-badge.unknown { border-style: dashed; }
.identity-status { border-left: 2px solid var(--ink); }
i.identity-status { display: inline-block; width: 0.55rem; height: 0.7rem; }
.identity-status.unresolved { border-left-style: dashed; }

.matrix, .analysis, .appendix {
  width: min(var(--measure-figure), calc(100% - 2.5rem));
  max-width: none;
  margin: 0 auto 3.5rem;
}
.matrix { padding: 1rem 0 0; background: transparent; border: 0; border-top: 2px solid var(--ink); box-shadow: none; }
.matrix-heading { align-items: start; }
.matrix-heading h2 { margin-bottom: 0; }
label { color: var(--ink); font-size: 0.72rem; font-weight: 600; }
input[type="search"] {
  display: block;
  width: min(20rem, 70vw);
  margin-top: 0.3rem;
  padding: 0.5rem 0.6rem;
  color: var(--ink);
  background: var(--paper);
  border: 1px solid var(--ink);
  border-radius: 0;
  font-size: 0.82rem;
}
.matrix-note, .filter-status { max-width: var(--measure-prose); color: var(--muted); font-size: 0.75rem; }
.matrix-note { margin: 0.55rem 0 0; }
.filter-status { margin: 0.2rem 0 0.45rem; }
.table-scroll { border: 1px solid var(--ink); }
.matrix-table { min-width: 68rem; font-size: var(--type-table); }
.matrix-table th, .matrix-table td { padding: 0.44rem 0.5rem; vertical-align: middle; border-color: var(--line); }
.matrix-table thead th { color: var(--paper); background: var(--ink); font-size: 0.7rem; font-weight: 500; }
.matrix-table thead th:first-child, .result-row > th { position: sticky; left: 0; z-index: 2; }
.matrix-table thead th:first-child { z-index: 4; }
.model-heading th { padding: 0.55rem 0.5rem 0.3rem; color: var(--ink); background: var(--paper); border-top: 2px solid var(--ink); border-bottom: 0; font: 500 0.9rem/1.3 "IBM Plex Mono", "SFMono-Regular", Consolas, monospace; }
.model-heading th > span { margin-right: 0.6rem; color: var(--muted); font: 500 0.62rem/1 "IBM Plex Sans", sans-serif; letter-spacing: 0.08em; text-transform: uppercase; }
.decode-band th { padding: 0.35rem 0.6rem; color: var(--muted); background: var(--neutral-bg); border-block: 1px solid var(--ink); font-size: 0.68rem; font-weight: 400; }
.decode-band .decode-badge { margin-right: 0.7rem; }
.result-row > th { width: 22rem; background: var(--paper); }
.result-row > th strong { margin: 0; overflow: hidden; font: 500 0.79rem/1.3 "IBM Plex Mono", "SFMono-Regular", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }
.row-meta { display: flex; align-items: center; gap: 0.45rem; margin-top: 0.18rem; white-space: nowrap; }
.result-row > th .row-meta > span { display: inline-block; color: var(--muted); font-size: 0.63rem; font-weight: 400; }
.result-row > th .decode-badge { display: inline-block; margin: 0; color: var(--ink); font-size: 0.61rem; }
.result-row > th .decode-badge.grammar_constrained { color: var(--paper); }
.result-row > th .identity-status { margin: 0; padding-left: 0.3rem; }
.detail-link { display: inline-block; margin: 0; font-size: 0.63rem; font-weight: 500; }
.score { position: relative; min-width: 7.5rem; text-align: center; }
.score.low-replication {
  background-image: repeating-linear-gradient(135deg, transparent 0 5px, rgb(28 28 28 / 12%) 5px 6px);
  background-blend-mode: multiply;
}
.ratio { font-size: 0.9rem; font-weight: 500; }
.measurement-state, .legacy-evidence { display: block; margin-top: 0.2rem; font-size: 0.58rem; line-height: 1.2; }

.analysis { border-top: 2px solid var(--ink); padding-top: 1rem; }
.analysis-primary { margin-top: -1.5rem; }
.analysis > p { max-width: var(--measure-prose); }
.svg-figure { margin: 1.5rem 0 3.25rem; }
.svg-figure figcaption { max-width: var(--measure-prose); }
.figure-scroll { padding: 0.5rem; background: var(--design-paper); border-color: var(--ink); }
.figure-links { font-size: 0.75rem; }
.desktop-raster { display: block; }
.mobile-raster { display: none; }

.methods { max-width: var(--measure-prose); padding: 1rem 0 3.5rem; border-top: 2px solid var(--ink); }
.methods p:not(.eyebrow) { max-width: var(--measure-prose); font-size: 1rem; }
.appendix { padding-top: 1rem; border-top: 2px solid var(--ink); }
.appendix > p { max-width: var(--measure-prose); }
.stack-detail { margin: 2rem 0 3rem; padding-top: 0.8rem; border-top: 1px solid var(--ink); }
.stack-detail header { display: flex; align-items: baseline; flex-wrap: wrap; gap: 0.5rem 0.8rem; }
.stack-detail header h3 { margin: 0; }
.stack-detail header h3 a { text-underline-offset: 0.15em; }
.stack-content { display: none; }
.stack-detail:target { scroll-margin-top: 1rem; border-top-width: 2px; }
.stack-detail:target .stack-content { display: block; }
.detail-index { margin: 0; color: var(--muted); font: 500 0.65rem/1 "IBM Plex Mono", monospace; text-transform: uppercase; }
.metadata { grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr)); gap: 1px; border: 1px solid var(--line); background: var(--line); }
.metadata div { padding: 0.55rem; background: var(--paper); }
.metadata dt { color: var(--muted); font-size: 0.6rem; font-weight: 600; }
.metadata dd { font-size: 0.74rem; }
.scenario-list { padding-left: 1.5rem; }
.scenario { border-color: var(--line); }
.scenario > p { margin: 0.25rem 0; max-width: var(--measure-prose); font-size: 0.78rem; }
.scenario .rationale { color: var(--muted); }
.failure-detail, .evidence-hash { display: inline-block; margin: 0.3rem 0 0 0.45rem; color: var(--muted); font-size: 0.68rem; }
.evidence-hash { max-width: 100%; overflow-wrap: anywhere; word-break: break-all; }
.back-link { font-size: 0.75rem; }

.colophon {
  width: min(var(--measure-figure), calc(100% - 2.5rem));
  margin-inline: auto;
  padding: 0.7rem 0;
  color: var(--muted);
  border-top: 1px solid var(--ink);
  font-size: 0.66rem;
}
.colophon p { margin: 0; }

pre { color: var(--paper); background: var(--ink); border-left: 0; }

@media (prefers-color-scheme: dark) {
  :root {
    color-scheme: dark;
    --paper: var(--design-paper);
    --panel: var(--design-paper);
    --ink: var(--design-ink);
    --muted: var(--design-grey-1);
    --line: var(--design-grey-3);
    --neutral-bg: #24262a;
    --neutral-ink: var(--design-ink);
    --pass-bg: #18372e;
    --pass-ink: #c9f0df;
    --partial-bg: #393426;
    --partial-ink: #eee3bd;
    --none-bg: #422a23;
    --none-ink: #f4d2c5;
  }
  .score.low-replication { background-blend-mode: screen; }
  .hatch-key { background-image: repeating-linear-gradient(135deg, transparent 0 3px, rgb(232 230 225 / 22%) 3px 4px); }
}

@media (max-width: 46rem) {
  .site-header, .matrix-heading { align-items: flex-start; flex-direction: column; gap: 0.8rem; }
  .site-header, .register-intro, .methods, .matrix, .analysis, .appendix, .colophon {
    width: min(100% - 1.5rem, var(--measure-figure));
  }
  .register-intro { padding-top: 2.25rem; }
  .mark-key, .method-key { display: grid; grid-template-columns: 1fr 1fr; }
  input[type="search"] { width: min(100%, 22rem); }
  .table-scroll { overflow: visible; border: 0; }
  .matrix-table, .matrix-table tbody, .matrix-table tr { display: block; min-width: 0; width: 100%; }
  .matrix-table thead { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  .model-group { margin-bottom: 1rem; border: 1px solid var(--ink); }
  .model-group.single-row { margin-bottom: 0; border: 0; }
  .model-heading th, .decode-band th { display: block; width: 100%; border-inline: 0; }
  .result-row { display: grid !important; grid-template-columns: repeat(3, minmax(0, 1fr)); border-bottom: 1px solid var(--ink); }
  .result-row:last-child { border-bottom: 0; }
  .result-row > th { position: static; grid-column: 1 / -1; width: auto; border: 0; border-bottom: 1px solid var(--line); }
  .row-meta { gap: 0.3rem; }
  .result-row > th .row-meta > span, .row-meta .detail-link { font-size: 0.58rem; }
  .result-row > td { display: block; min-width: 0; padding: 0.5rem 0.25rem; border-width: 0 1px 1px 0; }
  .score::before { content: attr(data-label); display: block; min-height: 2.2em; margin-bottom: 0.25rem; color: currentColor; font-size: 0.57rem; line-height: 1.1; }
  .ratio { font-size: 0.78rem; }
  .analysis-primary .figure-scroll { display: none; }
  .mobile-raster { display: block; }
  .mini-raster-panel { margin: 1.25rem 0; overflow-x: auto; }
  .mini-raster-panel h3 { position: sticky; left: 0; margin-bottom: 0.4rem; font-size: 0.78rem; }
  .mini-raster-grid { display: grid; align-items: center; gap: 2px; width: max-content; font: 0.58rem/1.1 "IBM Plex Mono", monospace; }
  .mini-raster-corner, .mini-row-label { position: sticky; left: 0; z-index: 2; width: 8.5rem; padding-right: 0.35rem; overflow: hidden; background: var(--paper); text-align: right; text-overflow: ellipsis; white-space: nowrap; }
  .mini-column-label { width: 0.78rem; overflow: hidden; writing-mode: vertical-rl; transform: rotate(180deg); white-space: nowrap; }
}

@media print {
  :root { color-scheme: light; --paper: #fff; --ink: #000; --muted: #333; --line: #aaa; }
  .site-header { color: #000; background: #fff; border-color: #000; }
  .wordmark, nav a { color: #000; }
  label, .filter-status, .detail-link, .back-link { display: none; }
  .table-scroll { overflow: visible; }
  .matrix-table { min-width: 0; font-size: 7pt; }
  .matrix-table thead th:first-child, .result-row > th { position: static; }
  .analysis-primary .figure-scroll { display: block; }
  .mobile-raster { display: none; }
  .stack-content { display: block; }
  a { color: inherit; }
}
"#;
