# M7: the measurement contract

Status: PLANNED, 2026-08-05.

M7 does three things, in dependency order:

1. Stops the site from publishing claims the data does not support (Tier 0).
2. Replaces `model_id` with a real measurement contract, schema v3 (Tier 1).
3. Adds analysis views that the contract makes defensible (Tier 2).

The ordering is not negotiable: Tier 2 views that group across servers are
invalid until Tier 1 lands, and Tier 1 is pointless if Tier 0's falsehoods are
still on the page.

## Why this milestone exists

Two independent reviews and a direct data audit converged on one finding:

**`metadata.model_id` is a provenance string, not an identity.** All 32
published rows carry distinct values. Ollama writes a tag (`qwen3:8b`),
llama.cpp writes an HF GGUF ref or a raw local blob path, mlx-lm writes an HF
repo id. `phi4-mini` is measured on all three servers and nothing in the schema
connects those three rows.

The cross-server comparison this project exists to make cannot be expressed in
its own schema. Every grouped view, every family rollup, and every quantization
chart is downstream of fixing that.

Three further facts established during the audit:

- **No published row clears amendment 4.** 32 distinct arms, none replicated.
  The n=5 data backing the case studies lives off-repo on macstudio. Amendment 7
  already records that the original n=5 runs measured harness determinism rather
  than model variance.
- **`declared_quant` is null for all 14 Ollama rows**, so the quant axis is
  unqueryable for the largest server bucket.
- **`multi_turn` passes 37/224 (17%), with 22 of 32 rows scoring exactly zero.**
  The site names this nowhere. The evidence supports stating it.

## Tier 0: truthfulness fixes

These ship independently and block nothing.

### T0.1 Errors are not failures

`render_category_cell` (`crates/willitcall/src/site.rs:342`) reduces a cell to
`passed/total`, so a row that produced only errors renders identically to a row
that genuinely failed. gemma3-4b and gemma3-12b return HTTP 400 for any request
carrying `tools`: 50 errors, zero measurements. Both render in the same red as
granite3.1-dense, with the same `aria-label="0 passed out of 13"`.

A screen reader user cannot distinguish "this combination failed" from "this
combination could not be measured". That is the exact class of overclaim this
project exists to prevent, present in its own flagship artifact.

Required: a distinct `not-measurable` cell state when
`errors > 0 && passed + failed == 0`, an accessible label carrying all four
counts (pass/fail/error/skipped), a legend entry, and a non-colour encoding so
the distinction survives greyscale and colour blindness.

### T0.2 Absolute local paths are published

Three result files carry `/Users/bbadmin/...` in `metadata.model_id`
(`llamacpp-granite3.1-dense-8b`, `llamacpp-phi4-mini`,
`llamacpp-watt-tool-8b-q4_k_m`). The same strings appear in **300 evidence
transcripts across 6 run directories**.

Two complications that make this more than a find-and-replace:

- Transcript bytes are sha256-hashed into `evidence_hash`. Redacting them
  requires rewriting the transcripts and recomputing the hashes, as an explicit,
  documented migration. Hiding the field in HTML leaves the paths in Git.
- `crates/willitcall/tests/site_cli.rs:236` asserts
  `index.contains("/models/blobs/sha256-deadbeef")`. A test currently *enforces*
  rendering `model_id` raw into HTML. That assertion must be replaced, not
  deleted silently.

### T0.3 Site generation is not gated on PRs

`.github/workflows/pages.yml` runs `willitcall site` only on `push` and
`workflow_dispatch`. A change that breaks site generation lands on main and is
discovered after deploy. Add a `pull_request` trigger that builds the site
without deploying.

## Tier 1: schema v3, the measurement contract

An optional `model_family` field added to v2 is rejected as the fix. Producers
could omit it, consumers would stay branchy, there would be no enforceable
identity guarantee, and it would enable attractive but invalid aggregation
without the controls that make aggregation sound. This is a version bump.

`read_result` (`crates/wic-core/src/result.rs:194`) currently deserializes every
version into one struct before inspecting `schema_version`. v3 inspects the
version first, deserializes `RunResultV1V2` or `RunResultV3`, then normalizes
into an internal `Measurement`. One struct must not accumulate a forest of
optional v3 fields.

