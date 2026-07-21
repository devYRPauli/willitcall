# willitcall

A caniuse-style compatibility matrix for tool calling on local models.

Every local inference stack claims OpenAI-compatible function calling. In
practice support varies by model, by quantization, by chat template, and by
server. `willitcall` is a small CLI that runs a fixed corpus of 50 tool-calling
scenarios against any OpenAI-compatible endpoint and emits a machine-readable
result file, so "does this model actually do parallel tool calls on llama.cpp"
becomes a fact you can look up instead of an afternoon you lose.

Status: the CLI and corpus work. The public matrix is live at
https://devyrpauli.github.io/willitcall/.

The full analysis of what these measurements found (the decoding mechanism
behind cross-server deltas, the quantization verdict, and the failure
taxonomy) is at
https://yashrajpandey.com/writing/same-weights-opposite-results/.

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

`--server` selects a preset (`llamacpp`, `ollama`, `mlx-lm`, `lmstudio`,
`vllm`, `custom`). The preset only supplies request defaults; the preset name is
recorded in the result file so results stay comparable.

The `mlx-lm` preset defaults to port 8081, not mlx-lm's own default of 8080,
because 8080 is this project's llama.cpp convention and two servers on one port
is exactly the contention the preflight exists to catch.

Two things to know before reading or adding an mlx-lm row:

- **MLX rows are converted weights.** MLX does not consume GGUF, so an MLX row
  for a model is not the same bits as the llama.cpp row for that model. The
  trick used elsewhere in this project of serving one blob through two servers
  to hold the weights constant does not work across this boundary. An MLX-vs-
  GGUF difference includes the conversion.
- **`/v1/models` on mlx-lm lists the whole local cache, not the loaded model.**
  llama.cpp reports the model it is serving; mlx-lm enumerates everything in the
  HuggingFace cache, and it will load whichever model your request names. Do not
  discover the model id from that endpoint - pass the repo id you intend to
  measure. Getting this wrong files a row under the wrong model name, which is
  worse than having no row.

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

## A cell is a property of the whole stack

**The servers do not decode the same way, so a green on one server and a red on
another is not by itself evidence about the model.**

llama.cpp compiles the tool definitions you send into a GBNF grammar and
constrains decoding with it. A tool call that names a function you did not
supply, or whose arguments do not fit the schema, is not merely unlikely there:
it cannot be sampled. Ollama and mlx-lm generate unconstrained text and parse a
tool call out of it afterwards, so the model can emit a wrong function name or a
malformed call, and the server finds out only after the fact.

That difference is systematic and it favours llama.cpp in every row, on every
model. So:

- A llama.cpp-versus-Ollama delta is a property of the stack. Read it as "this
  combination works", not as "Ollama is defective" or "this model is worse than
  that one".
- The comparison that isolates the model is same-server, not cross-server.
- A red under an unconstrained server can mean the model emitted something
  nearly right that the parser then rejected. The `unparsed_tool_call` failure
  class exists to mark exactly that case, and the transcript shows the bytes.

Each result records which side of this line its server sits on, in
`server.quirk_flags`: `grammar_constrained_decoding` for llama.cpp,
`unconstrained_post_hoc_parse` for Ollama and mlx-lm. LM Studio and vLLM are
unflagged because their decode path has not been verified here; absence of a
flag means unverified, not unconstrained.

This was established the hard way. An earlier version of this project published
a claim that Ollama discarded valid tool calls. Recovering the discarded bytes
showed the model had emitted the tool's *description* where its name belonged,
and Ollama's parser was right to reject it. The claim was retracted. The real
finding is the mechanism above.

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
