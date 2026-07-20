# Seed plan

What willitcall will run first, and why those cells. Written before any seeding
so the selection cannot be retrofitted to the results.

## Selection criteria

1. **What people actually run.** Biased toward Ollama library tags with large
   install bases and the GGUF repos the local-inference community links, not
   toward whatever tops a paper leaderboard.
2. **Span the size range.** 0.6B to 14B. The interesting question is where
   tool calling stops working, so the small end matters as much as the top.
3. **Fit the machine.** Apple Silicon, 16GB unified memory, one model loaded
   at a time. Weights must leave room for KV cache and the OS.
4. **Include known-bad cases on purpose.** A benchmark that only seeds models
   expected to pass is marketing. Entries below flagged "known-fragile" are
   included because documenting a real failure is the product.
5. **Cover both servers.** Ollama and llama.cpp use different chat templates
   for the same weights. That divergence is the thing this project measures,
   and it is not hypothetical -- see
   `case-studies/2026-07-19-ollama-drops-valid-tool-calls.md`, where the same
   GGUF passes on llama.cpp and fails on Ollama.

## Model list

Sizes are the actual model-layer bytes from the Ollama registry manifest,
probed 2026-07-19, not estimates.

| # | Model | Ollama tag | Size | Wave | Why |
|---|---|---|---|---|---|
| 1 | Qwen3 0.6B | `qwen3:0.6b` | 0.52 GB | 1 | Floor of the range. Can a 0.6B emit valid tool JSON at all? |
| 2 | Qwen3 1.7B | `qwen3:1.7b` | 1.36 GB | 1 | Small-agent size people actually deploy. |
| 3 | Qwen3 4B | `qwen3:4b` | 2.50 GB | 1 | Widely-run sweet spot. |
| 4 | Qwen3 8B | `qwen3:8b` | 5.23 GB | 1 | Most-cited current default for local tool calling. |
| 5 | Llama 3.1 8B | `llama3.1:8b` | 4.92 GB | 1 | Reference baseline; llama.cpp has native handling for this family. |
| 6 | Qwen2.5 7B | `qwen2.5:7b-instruct` | 4.68 GB | 2 | Already seeded in M2. Large install base; the Ollama parser bug case. |
| 7 | Llama-3-Groq-8B-Tool-Use | `llama3-groq-tool-use:8b` | 4.66 GB | 2 | Purpose-built tool-calling finetune. Should be a ceiling. |
| 8 | Hermes 3 8B | `hermes3:8b` | 4.66 GB | 2 | Popular agentic finetune with its own tool-call format. |
| 9 | Granite 3.1 8B | `granite3.1-dense:8b` | 4.99 GB | 2 | IBM enterprise tool-calling workhorse. |
| 10 | Mistral 7B v0.3 | `mistral:7b` | 4.37 GB | 2 | known-fragile. Function calling supported but inconsistently trained. |
| 11 | Phi-4-mini 3.8B | `phi4-mini` | 2.49 GB | 3 | Small MSFT model; function calling shipped late in the line. |
| 12 | Gemma 3 4B | `gemma3:4b` | 3.34 GB | 3 | known-fragile. Hugely popular generalist, documented template bugs. |
| 13 | Gemma 3 12B | `gemma3:12b` | 8.15 GB | 3 | known-fragile. Larger Gemma3, same template class of issue. |
| 14 | Qwen3 14B | `qwen3:14b` | 9.28 GB | 3 | Top of range. Borderline on 16GB, see below. |
| 15 | watt-tool-8B | community tag only | ~4.9 GB | 3 | known-fragile. BFCL-topping finetune with a reportedly missing chat template. |

Wave 1 is this session's target (5 models). Waves 2 and 3 follow in M4.

### Memory note

Qwen3 14B at 9.28 GB is the only borderline entry. Weights alone leave roughly
5-6 GB for KV cache, runtime and the OS on a 16GB machine. It gets a
conservative context length and its peak RSS gets recorded. This machine has a
prior OOM history, so 14B runs last, alone, with nothing else loaded.

### Not included

`qwen3.5` and `qwen3.6` were suggested during research as current releases.
Both return 404 from the Ollama registry (probed 2026-07-19), so neither is
seedable through the Ollama path today. Revisit in M4; if they exist only as
GGUF on Hugging Face they can still be seeded through llama.cpp.

## Quantization

Baseline for every model is **Q4_K_M** -- it is what the Ollama default tags
ship and therefore what most people are actually running.

Second and third quants, on a subset only:

- **Q8_0** as a near-fp16 control. If a scenario passes at Q8_0 and fails at
  Q4_K_M, that is a quantization effect. Without the control, a red cell
  cannot be attributed.
- **Q3_K_M** on the small models, where the cost of testing is trivial and the
  floor is most likely to be visible.

Honest statement of the evidence: the claim that quantization degrades
*structured output* faster than it degrades general chat quality is
**widely repeated and poorly evidenced**. The one primary source found
measuring function-calling accuracy against bit-width is CarbonCall
(arxiv.org/pdf/2504.20348). Most of the confident numbers circulating come
from aggregator blog content with no reproducible benchmark behind them.
llama.cpp's own docs do make one concrete primary-source claim, but it is
about *KV-cache* quantization (`-ctk q4_0`) degrading tool calling, which is a
different knob than weight quantization
(github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md).

So the quant axis is included precisely because the community consensus is
thin. This benchmark is positioned to produce that evidence rather than cite
it. The matrix should not assume the effect exists.

## Server handling

- llama.cpp **requires `--jinja`**. Without it, any request carrying `tools` is
  rejected outright. This applies to every model in the list.
- KV-cache quantization is left at the default. Given llama.cpp's own warning
  that `-ctk q4_0` degrades tool calling, quantizing the cache while measuring
  tool calling would confound the very axis under test.
- Ollama applies its own baked-in template, which frequently diverges from the
  upstream Hugging Face `chat_template.jinja` that the GGUF carries. Where a
  model is run on both servers, that divergence is a measured variable, not
  noise to be normalized away.

## Protocol

One model in memory at a time. `ollama stop <tag>` or kill `llama-server`
between runs. Every result file is schema-validated before it is committed.
Result plus evidence transcripts are committed together, so every red cell in
the published matrix has a full request/response transcript behind it.

## Outcome (2026-07-20, M4)

All 15 cells are seeded. Wave 1 was measured on an M1 Pro; waves 2 and 3 on an
M4 Max, both with Ollama 0.32.1 and llama.cpp b10050. Notes worth carrying:

- `phi4-mini` must be requested as `phi4-mini:latest`. The preflight compares
  the model id against `/v1/models` exactly, and the bare name is rejected.
- `gemma3:4b` and `gemma3:12b` do not produce a measurement at all: Ollama
  answers every request carrying `tools` with HTTP 400 `does not support
  tools`, so both cells are 50 errors rather than 50 failures.
- watt-tool-8B has no official Ollama tag and `ollama pull hf.co/...` fails on
  0.32.1 with a redirect-realm error, so it was seeded through llama.cpp from
  mradermacher/watt-tool-8B-GGUF Q4_K_M. It answers in prose without
  attempting a call, which is consistent with the reported template problem.
- granite3.1-dense:8b and phi4-mini emit correct calls in their own formats
  that neither server parses. Serving the same Ollama blobs through llama.cpp
  reproduces both scores exactly, which is what rules out a single-server
  defect. See the proposed amendment 5 in the design spec.
- The quant axis (Q8_0, Q3_K_M controls) is still untouched; every seeded cell
  is the default tag.
