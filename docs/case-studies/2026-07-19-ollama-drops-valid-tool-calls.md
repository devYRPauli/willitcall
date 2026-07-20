# Ollama 0.32.1 silently discards a valid Qwen2.5 tool call

Status: confirmed, reproducible, not yet filed upstream.
Date: 2026-07-19.
Scenario: `single-array-tags`.

## Summary

Running willitcall's `single-array-tags` scenario against
`qwen2.5:7b-instruct` on Ollama produces an empty assistant response: no
tool call, no text content. The M2 session recorded this as a genuine model
failure. That verdict was wrong.

The model emits a perfectly well-formed `<tool_call>` block that matches the
format Ollama's own chat template asked for. Ollama's tool-call parser
discards it and returns `content: ""` with no `tool_calls`. The generated
tokens are still billed in `eval_count`, so the response contradicts itself:
it reports 40 tokens generated and returns zero bytes of them.

The same GGUF file served by llama.cpp returns the correct tool call every
time.

This is a server defect, not a model defect and not a quantization defect.

## Environment

- Machine: Apple Silicon, macOS (Darwin 25.5.0).
- Ollama 0.32.1, model `qwen2.5:7b-instruct`.
- llama.cpp `llama-server` version 10050 (b15ca938a), `--jinja`.
- Model file: the SAME bytes for both servers. llama.cpp was pointed
  directly at Ollama's own blob,
  `~/.ollama/models/blobs/sha256-2bada8a7450677000f678be90653b85d364de7db25eb5ea54136ada5f3933730`
  (4683073952 bytes, Q4_K_M), so weights and quantization are controlled for
  exactly rather than approximately.
- Sampling identical everywhere: `temperature=0.0`, `top_p=1.0`, `seed=42`,
  `max_tokens=1024`.

## The request

One tool, `tag_document`, with a required string `document_id` and a required
array-of-string `tags`. `tool_choice: "auto"`, `stream: false`. Single user
turn:

    Tag document doc-17 with exactly alpha, beta, and gamma in that order.

The exact request bodies are in `evidence/`. The runnable A/B is
`evidence/repro-single-array-tags.py`.

## What happened

| Path | Result | Reps |
|---|---|---|
| Ollama `/v1/chat/completions` (OpenAI-compat) | empty, `eval_count` 40 | 10/10 |
| Ollama `/api/chat` (native) | empty, `eval_count` 40 | 1/1 |
| Ollama `/v1` with `tool_choice: "required"` | empty, `eval_count` 40 | 1/1 |
| Ollama `/v1`, streaming | one chunk, `content: ""`, `finish_reason: stop` | 1/1 |
| Ollama `/api/generate` with `raw: true` | correct `<tool_call>` text | 1/1 |
| llama.cpp `/v1/chat/completions` `--jinja` | correct tool call | 6/6 |

The failure is deterministic, not flaky.

### It is not the model

Ollama's own engine, same weights, generating from a hand-rendered prompt via
`/api/generate` with `raw: true`, produced exactly this, `eval_count` 37:

    <tool_call>
    {"name": "tag_document", "arguments": {"document_id": "doc-17", "tags": ["alpha", "beta", "gamma"]}}
    </tool_call>

That is valid JSON, the correct function name, the correct arguments, the tags
in the requested order, wrapped in the delimiters Ollama's template explicitly
asked for:

    For each function call, return a json object with function name and
    arguments within <tool_call></tool_call> XML tags

llama.cpp fed the same prompt produced a byte-identical string. The generation
is correct on both engines. Only the parse differs.

### It is not the tool description

M2 reported that changing only the tool description made the call succeed. It
does not. Two descriptions were tested, five reps each:

- A: `Apply tags to a document.` -> empty 5/5
- B: `Apply a list of tags to a document.` -> empty 5/5

Both succeed 3/3 on llama.cpp. The M2 observation was a single unreplicated
sample and does not hold. The description is not the variable.

### It is not the OpenAI-compatibility shim

Ollama's native `/api/chat` fails identically, so the defect is below the
OpenAI-compat layer, in the shared template-and-parse path.

## Secondary finding: `tool_choice` is ignored (not a defect - retracted)

Ollama accepted `tool_choice: "required"` and returned a response with no tool
call and no error. This was originally written up here as a second defect, on
the reasoning that the OpenAI API contract requires the parameter to either
force a tool call or fail. That framing was wrong and is retracted.