### T1.1 Model and artifact identity

`metadata.model` replaces the overloaded `model_id`:

| Field | Type | Meaning |
|---|---|---|
| `display_name` | String | Human-facing label. From the registry, never derived from a filename. |
| `family_id` | String or null | Lineage slug: `qwen2.5`, `phi-4`, `llama-3.1`. |
| `canonical_id` | String or null | Upstream checkpoint, e.g. `Qwen/Qwen2.5-7B-Instruct`. **This is the cross-server key.** |
| `parameter_count_b` | number or null | Nullable: `8b` in a name is not an exact tensor count. |
| `endpoint_id` | String | The selector sent to the endpoint. Basename or alias for local files, never an absolute path. Never an identity key. |
| `identity_status` | enum | `verified` / `declared` / `unresolved`. A visible epistemic state, not an optionality accident. |

`metadata.model.artifact`:

| Field | Type | Meaning |
|---|---|---|
| `source_kind` | enum | `huggingface` / `ollama` / `local_file` / `other` |
| `source_id` | String or null | HF repo, Ollama tag, or safe artifact name |
| `revision` | String or null | Immutable HF commit or Ollama manifest digest |
| `sha256` | String or null | Blob digest. Required by publication policy for local files. |
| `format` | enum | `gguf` / `mlx` / `safetensors` / `ollama_blob` / `unknown` |
| `quantization` | object or null | `{label, scheme, bits}`. `Q4_K_M` and MLX `4bit` are not interchangeable because both mention four bits. |

An MLX conversion is not the same artifact as a GGUF of the same checkpoint,
consistent with the README's existing warning. `canonical_id` links them;
`artifact` keeps them distinct.

A checked-in model registry is the source of these mappings, but resolved values
are embedded into every result so each file stays self-contained.

### T1.2 The rest of the contract

Model identity alone is not sufficient to identify a comparable arm.

| Field | Why |
|---|---|
| `metadata.corpus.{id, revision, sha256, scenario_count}` | `--scenarios` accepts arbitrary definitions (`main.rs:416`). Matching scenario ids do not prove matching prompts or schemas. |
| `metadata.corpus.scoring_version` | Scorer behaviour changes without scenario ids changing. Historical totals must not silently acquire new semantics. |
| `metadata.server.decode_mode` | Explicit `grammar_constrained` / `unconstrained_post_hoc` / `unknown`, replacing inference from `quirk_flags` where absence ambiguously means unverified. |
| `metadata.server.chat_template.{id, sha256}` | The README names chat template as a stack dimension; results do not record it at all. |
| `metadata.server.launch_config_sha256` | Same server version differs by `--jinja`, context size, cache quantization, parser flags. |
| `metadata.replication` | `{study_id, arm_id, run_index, mode}` where mode is `greedy_reproducibility` or `seed_varied_variance`. Makes amendments 4 and 7 enforceable rather than prose. |
| `metadata.arm_fingerprint` | Versioned hash over corpus, artifact, server build/config, template, decode mode, environment, and non-seed sampling. Makes arm grouping auditable. |
| `metadata.environment` structured | `os_name`, `os_version`, `architecture`, `accelerator`, `memory_bytes`, retaining a display label. |
| `scenarios[].failure` | `{stage, code, http_status, failed_turn_index}`. `"turn 1: server returned HTTP 400"` is not queryable. Keep `failure_reason` for humans. |
| Unique run id | `runner.rs:566` hashes timestamp, endpoint, and model selector, so two identical runs in the same second collide. Use UUIDv7 or a random nonce. |

### T1.3 Migration of the 32 published files

A deterministic migration command plus an explicit 32-entry mapping manifest.
**Identity is never inferred from filenames** - the site's current
filename-derived label (`site.rs:504`) is precisely the behaviour being retired.

- Ollama rows: recover manifest digest and real quantization from saved
  manifests or macstudio. Otherwise mark `declared` or `unresolved`.
- HF GGUF and MLX rows: map repo to canonical checkpoint, recover an immutable
  revision. A bare repo id is not an immutable artifact.
- Blob-path rows: the blob name already carries a sha256, but blob-to-canonical
  mapping must be corroborated. Granite has such evidence in its case study.
