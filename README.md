# willitcall

A caniuse-style compatibility matrix for tool calling on local models.

Every local inference stack claims OpenAI-compatible function calling. In
practice, support varies by model, quantization, chat template, and server.
`willitcall` is a small CLI. It runs a fixed corpus of 50 tool-calling
scenarios against any OpenAI-compatible endpoint. It writes a machine-readable
result file. You can then look up whether a model does parallel tool calls on
llama.cpp, instead of testing it yourself.

Status: the CLI and corpus work. The public matrix is live at
https://devyrpauli.github.io/willitcall/.

The write-up is at https://yashrajpandey.com/writing/same-weights-opposite-results/.
The replication runs behind the case studies are in
`docs/case-studies/evidence/replication/`.

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

The run prints a per-scenario report. Add `--json` for pipe-clean output. The
exit code is `0` if every scenario passed and `1` if a scenario failed (a wrong
model answer). It is `2` for a usage or scenario configuration error. It is `3`
if preflight failed, and the run writes no result file. It is `4` for a harness
error, or if a scenario ended in `error` (a failed request).

Check a result file against the published schema:

```
cargo run -p willitcall -- validate willitcall-result.json
```

New runs use result schema v3 (`schemas/result-v3.schema.json`). The CLI
resolves the model selector through `registry/models-v1.json` and embeds the
resolved model and artifact metadata in the result. Legacy schema v1 and v2
files are still valid input to `validate`, `site`, `annotate`, and `rescore`.
Editing a legacy file keeps its original schema version.

`--server` selects a preset (`llamacpp`, `ollama`, `mlx-lm`, `lmstudio`,
`vllm`, `custom`). The preset only supplies request defaults. The result file
records the preset name, so results stay comparable.

The `mlx-lm` preset defaults to port 8081, not mlx-lm's own default of 8080.
This project uses 8080 for llama.cpp. Two servers on one port cause the
contention that the preflight exists to catch.

Two facts matter when you read or add an mlx-lm row:

- **MLX rows use converted weights.** MLX does not read GGUF. An MLX row and the
  llama.cpp row for the same model do not use the same bits. This project holds
  weights constant by serving one blob through two servers. That fails across
  this boundary. An MLX-versus-GGUF difference includes the conversion.
- **`/v1/models` on mlx-lm lists the whole local cache, not the loaded model.**
  llama.cpp reports the model it serves. mlx-lm lists everything in the
  HuggingFace cache and loads whichever model your request names. Do not take
  the model id from that endpoint. Pass the repo id you intend to measure. A
  wrong id files the row under the wrong model name. That is worse than no row.

## What the scenarios test

The corpus is 50 scenarios in six categories. Each is plain TOML data in
`crates/wic-core/scenarios/`. Use `--scenarios <dir>` to run a modified set.

- `single` - one tool call per scenario. Argument shapes: strings, integers,
  decimals, booleans, enums, arrays, nested objects, empty arguments, and
  optional arguments, present and omitted.
- `parallel` - several tool calls in a single response.
- `streaming` - the same calls over SSE, reassembled from deltas. This catches
  servers that break only under streaming.
- `multi_turn` - the harness feeds a tool result back. The follow-up call must
  use a value that exists only in that result.
- `tool_choice` - `auto`, `none`, `required`, and a named function.
- `negative` - cases where the correct behavior is no tool call, and awkward
  argument content (a 256-character token, a non-ASCII city name).

Scoring is deterministic. There is no LLM judge. The project publishes failure
reasons, and a published reason has to be defensible.

Read a red cell carefully. A red in `parallel` means the model did not emit
several tool calls in one response. It does not mean the model is broken.

## A cell is a property of the whole stack

**The servers do not decode the same way. A green on one server and a red on
another is not, by itself, evidence about the model.**

llama.cpp compiles the tool definitions you send into a GBNF grammar and
constrains decoding with it. There, a call to a function that you did not supply
cannot be sampled. Ollama 0.32.1 with its default Go template, and mlx-lm,
generate unconstrained text. They parse the tool call out of that text
afterwards. The model can emit a call to a function that you did not supply, or
a malformed call. The server finds out only after the fact. So:

- A delta between llama.cpp and Ollama describes the stack. Read it as "this
  combination works". Do not read it as "Ollama is defective" or "this model is
  worse than that one".
