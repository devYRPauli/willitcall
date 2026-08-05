# Local path evidence re-anchoring

This migration is an explicit re-anchoring of the published evidence chain, not
an attempt to conceal edits. Git retains the preimage at one common
`source_commit`; each ledger records the old and new transcript hashes without
republishing the old absolute model path. The rewritten transcript hash is then
linked from the owning result's `evidence_hash`.

`tools/redact_local_paths.py` operates on one 50-scenario run at a time. Before
writing anything, it resolves every result-provided evidence path beneath the
explicit `--evidence-root`, verifies transcript run and scenario identities,
checks the current byte hash against the owning result, and checks that the
working bytes are the bytes stored at `source_commit`. A single failure aborts
the run before temporary files are created.

Only `/turns/*/request/body/model` and the model identifier substring inside
`/turns/*/response/body_raw` may change in a transcript. `body_raw` contains the
server's own returned bytes as a JSON string; the tool rewrites only the exact
identifier substring without re-serializing that response, so no other response
byte changes. It parses the response only for the safety check: before writing,
it verifies that every copy of the absolute path is a complete JSON string value
of a `"model"` key. Blob paths become their `sha256-<digest>` identifier; the
watt-tool path becomes
`watt-tool-8B.Q4_K_M.gguf`. The result is changed only at `metadata.model_id`
and the corresponding scenario `evidence_hash` values. The hash re-anchor
covers the response substring rewrite. All transcript, result, and ledger
outputs are prepared as temporary files before any destination is replaced.

Record the common commit once, before the first of the six run migrations, and
pass that same full commit to every invocation:

```sh
SOURCE_COMMIT=$(git rev-parse HEAD)
python3 tools/redact_local_paths.py \
  --source-commit "$SOURCE_COMMIT" \
  --result results/<result>.json \
  --evidence-root results \
  --ledger docs/migrations/local-path-redaction-ledgers/<run-id>.json
```

Each per-run JSON ledger records the source commit, repository-relative result
path, run and scenario identities, evidence path, old and new SHA-256 values,
both permitted pointers, per-pointer replacement counts, replacement identifier,
and total mutation count. It never stores the old absolute path.

Verification reloads each preimage with `git show`, verifies both hashes and the
new result-to-transcript link, compares parsed old/new JSON after normalizing
only the permitted request model and response echo locations, confirms that no
absolute local path survives, and also confirms byte-for-byte that no other
transcript formatting, response byte, or result field changed:

```sh
python3 tools/redact_local_paths.py \
  --check \
  --ledger docs/migrations/local-path-redaction-ledgers/<run-id>.json \
  --evidence-root results

python3 tools/redact_local_paths.py \
  --check-all docs/migrations/local-path-redaction-ledgers \
  --evidence-root results
```

`--check-all` additionally requires every discovered ledger to name the same
`source_commit`. Coordinated edits to a transcript, its result hash, and its
ledger still fail if anything outside the two permitted locations was altered.
`--evidence-root` accepts the results directory shown above and also retains
compatibility with `results/evidence`.
