# granite3.1-dense:8b emits correct tool calls that no server parses

## Summary

granite3.1-dense:8b scores 7/50 on Ollama 0.32.1 and 7/50 on llama.cpp
b10050. Read naively, that says the model cannot call tools. It can. It emits
well-formed calls, with the right function name and the right arguments, in a
`<tool_call>` wrapper that neither server parses into `tool_calls`. The text
is left sitting in `content`, so the harness sees no tool call and the
scenario fails.

The seven scenarios it passes are exactly the seven negative traps, where
emitting no parsed call is the correct answer. That is the tell: the model
scores full marks precisely where not calling a tool is right, and zero
elsewhere.

This is not the M3 finding. M3 was read as Ollama discarding a valid call from
its own engine; that reading was disproved on 2026-07-21 (the model had put
the tool's description in the `name` field, so the parser correctly rejected
it - see the qwen2.5 case study). Here the situation is different and better
evidenced: nothing is discarded, because the output is sitting in `content`
where it can be inspected directly. The model's output format and the servers'
parsers do not agree. Both servers fail the same way, so this is not
attributable to either one as a defect, and no `cause` annotation is applied.

Note the classifier below is what makes this claim safe: it counts
`unparsed_tool_call` only when the function name is one the scenario actually
offered and the arguments validate against that tool's schema. That check is
exactly what the M3 claim lacked, and it is why the wrong-`name` cases here
are excluded rather than counted as correct emissions.

One open thread: llama.cpp constrains tool-call decoding with a GBNF grammar
built from the tool definitions, applied lazily on the tool-call start token.
That granite still fails 43/50 there suggests the grammar never triggers for
this model's output format, which would be consistent with both arms failing
identically. Not verified; noted for anyone picking this up.

Replication: 5 runs per arm, 2 arms, 50 scenarios each. Every run in both
arms produced an identical outcome, down to the same set of passing scenario
ids. This meets the amendment 4 bar of at least five runs per arm.

## Environment

- Host: Apple M4 Max, 64GB, macOS 26.5.2 (single host for every run)
- Ollama 0.32.1, model `granite3.1-dense:8b`
- llama.cpp b10050 (b15ca938a), serving Ollama's own blob
  `sha256-44d19d212d76a6f3fc442e8411fdb44ea6b67ceccfb00be4b4345c9a4cf813e8`
  with `--jinja`
- Sampling: temperature 0.0, top_p 1.0, seed 42, max_tokens 1024

Serving the same blob through llama.cpp is what separates "the weights" from
"the server". The weights are held constant; only the server changes.

## What the model actually emits

From `multi-turn-calendar-followup`, Ollama arm, run 1:

```
<tool_call>[{"arguments":{"date": "2026-08-11"},"name":"get_calendar"},
 {"arguments": {"title": "Review", "date": "2026-08-11",
  "time": "<available_time>"},"name":"create_event"}]
```

and in the same response:

```
"tool_calls": null
```

The function names are the ones the scenario offered. The arguments validate
against those tools' parameter schemas. The only thing wrong is that the
server never turned this into `tool_calls`.

## Results

| Arm | Runs | Passed | Failed | `unparsed_tool_call` | Distinct outcomes |
|---|---|---|---|---|---|
| Ollama 0.32.1 | 5 | 7/50 | 43 | 35 | 1 |
| llama.cpp b10050 | 5 | 7/50 | 43 | 42 | 1 |

Zero variance across runs in both arms, and the set of passing scenario ids
was identical in all ten runs.

The classifier is deliberately conservative, which is why the counts are 35
and 42 rather than 43. It assigns `unparsed_tool_call` only when a function
name is extractable, that name is one the scenario actually offered, and the
arguments validate against that tool's schema. The unclassified remainder are
genuine model errors that happen to share the wrapper:

- eight Ollama cases omit the `"name"` field entirely, e.g.
  `<tool_call>[{"arguments":{"city": "München"}}]`
- one names `"function_call"`, which is not a tool the scenario offered
- one llama.cpp case uses `"args"` instead of `"arguments"`

Those are not calls the model got right, so they are not credited as such. A
false positive here would credit a model with a call it never made.

## Retraction: phi4-mini does not have this problem

The M4 note and the first draft of amendment 5 claimed phi4-mini shares this
failure mode, emitting calls as ``[`get_weather` {...}]``. **That claim was
wrong and is retracted.**

Checking every failing response across both servers:

| Row | Failures | `unparsed_tool_call` |
|---|---|---|
| ollama phi4-mini | 43 | 1 |
| llamacpp phi4-mini | 43 | 0 |

One response in 86 matches a tool-call shape. The rest are prose: the model
declines, narrates, or describes the tool instead of calling it. A
representative failure reads

> "I'm sorry, but I don't have real-time capabilities to provide current
> information. However, you can use my function `get_time` with the city
> parameter set as either 'Boston' or 'Tokyo'."

That is a model that did not call a tool, not a call that went unparsed.
phi4-mini's 7/50 is largely genuine failure and it should not appear in this
finding.

The original claim was generalised from a single example, which is the
failure mode amendment 4 exists to prevent, and the second time this project
has had to overturn an n=1 conclusion. The lesson is recorded here rather
than quietly dropped, because a benchmark that hides its own corrections is
not measuring honestly.

## Why this matters for willitcall

A red cell for granite3.1-dense:8b is truthful about the combination: if you
send this model tool definitions through either of these servers today, you
get no usable tool call. But "the model cannot call tools" is false, and the
matrix previously had no vocabulary to say so.

That is what `failure_class: "unparsed_tool_call"` is for. It records a
mechanical observation, distinct from a `cause`, which is a human attribution
made only after isolation. The cell stays red because the stack does not
work; the badge tells model authors we are not blaming their weights.

## Reproducing

```
# Ollama arm
willitcall run --endpoint http://localhost:11434/v1 \
  --model granite3.1-dense:8b --server ollama --out granite-ollama.json

# llama.cpp arm, same weights
llama-server -m ~/.ollama/models/blobs/sha256-44d19d212d76... --jinja --port 8080
willitcall run --endpoint http://127.0.0.1:8080/v1 \
  --model <id from /v1/models> --server llamacpp --out granite-llamacpp.json
```

Both should report 7 passed, 43 failed, and the passing seven should be the
negative traps.

## Evidence files

- `evidence/granite-ollama-run1.json` - full result, Ollama arm, run 1
- `evidence/granite-llamacpp-run1.json` - full result, llama.cpp arm, run 1

Each result's `evidence_path` entries point at the per-scenario transcripts
containing the raw request and response bodies quoted above.
