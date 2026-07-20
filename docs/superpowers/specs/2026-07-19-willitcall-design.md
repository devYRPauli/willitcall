# willitcall - design spec

Date: 2026-07-19
Status: approved direction (A-then-B), v1 in design
Owner: Yash Raj Pandey (devYRPauli)

## One-liner

The caniuse.com of local-model tool calling: a local conformance suite plus a
public red/green matrix answering "will this model, at this quant, on this
server, actually execute tool calls?"

## Problem

Tool calling is the load-bearing capability of the agent era, and on local
stacks it silently breaks: quantization degrades call formatting, inference
servers parse tool-call output differently, chat templates carry subtle bugs
(wrong role tokens, dropped tool-call IDs, broken parallel-call syntax), and
harnesses each work around a different subset. Users discover breakage only
as mysterious agent failures. Evidence: 4-5 fragment projects (<15 stars
each) exist for single slices of this; BFCL benchmarks fp16 API models and
does not cover the quant x server x template axis at all. Nobody publishes
the combined matrix.

## Users

1. Local-LLM users choosing a model+quant+server combo for agent work
   (r/LocalLLaMA, llama.cpp/Ollama/LM Studio users).
2. Model finetuners and quantizers validating releases before publishing.
3. Inference-server and harness developers triaging "tool calls broken with
   model X" issues (link the matrix cell instead of re-debugging).

## Product shape (two components)

### 1. `willitcall` CLI (Rust, single static binary)

- Runs a declarative scenario corpus (~50 cases) against ANY
  OpenAI-compatible endpoint (`/v1/chat/completions`): llama.cpp server,
  Ollama, LM Studio, vLLM, or a remote API.
- Scenario categories:
  - single tool call (simple/nested/edge-case JSON schemas)
  - parallel tool calls
  - streaming tool calls (delta reassembly correctness)
  - tool-choice modes (`auto`, `required`, `none`, named function)
  - multi-turn: tool result fed back, follow-up call
  - negative traps: no-tool-needed prompts (false-positive calls),
    invalid-schema temptations, unicode/long-argument stress
- Deterministic scoring per scenario: did a call happen, was JSON valid, did
  it match the schema, were args semantically right (exact/set match against
  expected, no LLM judging in v1), stream reassembly byte-correct.
- Output: versioned JSON result file (schema-versioned) capturing model id,
  quant, server+version, sampling params, scenario pass/fail + raw evidence
  hashes. Human TTY report + `--json` mode.
- Repro-first: result files embed everything needed to re-run.

### 2. The matrix site (static, GitHub Pages)

- caniuse-style red/green grid: rows = model+quant, columns = scenario
  groups, filterable by server. Cells link to result-file evidence.
- Fed by PRs adding result files to `results/` in the repo; CI validates
  schema + recomputes derived tables; site rebuilds on merge.
- Seeded by the owner: ~15 popular models x 2-3 GGUF quants on llama.cpp
  server + Ollama (16GB MacBook: strictly ONE model loaded at a time,
  memory watchdog, per the owner's OOM history).

### Month 2 differentiator: template forensics (approach B)

- Read chat template from GGUF metadata; static lint against a curated
  knowledge base of tool-calling breakage patterns (the nanochat
  tool-token mismatch class, dropped tool-call IDs, parallel-call syntax
  bugs).
- Correlate lint hits with failing scenarios so red cells become diagnoses
  ("fails parallel calls: template drops call ID"), not trivia.

### Explicitly deferred (v2+)

- Harness-in-the-loop testing (grok-build/Codex CLI against endpoints):
  churn trap per research; revisit after the matrix is alive.
- Template auto-repair (emit fixed templates): builds on forensics KB.
- LLM-judged semantic scoring.

## Architecture (CLI)

- `crates/willitcall` (bin) - CLI (clap), TTY report, orchestration.
- `crates/wic-core` (lib) - scenario model, runner, scoring, result schema.
  Scenarios are DATA (embedded TOML/JSON files, also loadable from disk) so
  non-Rust contributors can add cases without touching code.
- HTTP via `reqwest` (streaming: eventsource/SSE handling done manually for
  fidelity); serde for schemas; no async runtime beyond tokio basics.
- Server adapters are thin config presets (base URL, quirks flags), not
  code forks; quirk flags recorded in results.
- Result schema versioned from day one (`schema_version` field).

## Testing

- Unit: scoring functions, stream reassembly, schema validation (golden
  files of real server responses, including malformed ones).
- Integration: a mock OpenAI-compatible server (in-repo) replaying recorded
  fixtures so CI needs no model; plus one live smoke test target
  (llama.cpp server + a tiny model) run locally, not in CI.
- Acceptance for M1: `willitcall run --endpoint http://localhost:8080/v1
  --model <x>` completes the corpus against llama.cpp server and Ollama and
  emits a valid result file; fixtures prove at least one known-broken combo
  is correctly flagged red.

## Error handling

- Endpoint down / auth fail: clear preflight check, exit code distinct from
  scenario failures.
- Timeouts per scenario with retry-once policy; hung streams cut and scored
  as failures with evidence retained.
- Result files never written partially (write temp + atomic rename).

## Success criteria

- Month 1: CLI runs full corpus against llama.cpp server + Ollama locally;
  seed results for >= 5 models committed.
- Month 2: matrix site live with >= 15 models x 2-3 quants; launch post
  ("we tested N combos; here is what actually breaks").
- Month 3 (the flywheel test): result-file PRs from >= 3 strangers. If zero,
  pivot site to owner-maintained scorecard with scheduled re-runs.

## Non-goals

- Not a general LLM benchmark; only tool calling.
- Not an eval framework or observability product.
- No hosted backend, no accounts, no telemetry.

## Amendments (2026-07-19, post-M3, decided by owner)

1. Evidence (implemented in M3, schema_version=2): every result file ships
   full per-scenario request/response transcripts (auth-redacted, SHA-256
   integrity hash OF the transcript) under evidence/. Red cells must be
   defensible; hash-only evidence is not acceptable.
2. Empty-response scoring rule (to implement in M4): a response with no
   content AND no tool call must never silently score as model behavior.
   The cell stays red (truthful for the combo) but carries a cause
   annotation (e.g. cause: server-defect + case-study link) when isolation
   proves the server at fault. Seeding protocol: any empty response is
   cross-checked on a second server before the result is committed.
   Rationale: M3 proved Ollama 0.32.1 discards well-formed tool calls its
   own engine emits (same weights pass 6/6 via llama.cpp, empty 10/10 via
   Ollama) - red tells users the truth, the annotation tells model authors
   we are not blaming their weights.
3. Contention preflight (to implement in M4): `run` refuses (or requires
   --force) when another known inference server is responding on common
   ports. Rationale: an overlapping server produced plausible-but-wrong
   error counts during seeding; contributors have the same failure mode.
4. Forensics methodology bar: no verdict from n=1; replicate (>= 5 runs per
   arm) before recording a case-study conclusion. M2's single-sample
   "description change fixes it" conclusion was overturned by M3.

## Naming / availability (verified 2026-07-19)

- GitHub: no existing "willitcall" repos. npm 404, PyPI 404, crates.io "does
  not exist", willitcall.dev NXDOMAIN. Prior candidate "ToolProof" burned
  (Pagefind/toolproof + npm + toolproof.dev, and Moshe-ship/toolproof is
  same-niche).
