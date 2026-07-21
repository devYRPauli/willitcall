use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use wic_core::result::{
    parse_and_validate_result, CauseKind, EnvironmentMetadata, RunResult, Status,
};
use wic_core::ScenarioCategory;

const CATEGORIES: [ScenarioCategory; 6] = [
    ScenarioCategory::SingleCall,
    ScenarioCategory::ParallelCalls,
    ScenarioCategory::Streaming,
    ScenarioCategory::ToolChoiceModes,
    ScenarioCategory::MultiTurn,
    ScenarioCategory::NegativeTrap,
];

struct ResultFile {
    file_name: String,
    result: RunResult,
}

pub(crate) fn generate(
    results_directory: &Path,
    output_directory: &Path,
    repo_base: &str,
) -> Result<usize, String> {
    let results = read_results(results_directory)?;
    let repo_base = repo_base.trim_end_matches('/');
    let index = render_index(&results, repo_base);
    let submit = render_submit(repo_base);

    fs::create_dir_all(output_directory).map_err(|error| {
        format!(
            "failed to create site directory {}: {error}",
            output_directory.display()
        )
    })?;
    write_site_file(output_directory.join("index.html"), &index)?;
    write_site_file(output_directory.join("submit.html"), &submit)?;
    write_site_file(output_directory.join("style.css"), STYLE)?;
    write_site_file(output_directory.join("site.js"), SCRIPT)?;
    Ok(results.len())
}

fn read_results(directory: &Path) -> Result<Vec<ResultFile>, String> {
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
        .collect::<Result<Vec<PathBuf>, _>>()?;
    paths.retain(|path| {
        path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "json")
    });
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path)
                .map_err(|error| format!("failed to read result {}: {error}", path.display()))?;
            let result = parse_and_validate_result(&bytes)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let file_name = path
                .file_name()
                .ok_or_else(|| format!("result path {} has no file name", path.display()))?
                .to_string_lossy()
                .into_owned();
            Ok(ResultFile { file_name, result })
        })
        .collect()
}

fn write_site_file(path: PathBuf, contents: &str) -> Result<(), String> {
    fs::write(&path, contents)
        .map_err(|error| format!("failed to write site file {}: {error}", path.display()))
}

