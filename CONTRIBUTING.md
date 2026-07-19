# Contributing

## Running the test suite

Run all checks from the repository root:

```sh
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
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
2. Run `willitcall validate results/<file>.json`; it must pass against `schemas/result-v1.schema.json`.
3. Open a pull request adding the file under `results/`, and state the hardware and server version used.

Never hand-edit a result file. Each scenario record carries an evidence hash, so edited results are not comparable.
