# Replication runs

These are the runs behind the replication claims in the case studies. Each
JSON file is one willitcall run of 50 scenarios, as the CLI wrote it, in
result schema version 2. They are not rows of the matrix. The rows in
`results/` are separate single runs in schema version 3.

All runs used one Apple M4 Max 64GB host on macOS 26.5.2.

| folder | runs | server | case study |
|---|---|---|---|
| `m5-replication/` | granite3.1-dense 8B, 5 runs per server, greedy | Ollama 0.32.1 and llama.cpp b10050 | granite |
| `m6-armA/` | 9 quant arms x 5 runs, greedy | llama.cpp b10050 | quantization, llama.cpp 500s |
| `m6-armS/` | the same 9 arms x 5 runs, temperature 0.7, seeds 1 to 5 | llama.cpp b10050 | quantization, llama.cpp 500s |
| `m6-armB/` | Llama-3.1-8B at Q3_K_M, Q4_K_M and Q8_0, and Qwen2.5-7B at Q4_K_M, 5 runs each, greedy | llama.cpp b10075 | llama.cpp 500s |
| `m6-mlx-repl/` | Qwen2.5-7B 4-bit and 8-bit, 5 runs each, greedy | mlx-lm 0.31.3 | mlx 8-bit |

Greedy means temperature 0, top_p 1.0 and seed 42. The 9 quant arms are
Llama-3.1-8B, Qwen2.5-1.5B and Qwen2.5-7B, each at Q3_K_M, Q4_K_M and Q8_0.
The `*-version.txt` files hold the server version output for each folder.

## What is not here

The transcripts are not published. There is one per scenario, 6,500 in all.
Each scenario in a run file records `evidence_hash`, the SHA-256 of its
transcript file, so a transcript can be checked against its run.

## One change from the raw output

In the five `m5-replication/llamacpp-granite-run*.json` files,
`metadata.model_id` held the absolute path of the Ollama blob on the
measurement host. It now holds the blob name, `sha256-<digest>`, as in
`docs/migrations/2026-08-05-local-path-redaction.md`. No other byte changed.
The two run 1 files are also at `../granite-ollama-run1.json` and
`../granite-llamacpp-run1.json`.