- watt-tool has no digest in the result. Recover the file or mark `unresolved`.
- Migrate every file to v3 even when unresolved. Exclude unresolved artifacts
  from cross-model joins.
- v1/v2 remain accepted as legacy inputs; new runs emit v3.

## Tier 2: scoring and views

### T2.1 Scoring semantics

The per-scenario primitive (`pass|fail|error|skipped`) is sound and stays. A
scenario is an atomic compatibility contract; multi-turn stays all-or-nothing.

`passed/50` stays as a backwards-compatible corpus count. It is **not** a model
score and **not** a default sort key. The six category denominators are
13/8/8/7/7/7, so a flat total silently weights `single_call` heaviest.

Derived for presentation:

- Per-category `{passed, failed, errors, skipped}`.
- `measurement_coverage = (passed + failed) / total`.
- `macro_category_pass_rate = mean(pass_c / (pass_c + fail_c))`, computed only
  when every category has zero errors and skips. Otherwise **null, not zero**.

The six-value profile is more honest than any scalar.

**Negative-trap polarity is not a defect.** "Pass" consistently means "satisfied
the scenario contract", whether the contract requires a call or abstention
(`score.rs:132`). Two real issues:

1. `negative_trap` is heterogeneous: it mixes abstention with long/unicode
   argument transport. Do not split the historical category. Add scenario
   facets (`abstention`, `argument_fidelity`, `unicode`, `long_context`).
2. The scorer would count an empty response as a successful abstention, and
   `crates/wic-core/tests/empty_response.rs:76`
   (`empty_response_preserves_negative_trap_pass`) locks that in.

   **Audited 2026-08-05: this is latent, not live.** All 166 negative-trap
   passes across all 32 published rows are backed by genuine textual replies;
   zero are empty. No published number changes. Fix it as a guard against future
   contamination, not as a data correction.

   Add a per-turn `response_requirement` to scenario TOML
   (`tool_calls` / `text_without_tool_calls` / `no_tool_calls` / `either`).
   Abstention scenarios use `text_without_tool_calls`, so a null reply becomes a
   failed `empty_response` rather than a green cell.

### T2.2 Defensible views

Refused outright, with the reasoning recorded so the request can be declined
consistently when it is reopened:

- **A ranked leaderboard or sortable score column.** The most-used feature
  within a week, and every screenshot circulates stripped of every caveat. It is
  a one-way door: once a public ordering exists, a retraction reads as "the
  rankings changed" rather than "the method improved". This project's
  retractions are its moat.
- Server-average bars or boxplots across Ollama/llama.cpp/mlx: model selection
  is unbalanced and decode mode is systematically confounded.
- Family-by-server heatmaps before T1.1 lands.
- Quantization-versus-score plots: K-quants and MLX conversions are not one
  scalar treatment.
- Confidence intervals treating 50 fixed scenarios as IID trials, or any
  interval over arms with n=1.
- Radar charts: they imply comparable continuous axes and hide denominators.
- Any plot placing gemma3's error rows at score zero.

Shipped instead:

1. **Scenario-status raster.** Rows are observed stacks, columns are all 50
   scenario ids grouped by category, fill is four distinct patterns for
   pass/fail/error/skipped. Tooltips carry the scenario `description` and
   `rationale`. Rows ordered by stable identity, never by total. This is the
   primary view.
2. **Multi-turn raster.** The seven multi-turn scenarios across all stacks,
   later annotated with `failed_turn_index`. Shows the cliff directly and
   separates universal first-turn failure from chaining failure.
3. **Outcome-signature inventory.** Keyed on the exact 50-status vector, valued
   by the stacks sharing it. Better than a histogram for the 7/50 cluster
   because it shows whether identical totals came from identical scenarios.
   Granite's seven passes are exactly the negative category.
4. **Per-capability aggregate across stacks.** Six bars: "passed in X of Y
   measured stacks". A count of measurements, not a ranking. This is what
   surfaces the multi-turn cliff. It is **not** filtered to n>=5 arms: with zero
   replication in `results/`, such a filter renders empty.
5. **Observed pass-count strip plot**, one dot per fully measurable stack,
   error-bearing rows in a separate "not fully measurable" panel, labelled
   "distribution of published one-run stack observations".