fn render_index(results: &[ResultFile], repo_base: &str) -> String {
    let scenario_count = results
        .iter()
        .flat_map(|result| result.result.scenarios.iter())
        .map(|scenario| scenario.id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let case_studies_url = format!("{repo_base}/tree/main/docs/case-studies");
    let peg_native_case_study_url = format!(
        "{repo_base}/blob/main/docs/case-studies/2026-07-21-llamacpp-500s-on-llama-3.1-tool-calls.md"
    );
    let uniform_environment = results
        .first()
        .and_then(|result| result.result.metadata.environment.as_ref())
        .filter(|environment| {
            results
                .iter()
                .all(|result| result.result.metadata.environment.as_ref() == Some(*environment))
        });
    let environment_statement = uniform_environment
        .map(render_environment_statement)
        .unwrap_or_default();
    let mut html = String::new();
    write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="description" content="Measured tool-calling support by model, quant, server, and server version.">
  <title>willitcall support matrix</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <header class="site-header">
    <a class="wordmark" href="index.html">willitcall</a>
    <nav aria-label="Primary navigation">
      <a aria-current="page" href="index.html">Matrix</a>
      <a href="submit.html">Submit a result</a>
    </nav>
  </header>
  <main>
    <section class="methods" aria-labelledby="page-title">
      <p class="eyebrow">Measured compatibility</p>
      <h1 id="page-title">Tool-calling support matrix</h1>
      <p>A cell measures the whole stack: model x quant x server x server version. It is not a property of the model alone.</p>
      <p>Red means the combination failed as tested, not that the weights are bad. The same weights can pass on one server and fail on another; where that is proven, the cell carries a cause annotation.</p>
      <p>Every red cell links to the full request/response transcript that produced it when the result schema supplies a transcript path. Legacy schema v1 results do not record transcript paths. See the <a href="{}">case studies under docs/case-studies/</a> for controlled comparisons.</p>
      <p>The servers do not decode the same way. llama.cpp compiles the supplied tool definitions into a GBNF grammar and constrains decoding with it, so a call naming a function that was never supplied cannot be sampled there. Ollama and MLX LM generate unconstrained text and parse the tool call out of it afterwards. This systematically favours llama.cpp, so a llama.cpp-versus-Ollama difference is a property of the combination, not evidence of a server defect or a difference between models; the comparison that isolates the model is same-server.</p>
      <p>Sample size and method: {} distinct scenarios are represented in this result set. Each published cell is one run. Findings in the case studies are replicated across at least five runs per arm before a verdict is drawn, so a cell tells you what one run measured and a case study tells you what held up under repetition. The current case studies cover 90 runs across 18 quantization arms, and 40 runs across 8 arms for the peg-native anomaly.</p>
      <h2>Excluded rows</h2>
      <ul>
        <li>Meta-Llama-3.1-8B-Instruct on llama.cpp (Q8_0, Q4_K_M, Q3_K_M) is excluded from the quantization conclusion because llama.cpp returns HTTP 500 on 7-9 of 50 scenarios per run for this model ("does not match the expected peg-native format"). These are server errors, not model failures, and are not comparable across arms. See the <a href="{}">peg-native case study</a>.</li>
      </ul>
{}
    </section>

    <section class="matrix" aria-labelledby="matrix-title">
      <div class="matrix-heading">
        <div>
          <p class="eyebrow">Current results</p>
          <h2 id="matrix-title">Scenario groups</h2>
        </div>
        <label for="server-filter">Server
          <select id="server-filter">
            <option value="all">All</option>
            <option value="ollama">Ollama</option>
            <option value="llamacpp">llama.cpp</option>
            <option value="mlx_lm">MLX LM</option>
          </select>
        </label>
      </div>
      <div class="legend" aria-label="Cell status legend">
        <span><i class="swatch all-pass"></i>all pass</span>
        <span><i class="swatch partial"></i>partial</span>
        <span><i class="swatch none-pass"></i>none pass</span>
      </div>
      <p id="filter-status" class="filter-status" aria-live="polite">Showing {} result files.</p>
      <div class="table-scroll">
        <table>
          <thead>
            <tr>
              <th scope="col">Model / quant / server</th>
"#,
        escape_html(&case_studies_url),
        scenario_count,
        escape_html(&peg_native_case_study_url),
        environment_statement,
        results.len()
    )
    .expect("write HTML");
    for category in CATEGORIES {
        writeln!(
            html,
            "              <th scope=\"col\"><code>{}</code></th>",
            category
        )
        .expect("write HTML");
    }
    html.push_str(
        r#"            </tr>
          </thead>
"#,
    );

    for (index, result_file) in results.iter().enumerate() {
        render_result_rows(
            &mut html,
            index,
            result_file,
            repo_base,
            uniform_environment.is_none(),
        );
    }

    html.push_str(
        r#"        </table>
      </div>
    </section>
  </main>
  <footer>
    <p>Read ratios as passed scenarios / total scenarios in the category.</p>
  </footer>
  <script src="site.js"></script>
</body>
</html>
"#,
    );
    html
}

