# llama.cpp returns HTTP 500 on Llama-3.1 tool calls it cannot re-parse

## Summary

Meta-Llama-3.1-8B-Instruct on llama.cpp loses 7-9 of 50 scenarios per run, not
to wrong answers but to the server returning:

```
HTTP 500
{"error":{"code":500,"message":"The model produced output that does not match
the expected peg-native format","type":"server_error"}}
```

The model is not the problem. Its output is recoverable from the server's own
log, and it is semantically correct: right function names, right arguments. The
server rejects it during a post-generation re-parse and escalates the failure to
a 500 rather than returning the text.

Because the lost scenarios are server errors rather than model failures, and
because the loss varies by arm (7, 8 and 9 scenarios at Q3_K_M, Q8_0 and Q4_K_M
respectively), the three Llama-3.1 quant arms are **excluded with reason** from
the quantization case study.

## What the model actually emitted

llama.cpp logs the text it failed to parse. Every failing scenario has a
`common_chat_peg_parse: unparsed peg-native output:` line, and the content is a
well-formed call:

```
; {"name": "get_time", "parameters": {"city": "Denver"}}
; {"name": "create_order", "parameters": {"sku": "QZ-4", "quantity": "3"}}
; {"name": "get_temperature", "parameters": {"city": "Tokyo"}}; {"name": "compare_temperatures", ...}
{"name": "add", "parameters": {"a": 19, "b": 23}}
```

The dominant shape is a bare JSON object, and where the model emits more than
one call it separates them with `; `. Those are the calls the scenarios asked
for. Nothing about them is wrong except that the server's parser will not accept
them.

## Mechanism

llama.cpp constrains tool-call decoding with a GBNF grammar built from the tool
definitions, which is what makes a hallucinated function name unrepresentable
there. That constraint is applied **lazily**: it engages only once a tool-call
start marker is sampled. Llama-3.1 has no hand-written chat handler in
`common/chat.cpp`; it falls through to the generic autoparser path, which
synthesizes a PEG grammar and matching parser from the Jinja template at load
time.

When the model emits a bare JSON object with no start marker, the lazy grammar
never triggers, so generation is effectively unconstrained. After generation,
`common_chat_peg_parse` re-parses the completed text with the synthesized PEG
parser. That parse fails - a leading `; ` separator is not something the root
rule accepts - and the failure is thrown as a `std::runtime_error`, which the
server catches generically and maps to HTTP 500. There is no fallback that
returns the raw text instead.

So this is not "the constraint failed". It is "the constraint never engaged, and
the separate post-hoc parser then rejected the result and hard-failed". That
distinction matters for reading the matrix: llama.cpp's grammar advantage is
conditional on the trigger firing, and this row is a case where it does not.

## Not version-bound

Homebrew is pinned at b10050, so `brew upgrade` was a no-op and could not answer
the question. The arm was re-run against the upstream b10075 release binary
instead, holding model, quant, flags, host and corpus identical.

| arm                | b10050 passed / errors | b10075 passed / errors |
|--------------------|------------------------|------------------------|
| Q8_0               | 20 / 8                 | 20 / 8                 |
| Q4_K_M             | 20 / 9                 | 20 / 9                 |
| Q3_K_M             | 25 / 7                 | 25 / 7                 |
| Qwen2.5-7B Q4_K_M  | 45 / 0                 | 45 / 0                 |

Identical on every arm, every run. The Qwen2.5-7B row is the control: it never
triggers the error on either version, so the comparison is clean and the
upgrade did not perturb anything else.

The anomaly is therefore reproducible and not fixed between b10050 and b10075.

## Scope and what is not claimed

- Observed on one model family. Qwen2.5-7B and Qwen2.5-1.5B on the same server,
  version, flags and corpus never trigger it.
- Affected scenarios skew to `multi_turn` (6 of the 8-9 errors per run), with
  the remainder in negative-trap and parallel-call scenarios.
- This does not say Llama-3.1 cannot call tools. It says this combination
  cannot, through this server's parser. Llama-3.1 on Ollama is a separate
  published row and does not fail this way.
- No claim is made about which component "should" change. A 500 is clearly the
  wrong failure mode for unparseable model output, but whether the fix belongs
  in the autoparser, the template, or the error handling is upstream's call.

## Upstream status

This is a known bug *class* that upstream is actively working through
per-format, but no existing issue covers this model and template. Related:

- ggml-org/llama.cpp#20260 (open) - Qwen3.5, prefix content before the tool
  marker breaks the peg-native parse. Closest in mechanism to this report.
- ggml-org/llama.cpp#25072 (open) - Gemma 4, same error message, different
  format.
- ggml-org/llama.cpp#25321 (open) - gpt-oss/harmony, non-streaming final hard
  throws with this exact message. PR #25332 proposes a fix, unmerged.
- ggml-org/llama.cpp#24807 / #24839 / #24863 (all closed) - the
  `Until(...)` GBNF-vs-PEG boundary disagreement and its attempted fix.

Recorded as a **candidate upstream issue, not yet filed.** Filing gets its own
duplicate-verification pass first, as the Ollama report did. The most useful
artifact for a maintainer is the `common_chat_peg_parse: unparsed peg-native
output:` log line together with the raw completion text, both of which are
captured in the evidence for these runs.

## Replication

- 3 arms x 5 runs on b10050 (from the quantization arm A), plus 3 arms x 5 runs
  on b10075, plus a 5-run control arm on each version.
- 40 runs, 50 scenarios each. Every run of a given arm produced an identical
  outcome. Meets the amendment 4 bar.
- Deterministic under greedy decoding, and the errors persist in the
  seed-varied arm at temperature 0.7, so this is not a sampling artifact.

## Environment

- Host: Apple M4 Max, 64GB, macOS 26.5.2 (single host for every run)
- llama.cpp b10050 (b15ca938a) and b10075 (76f46ad29), both started with
  `--jinja`
- Weights: `bartowski/Meta-Llama-3.1-8B-Instruct-GGUF` at Q8_0, Q4_K_M, Q3_K_M