Every SVG carries `<title>`, `<desc>`, a table fallback, four-state non-colour
encoding, and escaped labels. No chart library.

### T2.3 Information architecture

The six paragraphs of caveats are not an accessibility problem to be deleted.
They are a design failure to be compiled into the page's geometry.

- Group rows by model; nest quant and server beneath.
- Stratify into visible `grammar_constrained` and `unconstrained_post_hoc`
  bands, each badged. The valid comparison becomes what adjacency shows; the
  invalid one requires crossing a labelled boundary.
- Human column labels, with the snake_case id in the tooltip.
- Model search box.
- Move caveat prose below the matrix; keep one sentence above it.
- Cells surface n, denominator, failure class, and transcript link at point of
  use. Cells with n<5 get a hatched treatment and an "n=1, no verdict" note:
  amendment 4 rendered as pixels rather than a paragraph.

**Audience note.** The target is not a non-technical reader; nobody
non-technical runs `llama-server --jinja`. It is a developer with a specific
question who currently cannot get an answer because rows are sorted by filename
behind a wall of prose. The goal is answer latency, not simplification.

## Architecture prerequisite

String concatenation is not the problem; `write!` generates correct HTML and
accessible SVG without a template dependency. The problem is that loading,
normalization, aggregation, prose, and rendering are fused:

- `generate` (`site.rs:25`) knows both data loading and every output asset.
- `read_results` (`site.rs:48`) has no catalog concept.
- `render_index` (`site.rs:92`) computes statistics while writing one document.
- Display identity comes from filenames (`site.rs:224`).
- `colspan="7"` is hardcoded (`site.rs:273`).
- Case-study run counts are hardcoded prose (`site.rs:138`), with tests
  preserving the literals (`site_cli.rs:249`).

Before a second view lands, introduce `SiteDataset` (normalized v1/v2/v3 plus
catalog), `StackRow` (safe display identity, structured stack dimensions),
`CategoryCounts` (four statuses, coverage, optional macro), `ScenarioView`, and
`StudyView` (only arms passing the replication gate). Split into `site/data.rs`,
`site/html.rs`, `site/svg.rs`.

The catalog join must fail loudly on duplicate ids, missing definitions, or
category mismatches, and a result's corpus hash must match the supplied catalog
before descriptions are attached. For custom corpora `site` accepts an explicit
catalog directory or displays "catalog unavailable"; it must never silently join
against the embedded corpus.

## Execution order

| # | Item | Depends on | Acceptance |
|---|---|---|---|
| 1 | T0.1 errors are not failures | - | All-error cells render a distinct non-colour-coded `not measurable` state; accessible label carries all four counts; regression test covers gemma3-shaped input |
| 2 | T0.3 CI gate | - | `pages.yml` builds the site on `pull_request` without deploying; a deliberately broken generator fails the check |
| 3 | T0.2 path redaction | - | No absolute `/Users/...` path in any result, transcript, or generated HTML; `evidence_hash` recomputed and the rehash documented; `site_cli.rs:236` assertion replaced |
| 4 | T1.1 + T1.2 schema v3 | 3 | v3 emitted by new runs; versioned parsing accepts v1/v2/v3; local paths sanitized before serialization; validator green |
| 5 | T1.3 registry + migration | 4 | All 32 files validate as v3 with explicit registry entries; no filename-derived identity; unresolved rows visibly marked and excluded from joins |
| 6 | Site data layer | 5 | `SiteDataset` performs strict catalog joins; four-state category counts derived; existing page renders unchanged from it |
| 7 | T2.1 scoring guard | 6 | `response_requirement` in scenario TOML; empty abstention no longer passes; `scoring_version` prevents silent cross-version comparison; no published total changes (verified against the 2026-08-05 audit) |
| 8 | T2.2 views | 6 | Raster, multi-turn raster, signature inventory, capability aggregate, strip plot all ship; no default score sort; every SVG has text fallback |
| 9 | T2.3 IA | 8 | Model grouping, decode-class bands, human labels, search, relocated prose |

Items 1-3 are independent of each other and of everything else. 4 through 9 are
a strict chain.