fn render_result_rows(
    html: &mut String,
    index: usize,
    result_file: &ResultFile,
    repo_base: &str,
    disclose_environment: bool,
) {
    let result = &result_file.result;
    let server = &result.metadata.server.preset_name;
    let server_display = display_server(server);
    let model = model_label(&result_file.file_name, server);
    let quant = result
        .metadata
        .declared_quant
        .as_deref()
        .unwrap_or("not declared");
    let details_id = format!("result-details-{index}");
    let environment_metadata = if disclose_environment {
        let environment = result.metadata.environment.as_ref();
        format!(
            "                    <div><dt>Host hardware</dt><dd>{}</dd></div>\n                    <div><dt>Host OS</dt><dd>{}</dd></div>\n",
            escape_html(
                environment
                    .map(|environment| environment.host_hardware_class.as_str())
                    .unwrap_or("not recorded")
            ),
            escape_html(
                environment
                    .map(|environment| environment.host_os.as_str())
                    .unwrap_or("not recorded")
            )
        )
    } else {
        String::new()
    };
    write!(
        html,
        "          <tbody class=\"result-group\" data-server=\"{}\">\n            <tr class=\"result-row\">\n              <th scope=\"row\">\n                <strong>{}</strong>\n                <span>quant: {}</span>\n                <span>server: {}</span>\n              </th>\n",
        escape_html(server),
        escape_html(&model),
        escape_html(quant),
        escape_html(server_display)
    )
    .expect("write HTML");

    for category in CATEGORIES {
        render_category_cell(html, result_file, category, repo_base);
    }

    write!(
        html,
        "            </tr>\n            <tr class=\"detail-row\">\n              <td colspan=\"7\">\n                <details id=\"{details_id}\">\n                  <summary>View {} scenarios and row metadata</summary>\n                  <dl class=\"metadata\">\n                    <div><dt>Result file</dt><dd><code>{}</code></dd></div>\n                    <div><dt>Model id</dt><dd><code>{}</code></dd></div>\n                    <div><dt>Declared quant</dt><dd>{}</dd></div>\n                    <div><dt>Server</dt><dd>{} {}</dd></div>\n                    <div><dt>Schema</dt><dd>v{}</dd></div>\n                    <div><dt>Run time</dt><dd>{}</dd></div>\n{}                  </dl>\n                  <ol class=\"scenario-list\">\n",
        result.scenarios.len(),
        escape_html(&result_file.file_name),
        escape_html(&result.metadata.model_id),
        escape_html(quant),
        escape_html(server_display),
        escape_html(
            result
                .metadata
                .server
                .reported_version
                .as_deref()
                .unwrap_or("version not reported")
        ),
        result.schema_version,
        escape_html(&result.metadata.timestamp),
        environment_metadata
    )
    .expect("write HTML");

    for scenario in &result.scenarios {
        let status = display_status(scenario.status);
        write!(
            html,
            "                    <li class=\"scenario status-{status}\"><code>{}</code> <span class=\"status-label\">{status}</span>",
            escape_html(&scenario.id)
        )
        .expect("write HTML");
        if let Some(reason) = scenario.failure_reason.as_deref() {
            write!(
                html,
                " <span class=\"failure-reason\">{}</span>",
                escape_html(reason)
            )
            .expect("write HTML");
        }
        if let Some(evidence_path) = scenario.evidence_path.as_deref() {
            let url = evidence_url(repo_base, evidence_path);
            write!(
                html,
                " <a class=\"transcript\" href=\"{}\">transcript</a>",
                escape_html(&url)
            )
            .expect("write HTML");
        }
        render_annotation(html, result.schema_version, scenario, repo_base);
        html.push_str("</li>\n");
    }

    html.push_str(
        r#"                  </ol>
                </details>
              </td>
            </tr>
          </tbody>
"#,
    );
}

fn render_environment_statement(environment: &EnvironmentMetadata) -> String {
    format!(
        "      <p>Measurement environment: {}; {}.</p>",
        escape_html(&environment.host_hardware_class),
        escape_html(&environment.host_os)
    )
}