A controlled test on 2026-07-20 settled it. Run against `qwen3:4b`, a model
whose tool calls Ollama parses correctly (verified: `tool_choice: "auto"`
returns a real `tool_calls` array), `tool_choice: "required"` was ignored 3/3 --
the model answered a question that needed no tool and emitted no call,
indistinguishable from `"auto"`. So the behavior is genuinely independent of the
parser drop rather than a consequence of it.

That independence is what disqualifies it. Ollama documents `tool_choice` as
unsupported, and maintainer `rick-github` states in `#14967` that it "is
accepted because it's part of the OpenAI API specification but is currently
ignored," and that "there is no mechanism for forcing a model to use a tool."
This is documented, intended behavior, not a bug, and it was deliberately left
out of `#17274`.

The consequence for willitcall stands even though the defect claim does not: a
harness cannot use `tool_choice: "required"` on Ollama to distinguish "model
declined to call" from "server dropped the call." That ambiguity is real, and it
is what made this bug read as a model failure for a whole milestone.

## Why this matters for willitcall

This is the project's premise, demonstrated: a red cell is a property of the
model *and quant and server*, not of the model alone. The same weights at the
same quant score differently on two runtimes because one of them loses valid
output. A matrix that reported only "qwen2.5:7b fails array arguments" would
be actively misleading.

Two consequences for the corpus:

1. `results/ollama-qwen2.5-7b-instruct.json` remains correct as an
   observation. The stack does fail the scenario. The M2 note's attribution
   ("real model/template fragility") is what was wrong.
2. Empty-response-with-nonzero-`eval_count` is a distinguishable signature and
   is worth surfacing in the matrix as a separate class from a real refusal.
   Not implemented; noted for M4.

## Upstream issue: filed as ollama/ollama#17274

Filed 2026-07-20: https://github.com/ollama/ollama/issues/17274

The dedicated verify-dupes pass ran on 2026-07-20 and cleared all three gates:

1. **Latest version.** 0.32.1 was itself the newest release (2026-07-16), so the
   measured version was the current one. No "fixed upstream" exit existed.
2. **Dupe search.** Two passes, the second briefed adversarially to disqualify
   the filing. Novel. `#16932` shares the symptoms but was root-caused by a
   maintainer to the ministral parser, which qwen2.5 never reaches; its fix PR
   `#16942` was rejected in review. `#12174` is the closest prior report
   (qwen2.5-coder, array parameter) but shows a non-empty `content` carrying
   leaked JSON, and its thread stalled on "the model is unreliable" -- the
   llama.cpp and `raw: true` cross-checks are the evidence that reading lacks,
   so the issue cites it directly. No merged-but-unreleased fix touches the
   qwen2.5 path.
3. **Independent re-verification.** Every row of the table above was re-run from
   scratch on macstudio without trusting the M3 session. All confirmed, no
   discrepancies; the llama.cpp side came back 12/12 across both tool
   descriptions.

The `eval_count` contradiction was folded into the same issue rather than filed
separately: it cannot occur independently of the drop.

## Reproducing

Start from a machine with `qwen2.5:7b-instruct` pulled in Ollama.

    python3 docs/case-studies/evidence/repro-single-array-tags.py \
      http://localhost:11434/v1 qwen2.5:7b-instruct ollama 5

Then serve Ollama's own blob with llama.cpp and repeat:

    ollama stop qwen2.5:7b-instruct
    llama-server -m ~/.ollama/models/blobs/sha256-2bada8a745...3730 \
      --jinja --port 8080 -c 4096 --alias qwen2.5-7b-instruct-q4km

    python3 docs/case-studies/evidence/repro-single-array-tags.py \
      http://localhost:8080/v1 qwen2.5-7b-instruct-q4km llamacpp 3

`--jinja` is required. Without it llama.cpp does not do tool calling at all.

## Evidence files

- `evidence/ollama-0.32.1-qwen2.5-7b-tags.json` -- full request and raw
  response body for variants A and B against Ollama.
- `evidence/llamacpp-b10050-qwen2.5-7b-tags.json` -- the same pair against
  llama.cpp on the same GGUF.
- `evidence/repro-single-array-tags.py` -- the A/B harness that produced both.