- To isolate the model, compare results from the same server.
- A red can mean the server did not parse a valid call. The `unparsed_tool_call`
  failure class marks that case, and the transcript shows the bytes.

Ollama 0.34.0 can use the embedded Jinja template instead of the Go template.
Set `OLLAMA_GO_TEMPLATE=0` in the server environment. In one test on 0.34.0,
`qwen2.5:0.5b` returned an empty message with the Go template. It returned the
tool call with the Jinja template. See https://github.com/ollama/ollama/issues/17274.

Two models have a row on each server with the same blob SHA-256:
`granite3.1-dense:8b` and `phi4-mini:latest`. Each scored 7 of 50 on Ollama and
7 of 50 on llama.cpp.

Schema v3 results can record the decode mode of the server in
`metadata.server.decode_mode`. They can also keep the matching
`server.quirk_flags`: `grammar_constrained_decoding` for llama.cpp, and
`unconstrained_post_hoc_parse` for Ollama and mlx-lm. When a historical run did
not record the mode, the site reads it from the cited preset mapping in
`registry/decode-modes-v1.json`. A preset that is not in the mapping stays
`unknown`, never guessed.

This project once claimed that Ollama discarded valid tool calls. Recovering the
discarded bytes showed that the model wrote the tool's *description* where its
name belonged. Ollama's parser was right to reject the call, and the project
retracted the claim. The real finding is the decode difference.

## The scenario-authoring rule

**A scenario that a fully correct model could fail is a bug in the scenario.**

A false red is worse for this project than a missing test. If the matrix says a
model fails and it does not, readers stop trusting the other cells. So an
expectation must admit *every* correct answer, not just the author's own.

When you write or review a scenario:

- Prefer structural checks over string equality. Assert that the call happened,
  that the arguments validate against the schema, that the argument set matches.
- If a text value must match exactly, the prompt has to force it. Put the exact
  literal in the user message, or use an opaque identifier (`doc-17`, `EXT-55`).
  A model may legitimately expand or spell other values differently.
- Where the exact text does not matter, set `arguments_match = "ignore"` on that
  expected call. Use `"subset"` when extra arguments are acceptable.
- Arrays are compared positionally. If order is not part of the requirement, say
  "in that order" in the prompt, or do not use an array.
- Do not pin an operand position for a commutative operation.
- Do not expect a value the model has no way to know.

Every scenario has a `rationale` field. It states what the scenario asserts and
why no correct model can fail it for another reason. A pull request without a
rationale gets a request for one.

Two real bugs of this class were fixed. In `negative-unicode-argument`, a model
wrote a city name with an umlaut in decomposed Unicode (NFD), and the scorer
compared bytes. It now compares strings in NFC. Another scenario expected
`place = "Fenway Park"`, but a correct model geocodes `"Fenway Park, Boston, MA"`.

## Submitting a result

The matrix is fed by result files in `results/`. All matrix rows so far were
measured on one Apple M4 Max 64GB host. The live matrix at
https://devyrpauli.github.io/willitcall/ lists them.

1. Run the full corpus against your endpoint, one model loaded at a time. Two
   models at once cause spurious `error` outcomes from resource contention, not
   real measurements. Keep the script, the log, and the output of each batch
   together. Use one dated folder outside the repo, for example
   `~/willitcall-runs/2026-10-05-short-name/`.
2. Make sure `registry/models-v1.json` has an entry for the exact model
   selector, with an evidence reference for each claimed identity field. If you
   cannot recover the provenance, an `unresolved` entry is acceptable. The
   result and the site show that status. They do not infer identity from a name.
3. Run `willitcall validate` on the schema v3 output. The CLI still accepts
   legacy schema v1 and v2 files. Do not upgrade them by editing.
4. Open a pull request that adds the result file **and its `evidence/`
   directory** under `results/`. State the hardware and server version.

Every scenario writes a full request and response transcript to
`evidence/<run_id>/<scenario-id>.json`. The result file references it, so you
can inspect any red cell. The run redacts credential-bearing headers and URL
query parameters at capture time. If your endpoint is not local, read each
transcript before you publish it.

Do not hand-edit a result file or a transcript. `evidence_hash` is the SHA-256
of the transcript bytes, so you can detect an edit. An edited result is not
comparable with anything else in the matrix.

## Roadmap

Next: template forensics. Read the chat template out of GGUF metadata, and lint
it against known tool-calling breakage patterns. A red cell can then say "fails
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