fn render_category_cell(
    html: &mut String,
    result_file: &ResultFile,
    category: ScenarioCategory,
    repo_base: &str,
) {
    let scenarios = result_file
        .result
        .scenarios
        .iter()
        .filter(|scenario| scenario.category == category)
        .collect::<Vec<_>>();
    let total = scenarios.len();
    let passed = scenarios
        .iter()
        .filter(|scenario| scenario.status == Status::Pass)
        .count();
    let class = if total == 0 {
        "untested"
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
    write!(
        html,
        "              <td class=\"score {class}\" aria-label=\"{}: {passed} passed out of {total}\">",
        category
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
        if total > 0 && passed < total && result_file.result.schema_version == 1 {
            html.push_str("<span class=\"legacy-evidence\">schema v1: no transcript path</span>");
        }
    }
    html.push_str("</td>\n");
}

fn render_annotation(
    html: &mut String,
    schema_version: u32,
    scenario: &wic_core::result::ScenarioOutcome,
    repo_base: &str,
) {
    if let Some(cause) = scenario.cause.as_ref() {
        let label = match cause.kind {
            CauseKind::ServerDefect => "server defect",
            CauseKind::Unknown => "cause unknown",
        };
        let title = cause.note.as_deref().unwrap_or(label);
        if let Some(reference) = cause.reference.as_deref() {
            let reference = reference_url(repo_base, reference);
            write!(
                html,
                " <a class=\"badge cause\" href=\"{}\" title=\"{}\">{label}</a>",
                escape_html(&reference),
                escape_html(title)
            )
            .expect("write HTML");
        } else {
            write!(
                html,
                " <span class=\"badge cause\" title=\"{}\">{label}</span>",
                escape_html(title)
            )
            .expect("write HTML");
        }
    }
    if scenario.failure_class.as_deref() == Some("empty_response") {
        html.push_str(" <span class=\"badge neutral\">empty response</span>");
    } else if scenario.failure_class.as_deref() == Some("unparsed_tool_call") {
        html.push_str(" <span class=\"badge neutral unparsed\">unparsed tool call</span>");
    } else if schema_version == 1
        && scenario.cause.is_none()
        && scenario.status != Status::Pass
        && scenario.evidence_hash.is_some()
    {
        html.push_str(" <span class=\"badge neutral\">legacy evidence hash only</span>");
    }
}

fn render_submit(repo_base: &str) -> String {
    let contributing_url = format!("{repo_base}/blob/main/CONTRIBUTING.md");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="description" content="Commands and checks for submitting a willitcall result.">
  <title>Submit a result - willitcall</title>
  <link rel="stylesheet" href="style.css">
</head>
<body>
  <header class="site-header">
    <a class="wordmark" href="index.html">willitcall</a>
    <nav aria-label="Primary navigation">
      <a href="index.html">Matrix</a>
      <a aria-current="page" href="submit.html">Submit a result</a>
    </nav>
  </header>
  <main class="submit-page">
    <section class="methods">
      <p class="eyebrow">Submission method</p>
      <h1>Produce a new result cell</h1>
      <p>Run one model at a time. Keep the result file and the evidence directory written beside it.</p>
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
        <li>preflight clean (no contention override), or the override is explained</li>
        <li>result file schema-valid</li>
        <li>evidence transcripts included</li>
        <li>empty responses cross-checked on a second server per the seeding protocol</li>
      </ul>
      <p>Read <a href="{}">CONTRIBUTING.md</a> for the complete contribution rules.</p>
    </section>
  </main>
  <footer><p>Results are reviewed as measured stack behavior, not model-only claims.</p></footer>
</body>
</html>
"#,
        escape_html(&contributing_url)
    )
}

fn model_label(file_name: &str, server: &str) -> String {
    let stem = file_name.strip_suffix(".json").unwrap_or(file_name);
    stem.strip_prefix(&format!("{server}-"))
        .unwrap_or(stem)
        .to_owned()
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

fn escape_html(value: &str) -> String {
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

const SCRIPT: &str = r#"const filter = document.getElementById("server-filter");
const groups = Array.from(document.querySelectorAll(".result-group"));
const status = document.getElementById("filter-status");

filter.addEventListener("change", () => {
  let shown = 0;
  for (const group of groups) {
    const visible = filter.value === "all" || group.dataset.server === filter.value;
    group.hidden = !visible;
    if (visible) shown += 1;
  }
  status.textContent = `Showing ${shown} result ${shown === 1 ? "file" : "files"}.`;
});
"#;

const STYLE: &str = r#":root {
  color-scheme: light;
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
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
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

nav { display: flex; gap: 1.25rem; }
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
.badge { display: inline-block; margin-left: 0.45rem; padding: 0.12rem 0.42rem; border-radius: 999px; font-size: 0.7rem; font-weight: 800; text-decoration: none; }
.badge.cause { color: #632014; background: #ffe0d4; border: 1px solid #e6a18d; }
.badge.neutral { color: var(--neutral-ink); background: var(--neutral-bg); border: 1px solid #bcc7cf; }
.badge.neutral.unparsed { color: #4a3410; background: #fdf0d5; border: 1px solid #d9b877; }

.submit-page { max-width: 58rem; }
.submit-page section { margin-bottom: 2.5rem; }
pre { overflow-x: auto; padding: 1.25rem; color: #eef7f8; background: #18343d; border-left: 4px solid #4ca1a9; }
code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
.checklist { padding-left: 1.3rem; }
.checklist li { margin-bottom: 0.65rem; }

footer { padding: 1.5rem 0 3rem; color: var(--muted); border-top: 1px solid var(--line); font-size: 0.86rem; }

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
"#;
