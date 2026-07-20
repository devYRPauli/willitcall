# willitcall

A caniuse-style compatibility matrix for tool calling on local models.

Every local inference stack claims OpenAI-compatible function calling. In
practice support varies by model, by quantization, by chat template, and by
server. `willitcall` is a small CLI that runs a fixed corpus of 50 tool-calling
scenarios against any OpenAI-compatible endpoint and emits a machine-readable
result file, so "does this model actually do parallel tool calls on llama.cpp"
becomes a fact you can look up instead of an afternoon you lose.

Status: the CLI and corpus work. The public matrix site is not up yet.

## Quickstart

You need Rust (stable) and a running OpenAI-compatible server.

With Ollama:

```
ollama serve
ollama pull qwen2.5:7b-instruct

cargo run -p willitcall -- run \
  --endpoint http://localhost:11434/v1 \
  --model qwen2.5:7b-instruct \
  --server ollama \
  --out willitcall-result.json
```

With llama.cpp:

```
llama-server -m /path/to/model.gguf --port 8080 --jinja

cargo run -p willitcall -- run \
  --endpoint http://localhost:8080/v1 \
  --model /path/to/model.gguf \
  --server llamacpp \
  --out willitcall-result.json
```

The run prints a per-scenario report and exits 0 if every scenario passed, 1 if
any scenario failed, and 3 if preflight failed (no result file is written in
that case). Add `--json` for pipe-clean output.

Check a result file against the published schema:

```
cargo run -p willitcall -- validate willitcall-result.json
```

`--server` selects a preset (`llamacpp`, `ollama`, `lmstudio`, `vllm`,
`custom`). The preset only supplies request defaults; the preset name is
recorded in the result file so results stay comparable.

## What the scenarios test

The corpus is 50 scenarios in six categories. Every scenario is plain TOML data
in `crates/wic-core/scenarios/`; point `--scenarios <dir>` at your own directory
to run a modified set.

- `single` - one tool call with one argument shape: strings, integers, decimals,
  booleans, enums, arrays, nested objects, empty arguments, optional arguments
  present and omitted.
- `parallel` - several tool calls emitted in a single response.
- `streaming` - the same calls again over SSE, reassembled from deltas, to catch
  servers that only break under streaming.
- `multi_turn` - a tool result is fed back and a follow-up call must use a value
  that only exists in that result.
- `tool_choice` - `auto`, `none`, `required`, and a named function.
- `negative` - cases where the correct behavior is to NOT call a tool, plus
  awkward argument content (a 256-character token, a non-ASCII city name).

Scoring is deterministic. There is no LLM judge, because failure reasons get
published and a published reason has to be defensible.

Read a red cell carefully: a red in `parallel` means the model did not emit
several tool calls in one response, which is the capability that column
measures. It does not mean the model is broken.

## The scenario-authoring rule

**A scenario that a fully correct model could fail is a bug in the scenario.**

A false red is worse for this project than a missing test. If the matrix says a
model fails and it does not, nobody trusts any other cell. So an expectation
must admit *every* correct answer, not just the one the author had in mind.

Concretely, when writing or reviewing a scenario:

- Prefer structural checks over string equality. Assert that the call happened,
  that the arguments validate against the schema, that the argument set matches.
- If a text value must match exactly, the prompt has to force it. Put the exact
  literal in the user message, or use an opaque identifier (`doc-17`, `EXT-55`)
  rather than something a model may legitimately expand or spell differently.
- Where the exact text does not matter, set `arguments_match = "ignore"` on that
  expected call, or `"subset"` when extra arguments are acceptable.
- Arrays are compared positionally, so if order is not part of the requirement,
  say "in that order" in the prompt or do not use an array.
- Do not pin an operand position for a commutative operation.
- Do not expect a value the model has no way to know.

Every scenario carries a `rationale` field stating why its expectation is
complete: what the scenario asserts, and why no correct model can fail it for
another reason. Pull requests that add a scenario without a rationale will be
asked for one.

Two real bugs of this class were found and fixed during development: a scenario
expected the ASCII spelling of an Icelandic city name when a correct model
writes it with a diacritic, and a scenario expected `place = "Fenway Park"` when
a correct model geocodes `"Fenway Park, Boston, MA"`.

## Submitting a result

The matrix is fed by result files in `results/`, each shipping the transcripts
that back it. Seed results so far cover `qwen3` at 0.6b/1.7b/4b/8b,
`llama3.1:8b` and `qwen2.5:7b-instruct` on Ollama, plus
`Qwen2.5-1.5B-Instruct-Q4_K_M` on llama.cpp, all from a 16GB M-series MacBook.

1. Run the full corpus against your endpoint, one model loaded at a time.
   Running two models at once produces spurious `error` outcomes from resource
   contention, not real measurements.
2. Run `willitcall validate` on the output. Current results are schema
   version 2 (`schemas/result-v2.schema.json`); version 1 files are still
   accepted.
3. Open a pull request adding the result file **and its `evidence/` directory**
   under `results/`, and say what hardware and server version produced it.

Every scenario writes a full request/response transcript to
`evidence/<run_id>/<scenario-id>.json`, referenced from the result file, so any
red cell can be inspected rather than taken on trust. Credential-bearing
headers and URL query parameters are redacted at capture time, but read a
transcript before publishing it if your endpoint is not local.

Do not hand-edit a result file or a transcript. `evidence_hash` is the SHA-256
of the transcript bytes, so edits are detectable and edited results are not
comparable with anything else in the matrix.

## Roadmap

- The static matrix site (GitHub Pages), fed by `results/` and rebuilt on merge.
- Seed results across popular models and quantizations on llama.cpp and Ollama.
- Template forensics: read the chat template out of GGUF metadata and lint it
  against known tool-calling breakage patterns, so a red cell can say "fails
  parallel calls: template drops the call id" instead of just "fails".

Deferred: harness-in-the-loop testing against real agent CLIs, automatic
template repair, and LLM-judged semantic scoring.

## Development

```
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT. See LICENSE.
