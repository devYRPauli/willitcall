# Quantization does not cleanly order tool-calling ability

## Summary

The folklore is that heavier quantization degrades tool calling, so Q8_0 should
beat Q4_K_M should beat Q3_K_M. Measured across three quantization levels of two
Qwen2.5 models on llama.cpp, that ordering does not appear. Nor does its
opposite.

Under greedy decoding all three Qwen2.5-7B quants land within three scenarios of
each other, and the apparent winner is the *lowest* quant. Under seed-varied
decoding that apparent win disappears into the sampling spread. What survives
replication is a weaker but cleaner statement:

**On this corpus, on these models, on llama.cpp, quantization level from Q8_0
down to Q3_K_M does not predict tool-calling ability, and the ordering is not
even monotonic.**

This case study also retracts a stronger claim. M5 measured these arms once each
and read the result as "equal or higher at the lowest quant, against folklore".
That reading is not supported. It was an artifact of two things: n=1, and greedy
decoding that made n=5 look like confirmation when it was re-measurement.

## What was wrong with the first measurement

M5 recorded Qwen2.5-7B as 47/45/48 across Q8_0/Q4_K_M/Q3_K_M and read the 48 at
Q3_K_M as beating the 47 at Q8_0. Re-running each arm five times reproduced
those numbers *exactly* - 47, 47, 47, 47, 47 and so on, down to the identical
set of failing scenario ids across all 45 runs.

That perfect stability was not confirmation. The runner hardcoded
`temperature: 0.0` and `seed: 42`, so decoding was greedy and every repeat run
of an arm was the same computation. Repetition under greedy decoding measures
whether the harness is deterministic, not whether a difference between arms is
real. Varying the seed alone would not have helped either: at temperature 0
sampling is argmax and the seed is inert.

Both parameters are now CLI flags (defaults unchanged, so the published matrix
stays reproducible), and the difference-claim was re-tested with five distinct
seeds at temperature 0.7. See amendment 7 in the design spec.

## Numbers

Scores are scenarios passed out of 50. "greedy" is the deterministic default
(temperature 0, seed 42), identical on all five runs. "seed-varied" is five runs
at temperature 0.7 with seeds 1-5.

### Qwen2.5-7B-Instruct

| quant   | greedy | seed-varied         | mean | range |
|---------|--------|---------------------|------|-------|
| Q8_0    | 47     | 46, 47, 47, 47, 47  | 46.8 | 46-47 |
| Q4_K_M  | 45     | 45, 45, 45, 45, 47  | 45.4 | 45-47 |
| Q3_K_M  | 48     | 45, 46, 46, 47, 48  | 46.4 | 45-48 |

All three ranges overlap. The greedy result put Q3_K_M a point above Q8_0; the
seed-varied means put it 0.4 below, well inside the spread. There is no
detectable difference between these three quantizations on this corpus.

### Qwen2.5-1.5B-Instruct

| quant   | greedy | seed-varied         | mean | range |
|---------|--------|---------------------|------|-------|
| Q8_0    | 42     | 39, 41, 41, 42, 42  | 41.0 | 39-42 |
| Q4_K_M  | 42     | 36, 37, 37, 38, 39  | 37.4 | 36-39 |
| Q3_K_M  | 40     | 37, 38, 39, 40, 40  | 38.8 | 37-40 |

Here Q8_0 is the strongest, which is the folklore direction. But the ordering is
**not monotonic**: Q4_K_M is the worst of the three, below the more aggressively
quantized Q3_K_M, and their ranges barely touch. Whatever is driving the
difference, it is not "fewer bits, worse calls".

Note also that greedy decoding scored Q4_K_M at 42, tying it with Q8_0, while
its seed-varied mean is 37.4 - the largest greedy-vs-sampled gap in the study.
A single greedy run flattered this arm by roughly five scenarios.

### Meta-Llama-3.1-8B-Instruct: excluded

Excluded from the quant verdict, with reason. Every run of all three arms lost
7-9 scenarios to llama.cpp returning HTTP 500 with `The model produced output
that does not match the expected peg-native format`. Those are server errors,
not model failures, and they depress the pass count by an amount that varies by
arm, so the three quants are not comparable to each other or to anything else.
The anomaly is treated separately; see the peg-native case study.

## Why the non-monotonicity is not surprising

Different quantizations are not the same weights at different precisions. They
are separately produced files, and K-quants apply different bit widths to
different tensors, guided by an importance matrix. Two quants of one model can
therefore differ in *which* weights were preserved, not merely in how much
precision was kept overall. There is no reason a coarser overall quantization
must preserve less of whatever tool-call formatting depends on.

This is a hypothesis for the shape of the result, not something this study
measured. It was not tested here.

## Scope

This is deliberately narrow.

- Two models, both Qwen2.5. Not a claim about quantization in general, or about
  any other model family.
- One server, llama.cpp b10050, which grammar-constrains tool-call decoding. A
  constrained decoder plausibly absorbs exactly the kind of format degradation
  that quantization would otherwise expose, so this result should **not** be
  assumed to carry over to Ollama or mlx-lm, which parse unconstrained output
  post hoc.
- One corpus of 50 tool-calling scenarios. "Tool-calling ability" here means
  this corpus, not general capability.
- Five seeds per arm at one temperature. Enough to see that the 7B differences
  sit inside the spread; not enough to put a confidence interval on a small
  difference.
- GGUF weights from a single publisher per model.

## Replication

- 9 arms greedy x 5 runs = 45 runs, llama.cpp b10050.
- 9 arms seed-varied x 5 seeds = 45 runs, llama.cpp b10050, temperature 0.7,
  seeds 1-5.
- 90 runs total, 50 scenarios each. This meets the amendment 4 bar of at least
  five runs per arm, and the amendment 7 requirement that a difference-claim be
  supported by a seed-varied arm.

## Environment

- Host: Apple M4 Max, 64GB, macOS 26.5.2 (single host for every run)
- llama.cpp b10050 (b15ca938a), started with `--jinja`
- Weights: `Qwen/Qwen2.5-7B-Instruct-GGUF` and `Qwen/Qwen2.5-1.5B-Instruct-GGUF`
  at Q8_0, Q4_K_M and Q3_K_M
