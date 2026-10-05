# Contributing

## Running the test suite

Run all checks from the repository root:

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

If you change `tools/` or a path redaction ledger, also run the two Python
checks that CI runs:

```sh
python3 -m unittest discover -s tools -v
python3 tools/redact_local_paths.py --check-all migrations/path-redaction --evidence-root results
```

## Adding a scenario

> **A scenario that a fully correct model could fail is a bug in the scenario. An expectation must admit EVERY correct answer, not just the one the author had in mind.**

Before submitting a scenario:

- Prefer structural checks over string equality.
- Put an exact literal in the prompt, or use an opaque identifier, when a string must match exactly.
- Use `arguments_match = "ignore"` for open-ended arguments or `"subset"` when additional arguments are acceptable.
- Arrays are positional, so pin their order explicitly in the prompt.
- Never pin operand position for a commutative operation.
- Never expect a value the model cannot know from the prompt or an earlier tool result.
- Add a `rationale` that says what capability is asserted and why the expectation admits every correct answer.
- Set `response_requirement` on every turn. Use `tool_calls` when the turn expects calls, and `text_without_tool_calls` when it expects none.

Save the scenario as `crates/wic-core/scenarios/<id>.toml`. The file name must equal the `id` field.

This example pins an opaque id and uses subset matching so an optional argument cannot cause a false failure:

```toml
id = "single-record-lookup"
category = "single_call"
description = "Look up one record by its opaque id."
rationale = "This asserts a record lookup with a required id. The opaque value rec-17 appears verbatim in the prompt, and subset matching permits any legitimate optional arguments."
arguments_match = "subset" # Permit additional optional arguments.

[[tools]]
name = "get_record"
description = "Get a record by id."
[tools.parameters]
type = "object"
required = ["record_id"]
[tools.parameters.properties.record_id]
type = "string"

[tool_choice]
mode = "auto"

[[turns]]
response_requirement = "tool_calls"

[[turns.messages]]
role = "user"
content = "Get record rec-17." # The expected value is literal.

[[turns.expected_calls]]
name = "get_record"
[turns.expected_calls.arguments]
record_id = "rec-17"
```

## Submitting a result file

1. Run the full scenario corpus against one loaded model at a time.
2. Add the exact model selector to `registry/models-v1.json`. Every claimed
   identity field needs a provenance reference. If you cannot recover the
   provenance, an explicit `unresolved` entry is acceptable. The result and the
   site show that status. Do not infer identity from the selector or the result
   filename.
3. Run `willitcall validate results/<file>.json`; new runs must pass as schema v3
   against `schemas/result-v3.schema.json`. The CLI continues to accept schema v1
   and v2 files without upgrading them during `annotate` or `rescore`.
4. Open a pull request that adds the file and its `evidence/` directory under `results/`. State the hardware and the server version.

Never hand-edit a result file. Each scenario record carries an evidence hash, so edited results are not comparable.

### Preflight metadata

`metadata.preflight_override` is present only when `--force` allows a run despite detected foreign inference endpoints. Its `forced` flag is `true`, and `foreign_endpoints` records the endpoints that were detected.

`metadata.preflight_ignored_ports` is present only when one or more `--ignore-port` flags narrow contention detection. It records the ignored port numbers, which were not probed and could have contained undetected inference servers.

### Failure classes

`failure_class` is a single mechanical observation assigned only after a scenario fails. A scenario with status `error` has no class. For a failure, `empty_response` takes precedence over `unparsed_tool_call`, and any other failure has no class. `empty_response` means the response had neither content nor a parsed tool call. `unparsed_tool_call` means the server parsed no tool call, but the content matches a registered tool-call shape. The function in that shape was offered, and its arguments pass the tool's parameter schema. A `cause` is a separate human attribution.
