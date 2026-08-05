#!/usr/bin/env python3
"""Redact local model paths while preserving the evidence hash chain."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
from typing import Any


EXPECTED_SCENARIOS = 50
REQUEST_MODEL_POINTER = "/turns/*/request/body/model"
RESPONSE_BODY_RAW_POINTER = "/turns/*/response/body_raw"
PERMITTED_POINTERS = [REQUEST_MODEL_POINTER, RESPONSE_BODY_RAW_POINTER]
LEDGER_VERSION = 1
_BLOB_IDENTIFIER = re.compile(r"sha256-[0-9a-f]{64}\Z")
_WATT_IDENTIFIER = "watt-tool-8B.Q4_K_M.gguf"


class RedactionError(RuntimeError):
    """Raised when a migration or verification invariant is violated."""


def _sha256(data: bytes) -> str:
    return f"sha256:{hashlib.sha256(data).hexdigest()}"


def _load_json(data: bytes, label: str) -> Any:
    def reject_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise RedactionError(f"{label}: duplicate JSON key {key!r}")
            result[key] = value
        return result

    try:
        return json.loads(data, object_pairs_hook=reject_duplicates)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise RedactionError(f"{label}: invalid JSON: {exc}") from exc


def _json_value_spans(
    data: bytes, label: str
) -> dict[tuple[Any, ...], tuple[int, int, Any]]:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise RedactionError(f"{label}: JSON is not UTF-8") from exc
    if text.encode("utf-8") != data:
        raise RedactionError(f"{label}: JSON bytes do not round-trip as UTF-8")

    decoder = json.JSONDecoder()
    spans: dict[tuple[Any, ...], tuple[int, int, Any]] = {}

    def skip_space(index: int) -> int:
        while index < len(text) and text[index] in " \t\r\n":
            index += 1
        return index

    def parse_value(index: int, path: tuple[Any, ...]) -> int:
        index = skip_space(index)
        if index >= len(text):
            raise RedactionError(f"{label}: unexpected end of JSON")
        if text[index] == "{":
            index = skip_space(index + 1)
            if index < len(text) and text[index] == "}":
                return index + 1
            while True:
                try:
                    key, key_end = decoder.raw_decode(text, index)
                except json.JSONDecodeError as exc:
                    raise RedactionError(f"{label}: invalid object key: {exc}") from exc
                if not isinstance(key, str):
                    raise RedactionError(f"{label}: object key is not a string")
                index = skip_space(key_end)
                if index >= len(text) or text[index] != ":":
                    raise RedactionError(f"{label}: missing colon after object key")
                index = parse_value(index + 1, path + (key,))
                index = skip_space(index)
                if index < len(text) and text[index] == "}":
                    return index + 1
                if index >= len(text) or text[index] != ",":
                    raise RedactionError(f"{label}: missing object separator")
                index = skip_space(index + 1)
        if text[index] == "[":
            index = skip_space(index + 1)
            if index < len(text) and text[index] == "]":
                return index + 1
            item_index = 0
            while True:
                index = parse_value(index, path + (item_index,))
                item_index += 1
                index = skip_space(index)
                if index < len(text) and text[index] == "]":
                    return index + 1
                if index >= len(text) or text[index] != ",":
                    raise RedactionError(f"{label}: missing array separator")
                index = skip_space(index + 1)
        try:
            value, end = decoder.raw_decode(text, index)
        except json.JSONDecodeError as exc:
            raise RedactionError(f"{label}: invalid JSON value: {exc}") from exc
        if path in spans:
            raise RedactionError(f"{label}: duplicate JSON path {path!r}")
        spans[path] = (index, end, value)
        return end

    end = skip_space(parse_value(0, ()))
    if end != len(text):
        raise RedactionError(f"{label}: trailing data after JSON document")
    return spans


def _replace_string_values(
    data: bytes,
    replacements: dict[tuple[Any, ...], tuple[str, str]],
    label: str,
) -> bytes:
    text = data.decode("utf-8")
    spans = _json_value_spans(data, label)
    edits: list[tuple[int, int, str]] = []
    for path, (old_value, new_value) in replacements.items():
        if path not in spans:
            raise RedactionError(f"{label}: missing permitted JSON path {path!r}")
        start, end, actual = spans[path]
        if actual != old_value:
            raise RedactionError(
                f"{label}: value at {path!r} does not match the expected preimage"
            )
        edits.append((start, end, json.dumps(new_value)))
    for start, end, encoded_value in sorted(edits, reverse=True):
        text = text[:start] + encoded_value + text[end:]
    return text.encode("utf-8")


def _model_paths(document: Any, label: str) -> list[tuple[tuple[Any, ...], str]]:
    if not isinstance(document, dict) or not isinstance(document.get("turns"), list):
        raise RedactionError(f"{label}: transcript turns must be an array")
    paths: list[tuple[tuple[Any, ...], str]] = []
    for index, turn in enumerate(document["turns"]):
        try:
            model = turn["request"]["body"]["model"]
        except (KeyError, TypeError) as exc:
            raise RedactionError(
                f"{label}: missing /turns/{index}/request/body/model"
            ) from exc
        if not isinstance(model, str):
            raise RedactionError(
                f"{label}: /turns/{index}/request/body/model is not a string"
            )
        paths.append((("turns", index, "request", "body", "model"), model))
    if not paths:
        raise RedactionError(f"{label}: transcript has no request model pointers")
    return paths


def _body_raw_paths(document: Any, label: str) -> list[tuple[tuple[Any, ...], str]]:
    paths: list[tuple[tuple[Any, ...], str]] = []
    for index, turn in enumerate(document["turns"]):
        try:
            body_raw = turn["response"]["body_raw"]
        except (KeyError, TypeError) as exc:
            raise RedactionError(
                f"{label}: missing /turns/{index}/response/body_raw"
            ) from exc
        if not isinstance(body_raw, str):
            raise RedactionError(
                f"{label}: /turns/{index}/response/body_raw is not a string"
            )
        paths.append((("turns", index, "response", "body_raw"), body_raw))
    return paths


def _json_model_count(data: bytes, model_id: str, label: str) -> int:
    body_raw = data.decode("utf-8")
    spans = _json_value_spans(data, label)
    count = 0
    for path, (start, end, value) in spans.items():
        if any(isinstance(part, str) and model_id in part for part in path):
            raise RedactionError(f"{label}: absolute path occurs in a JSON key")
        if not isinstance(value, str) or model_id not in value:
            continue
        if not path or path[-1] != "model" or value != model_id:
            raise RedactionError(
                f'{label}: absolute path is not a complete "model" string value'
            )
        token = body_raw[start:end]
        if token.count(model_id) != 1:
            raise RedactionError(
                f"{label}: model path is not an exact replaceable substring"
            )
        count += 1
    if body_raw.count(model_id) != count:
        raise RedactionError(
            f'{label}: absolute path is not a complete "model" string value'
        )
    if count == 0:
        raise RedactionError(f"{label}: response does not echo the model identifier")
    return count


def _sse_model_count(data: bytes, model_id: str, label: str) -> int:
    count = 0
    payload_count = 0
    done = False
    for line_number, line in enumerate(data.splitlines(keepends=True), 1):
        if line.endswith(b"\r\n"):
            content = line[:-2]
        elif line.endswith((b"\r", b"\n")):
            content = line[:-1]
        else:
            content = line
        if not content:
            continue
        if not content.startswith(b"data: "):
            raise RedactionError(f"{label}: invalid SSE line {line_number}")
        payload = content[len(b"data: ") :]
        if payload == b"[DONE]":
            if done:
                raise RedactionError(f"{label}: duplicate SSE [DONE] sentinel")
            done = True
            continue
        if done:
            raise RedactionError(f"{label}: SSE data follows the [DONE] sentinel")
        document = _load_json(payload, f"{label}:SSE line {line_number}")
        if not isinstance(document, dict):
            raise RedactionError(
                f"{label}: SSE line {line_number} payload is not a JSON object"
            )
        count += _json_model_count(
            payload, model_id, f"{label}:SSE line {line_number}"
        )
        payload_count += 1
    if not payload_count or not done:
        raise RedactionError(f"{label}: SSE response is not terminated by [DONE]")
    return count


def _raw_model_count(body_raw: str, model_id: str, label: str) -> int:
    data = body_raw.encode("utf-8")
    try:
        document = _load_json(data, label)
    except RedactionError as json_error:
        try:
            return _sse_model_count(data, model_id, label)
        except RedactionError as sse_error:
            raise RedactionError(
                f"{label}: body_raw is neither a JSON object nor well-formed SSE: "
                f"{sse_error}"
            ) from json_error
    if not isinstance(document, dict):
        raise RedactionError(f"{label}: response body_raw is not a JSON object")
    return _json_model_count(data, model_id, label)


def _permitted_replacement_counts(
    data: bytes, document: Any, model_id: str, label: str
) -> dict[tuple[Any, ...], int]:
    request_paths = _model_paths(document, label)
    if any(value != model_id for _, value in request_paths):
        raise RedactionError(f"{label}: request model does not match metadata.model_id")
    response_paths = _body_raw_paths(document, label)
    request_set = {path for path, _ in request_paths}
    response_set = {path for path, _ in response_paths}
    spans = _json_value_spans(data, label)

    for path, (_, _, value) in spans.items():
        if any(isinstance(part, str) and model_id in part for part in path):
            raise RedactionError(f"{label}: absolute path occurs in a JSON key")
        if not isinstance(value, str) or model_id not in value:
            continue
        if path in request_set and value == model_id:
            continue
        if path in response_set:
            continue
        raise RedactionError(
            f'{label}: absolute path is not at a permitted "model" value'
        )

    counts = {path: 1 for path, _ in request_paths}
    for path, body_raw in response_paths:
        counts[path] = _raw_model_count(
            body_raw, model_id, f"{label}:{_pointer(path)}"
        )
    return counts


def _pointer(path: tuple[Any, ...]) -> str:
    return "/" + "/".join(str(part) for part in path)


def _normalize_transcript(document: Any, model_id: str, label: str) -> Any:
    normalized = copy.deepcopy(document)
    for path, _ in _model_paths(normalized, label):
        normalized[path[0]][path[1]][path[2]][path[3]][path[4]] = "<model>"
    for path, body_raw in _body_raw_paths(normalized, label):
        normalized[path[0]][path[1]][path[2]][path[3]] = body_raw.replace(
            model_id, "<model>"
        )
    return normalized


def _replace_body_raw_substrings(
    data: bytes,
    counts: dict[tuple[Any, ...], int],
    old_value: str,
    new_value: str,
    label: str,
) -> bytes:
    text = data.decode("utf-8")
    spans = _json_value_spans(data, label)
    edits: list[tuple[int, int, str]] = []
    for path, count in counts.items():
        if path not in spans:
            raise RedactionError(f"{label}: missing permitted JSON path {path!r}")
        start, end, actual = spans[path]
        if not isinstance(actual, str) or actual.count(old_value) != count:
            raise RedactionError(
                f"{label}: response value at {path!r} does not match the preimage"
            )
        encoded_value = text[start:end]
        if encoded_value.count(old_value) != count:
            raise RedactionError(
                f"{label}: response path is not an exact replaceable substring"
            )
        edits.append((start, end, encoded_value.replace(old_value, new_value)))
    for start, end, encoded_value in sorted(edits, reverse=True):
        text = text[:start] + encoded_value + text[end:]
    return text.encode("utf-8")


def _replacement_identifier(model_id: str) -> str:
    if not model_id.startswith("/Users/"):
        raise RedactionError("metadata.model_id is not an absolute local /Users path")
    identifier = PurePosixPath(model_id).name
    if _BLOB_IDENTIFIER.fullmatch(identifier) or identifier == _WATT_IDENTIFIER:
        return identifier
    raise RedactionError("metadata.model_id is not a supported blob or watt-tool path")


def _run_git(repo_root: Path, *arguments: str) -> bytes:
    try:
        return subprocess.run(
            ["git", "-C", str(repo_root), *arguments],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ).stdout
    except subprocess.CalledProcessError as exc:
        detail = exc.stderr.decode("utf-8", errors="replace").strip()
        raise RedactionError(f"git {' '.join(arguments)} failed: {detail}") from exc


def _repo_root(path: Path) -> Path:
    output = _run_git(path, "rev-parse", "--show-toplevel")
    return Path(output.decode("utf-8").strip()).resolve()


def _source_commit(repo_root: Path, requested: str) -> str:
    output = _run_git(repo_root, "rev-parse", "--verify", f"{requested}^{{commit}}")
    return output.decode("ascii").strip()


def _repo_relative(path: Path, repo_root: Path, label: str) -> str:
    try:
        return path.resolve().relative_to(repo_root).as_posix()
    except ValueError as exc:
        raise RedactionError(f"{label} must be inside the Git worktree") from exc


def _git_show(repo_root: Path, source_commit: str, path: Path, label: str) -> bytes:
    relative = _repo_relative(path, repo_root, label)
    return _run_git(repo_root, "show", f"{source_commit}:{relative}")


def _resolve_evidence(evidence_root: Path, evidence_path: str, run_id: str) -> Path:
    relative = PurePosixPath(evidence_path)
    if relative.is_absolute() or ".." in relative.parts:
        raise RedactionError(f"unsafe evidence path {evidence_path!r}")
    if len(relative.parts) < 3 or relative.parts[0] != "evidence":
        raise RedactionError(
            f"evidence path is not rooted at evidence/: {evidence_path!r}"
        )
    if relative.parts[1] != run_id:
        raise RedactionError(
            f"evidence path run id {relative.parts[1]!r} does not match {run_id!r}"
        )
    root = evidence_root.resolve()
    path_parts = relative.parts[1:] if root.name == "evidence" else relative.parts
    resolved = (root / Path(*path_parts)).resolve()
    try:
        resolved.relative_to(root)
    except ValueError as exc:
        raise RedactionError(
            f"evidence path escapes --evidence-root: {evidence_path!r}"
        ) from exc
    return resolved


def _scenario_map(result: Any, label: str) -> dict[str, tuple[int, dict[str, Any]]]:
    if not isinstance(result, dict) or not isinstance(result.get("scenarios"), list):
        raise RedactionError(f"{label}: result scenarios must be an array")
    if len(result["scenarios"]) != EXPECTED_SCENARIOS:
        raise RedactionError(
            f"{label}: expected {EXPECTED_SCENARIOS} scenarios, "
            f"found {len(result['scenarios'])}"
        )
    scenarios: dict[str, tuple[int, dict[str, Any]]] = {}
    for index, scenario in enumerate(result["scenarios"]):
        if not isinstance(scenario, dict) or not isinstance(scenario.get("id"), str):
            raise RedactionError(f"{label}: scenario {index} has no string id")
        scenario_id = scenario["id"]
        if scenario_id in scenarios:
            raise RedactionError(f"{label}: duplicate scenario id {scenario_id!r}")
        if not isinstance(scenario.get("evidence_path"), str):
            raise RedactionError(
                f"{label}: scenario {scenario_id!r} has no evidence path"
            )
        if not isinstance(scenario.get("evidence_hash"), str):
            raise RedactionError(
                f"{label}: scenario {scenario_id!r} has no evidence hash"
            )
        scenarios[scenario_id] = (index, scenario)
    return scenarios


def _result_identity(result: Any, label: str) -> tuple[str, str]:
    try:
        run_id = result["metadata"]["run_id"]
        model_id = result["metadata"]["model_id"]
    except (KeyError, TypeError) as exc:
        raise RedactionError(
            f"{label}: missing metadata.run_id or metadata.model_id"
        ) from exc
    if not isinstance(run_id, str) or not isinstance(model_id, str):
        raise RedactionError(f"{label}: result identity fields must be strings")
    return run_id, model_id


def _write_temp(destination: Path, data: bytes, mode: int | None = None) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(
        prefix=f".{destination.name}.", suffix=".tmp", dir=destination.parent
    )
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(temporary, mode if mode is not None else 0o644)
        return temporary
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def redact_run(
    result_path: Path | str,
    evidence_root: Path | str,
    ledger_path: Path | str,
    source_commit: str,
    repo_root: Path | str | None = None,
) -> dict[str, Any]:
    result_path = Path(result_path).resolve()
    evidence_root = Path(evidence_root).resolve()
    ledger_path = Path(ledger_path).resolve()
    repository = (
        Path(repo_root).resolve() if repo_root else _repo_root(result_path.parent)
    )
    commit = _source_commit(repository, source_commit)

    result_bytes = result_path.read_bytes()
    source_result_bytes = _git_show(repository, commit, result_path, "result path")
    if result_bytes != source_result_bytes:
        raise RedactionError("result bytes do not match the recorded source commit")
    result = _load_json(result_bytes, str(result_path))
    run_id, old_model = _result_identity(result, str(result_path))
    replacement = _replacement_identifier(old_model)
    scenarios = _scenario_map(result, str(result_path))

    preflight: list[dict[str, Any]] = []
    seen_evidence: set[Path] = set()
    for scenario_id, (result_index, scenario) in scenarios.items():
        evidence_path = scenario["evidence_path"]
        transcript_path = _resolve_evidence(evidence_root, evidence_path, run_id)
        if transcript_path in seen_evidence:
            raise RedactionError(f"duplicate evidence destination {evidence_path!r}")
        seen_evidence.add(transcript_path)
        transcript_bytes = transcript_path.read_bytes()
        source_bytes = _git_show(
            repository, commit, transcript_path, f"evidence path {evidence_path}"
        )
        if transcript_bytes != source_bytes:
            raise RedactionError(
                f"{evidence_path}: transcript bytes do not match the source commit"
            )
        current_hash = _sha256(transcript_bytes)
        if current_hash != scenario["evidence_hash"]:
            raise RedactionError(
                f"{evidence_path}: evidence hash mismatch; aborting the whole run"
            )
        transcript = _load_json(transcript_bytes, evidence_path)
        if transcript.get("run_id") != run_id:
            raise RedactionError(f"{evidence_path}: transcript run_id mismatch")
        if transcript.get("scenario_id") != scenario_id:
            raise RedactionError(f"{evidence_path}: transcript scenario_id mismatch")
        replacement_counts = _permitted_replacement_counts(
            transcript_bytes, transcript, old_model, evidence_path
        )
        preflight.append(
            {
                "scenario_id": scenario_id,
                "result_index": result_index,
                "scenario": scenario,
                "evidence_path": evidence_path,
                "path": transcript_path,
                "bytes": transcript_bytes,
                "document": transcript,
                "replacement_counts": replacement_counts,
                "old_hash": current_hash,
            }
        )

    prepared_transcripts: list[tuple[Path, bytes, int]] = []
    ledger_scenarios: list[dict[str, Any]] = []
    result_replacements: dict[tuple[Any, ...], tuple[str, str]] = {
        ("metadata", "model_id"): (old_model, replacement)
    }
    for item in preflight:
        request_counts = {
            path: count
            for path, count in item["replacement_counts"].items()
            if path[-2:] == ("body", "model")
        }
        response_counts = {
            path: count
            for path, count in item["replacement_counts"].items()
            if path[-2:] == ("response", "body_raw")
        }
        replacements = {
            path: (old_model, replacement) for path in request_counts
        }
        rewritten = _replace_string_values(
            item["bytes"], replacements, item["evidence_path"]
        )
        rewritten = _replace_body_raw_substrings(
            rewritten,
            response_counts,
            old_model,
            replacement,
            item["evidence_path"],
        )
        rewritten_document = _load_json(rewritten, item["evidence_path"])
        if _normalize_transcript(
            item["document"], old_model, item["evidence_path"]
        ) != _normalize_transcript(
            rewritten_document, replacement, item["evidence_path"]
        ):
            raise RedactionError(
                f"{item['evidence_path']}: content changed outside permitted pointers"
            )
        new_hash = _sha256(rewritten)
        result_replacements[("scenarios", item["result_index"], "evidence_hash")] = (
            item["old_hash"],
            new_hash,
        )
        prepared_transcripts.append(
            (item["path"], rewritten, item["path"].stat().st_mode & 0o777)
        )
        ledger_scenarios.append(
            {
                "scenario_id": item["scenario_id"],
                "evidence_path": item["evidence_path"],
                "old_sha256": item["old_hash"],
                "new_sha256": new_hash,
                "permitted_pointers": [
                    _pointer(path) for path in item["replacement_counts"]
                ],
                "replacement_counts": {
                    _pointer(path): count
                    for path, count in item["replacement_counts"].items()
                },
                "replacement_identifier": replacement,
                "mutation_count": sum(item["replacement_counts"].values()),
            }
        )

    rewritten_result = _replace_string_values(
        result_bytes, result_replacements, str(result_path)
    )
    ledger = {
        "ledger_version": LEDGER_VERSION,
        "source_commit": commit,
        "result_path": _repo_relative(result_path, repository, "result path"),
        "run_id": run_id,
        "permitted_pointers": PERMITTED_POINTERS,
        "replacement_identifier": replacement,
        "scenarios": ledger_scenarios,
    }
    ledger_bytes = (json.dumps(ledger, indent=2) + "\n").encode("utf-8")
    if old_model.encode("utf-8") in ledger_bytes or b"/Users/" in ledger_bytes:
        raise RedactionError("ledger would disclose the old absolute path")

    staged: list[tuple[Path, Path]] = []
    try:
        for destination, data, mode in prepared_transcripts:
            staged.append((destination, _write_temp(destination, data, mode)))
        staged.append(
            (
                result_path,
                _write_temp(
                    result_path, rewritten_result, result_path.stat().st_mode & 0o777
                ),
            )
        )
        ledger_mode = (
            ledger_path.stat().st_mode & 0o777 if ledger_path.exists() else None
        )
        staged.append(
            (ledger_path, _write_temp(ledger_path, ledger_bytes, ledger_mode))
        )
        for destination, temporary in staged:
            os.replace(temporary, destination)
    finally:
        for _, temporary in staged:
            temporary.unlink(missing_ok=True)
    return ledger


def _validate_ledger(ledger: Any, label: str) -> tuple[str, str, str, list[Any]]:
    if not isinstance(ledger, dict) or ledger.get("ledger_version") != LEDGER_VERSION:
        raise RedactionError(f"{label}: unsupported ledger version")
    source_commit = ledger.get("source_commit")
    result_path = ledger.get("result_path")
    run_id = ledger.get("run_id")
    scenarios = ledger.get("scenarios")
    if not all(
        isinstance(value, str) for value in (source_commit, result_path, run_id)
    ):
        raise RedactionError(f"{label}: ledger identity fields must be strings")
    if ledger.get("permitted_pointers") != PERMITTED_POINTERS:
        raise RedactionError(f"{label}: ledger permitted pointers are invalid")
    if not isinstance(scenarios, list) or len(scenarios) != EXPECTED_SCENARIOS:
        raise RedactionError(
            f"{label}: ledger must contain {EXPECTED_SCENARIOS} scenarios"
        )
    return source_commit, result_path, run_id, scenarios


def check_ledger(
    ledger_path: Path | str,
    evidence_root: Path | str,
    repo_root: Path | str | None = None,
) -> dict[str, Any]:
    ledger_path = Path(ledger_path).resolve()
    evidence_root = Path(evidence_root).resolve()
    repository = (
        Path(repo_root).resolve() if repo_root else _repo_root(ledger_path.parent)
    )
    ledger_bytes = ledger_path.read_bytes()
    if b"/Users/" in ledger_bytes:
        raise RedactionError(f"{ledger_path}: ledger contains an absolute local path")
    ledger = _load_json(ledger_bytes, str(ledger_path))
    commit, result_relative, run_id, entries = _validate_ledger(
        ledger, str(ledger_path)
    )
    if _source_commit(repository, commit) != commit:
        raise RedactionError(f"{ledger_path}: source_commit is not a full commit id")

    relative = PurePosixPath(result_relative)
    if relative.is_absolute() or ".." in relative.parts:
        raise RedactionError(f"{ledger_path}: unsafe result path")
    result_path = (repository / Path(*relative.parts)).resolve()
    _repo_relative(result_path, repository, "ledger result path")
    old_result_bytes = _git_show(repository, commit, result_path, "ledger result path")
    new_result_bytes = result_path.read_bytes()
    old_result = _load_json(old_result_bytes, f"{result_relative} at {commit}")
    new_result = _load_json(new_result_bytes, result_relative)
    old_run_id, old_model = _result_identity(old_result, f"{result_relative} preimage")
    new_run_id, new_model = _result_identity(new_result, result_relative)
    replacement = ledger.get("replacement_identifier")
    if old_run_id != run_id or new_run_id != run_id:
        raise RedactionError(f"{result_relative}: result run_id does not match ledger")
    if (
        not isinstance(replacement, str)
        or _replacement_identifier(old_model) != replacement
    ):
        raise RedactionError(f"{ledger_path}: replacement identifier is invalid")
    if new_model != replacement:
        raise RedactionError(
            f"{result_relative}: metadata.model_id was not re-anchored"
        )

    old_scenarios = _scenario_map(old_result, f"{result_relative} preimage")
    new_scenarios = _scenario_map(new_result, result_relative)
    if set(old_scenarios) != set(new_scenarios):
        raise RedactionError(f"{result_relative}: scenario set changed")
    entry_map: dict[str, dict[str, Any]] = {}
    result_replacements: dict[tuple[Any, ...], tuple[str, str]] = {
        ("metadata", "model_id"): (old_model, replacement)
    }

    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("scenario_id"), str):
            raise RedactionError(f"{ledger_path}: invalid scenario ledger entry")
        scenario_id = entry["scenario_id"]
        if scenario_id in entry_map or scenario_id not in old_scenarios:
            raise RedactionError(f"{ledger_path}: invalid scenario id {scenario_id!r}")
        entry_map[scenario_id] = entry
        old_index, old_scenario = old_scenarios[scenario_id]
        new_index, new_scenario = new_scenarios[scenario_id]
        if old_index != new_index:
            raise RedactionError(f"{result_relative}: scenario order changed")
        evidence_path = entry.get("evidence_path")
        if (
            evidence_path != old_scenario["evidence_path"]
            or evidence_path != new_scenario["evidence_path"]
        ):
            raise RedactionError(f"{scenario_id}: evidence path changed")
        old_hash = entry.get("old_sha256")
        new_hash = entry.get("new_sha256")
        if old_hash != old_scenario["evidence_hash"]:
            raise RedactionError(
                f"{scenario_id}: old result hash does not match ledger"
            )
        if new_hash != new_scenario["evidence_hash"]:
            raise RedactionError(f"{scenario_id}: new evidence hash link is broken")
        if entry.get("replacement_identifier") != replacement:
            raise RedactionError(f"{scenario_id}: replacement identifier mismatch")

        transcript_path = _resolve_evidence(evidence_root, evidence_path, run_id)
        old_bytes = _git_show(
            repository, commit, transcript_path, f"evidence path {evidence_path}"
        )
        new_bytes = transcript_path.read_bytes()
        if _sha256(old_bytes) != old_hash:
            raise RedactionError(f"{evidence_path}: preimage hash mismatch")
        if _sha256(new_bytes) != new_hash:
            raise RedactionError(f"{evidence_path}: new transcript hash mismatch")
        old_document = _load_json(old_bytes, f"{evidence_path} preimage")
        new_document = _load_json(new_bytes, evidence_path)
        for document, version in ((old_document, "preimage"), (new_document, "new")):
            if document.get("run_id") != run_id:
                raise RedactionError(f"{evidence_path}: {version} run_id mismatch")
            if document.get("scenario_id") != scenario_id:
                raise RedactionError(f"{evidence_path}: {version} scenario_id mismatch")
        old_counts = _permitted_replacement_counts(
            old_bytes, old_document, old_model, f"{evidence_path} preimage"
        )
        new_counts = _permitted_replacement_counts(
            new_bytes, new_document, replacement, evidence_path
        )
        if old_counts != new_counts:
            raise RedactionError(f"{evidence_path}: permitted pointer set changed")
        if b"/Users/" in new_bytes or any(
            any(isinstance(part, str) and "/Users/" in part for part in path)
            or (isinstance(value, str) and "/Users/" in value)
            for path, (_, _, value) in _json_value_spans(
                new_bytes, evidence_path
            ).items()
        ):
            raise RedactionError(
                f"{evidence_path}: absolute path survives in new transcript"
            )
        permitted = [_pointer(path) for path in old_counts]
        if entry.get("permitted_pointers") != permitted:
            raise RedactionError(f"{evidence_path}: ledger pointer list mismatch")
        pointer_counts = {_pointer(path): count for path, count in old_counts.items()}
        if entry.get("replacement_counts") != pointer_counts:
            raise RedactionError(f"{evidence_path}: replacement counts mismatch")
        if entry.get("mutation_count") != sum(old_counts.values()):
            raise RedactionError(f"{evidence_path}: mutation count mismatch")
        if _normalize_transcript(
            old_document, old_model, f"{evidence_path} preimage"
        ) != _normalize_transcript(
            new_document, replacement, evidence_path
        ):
            raise RedactionError(
                f"{evidence_path}: content changed outside permitted pointers"
            )
        expected_bytes = _replace_string_values(
            old_bytes,
            {
                path: (old_model, replacement)
                for path in old_counts
                if path[-2:] == ("body", "model")
            },
            f"{evidence_path} preimage",
        )
        expected_bytes = _replace_body_raw_substrings(
            expected_bytes,
            {
                path: count
                for path, count in old_counts.items()
                if path[-2:] == ("response", "body_raw")
            },
            old_model,
            replacement,
            f"{evidence_path} preimage",
        )
        if expected_bytes != new_bytes:
            raise RedactionError(
                f"{evidence_path}: byte formatting changed outside permitted values"
            )
        result_replacements[("scenarios", old_index, "evidence_hash")] = (
            old_hash,
            new_hash,
        )

    if set(entry_map) != set(old_scenarios):
        raise RedactionError(f"{ledger_path}: ledger does not cover the whole run")
    expected_result = _replace_string_values(
        old_result_bytes, result_replacements, f"{result_relative} preimage"
    )
    if expected_result != new_result_bytes:
        raise RedactionError(
            f"{result_relative}: result changed outside metadata.model_id and evidence_hash"
        )
    return ledger


def check_all_ledgers(
    paths: list[Path | str],
    evidence_root: Path | str,
    repo_root: Path | str | None = None,
) -> list[dict[str, Any]]:
    ledger_paths: set[Path] = set()
    for supplied in paths:
        path = Path(supplied).resolve()
        if path.is_dir():
            ledger_paths.update(path.rglob("*.json"))
        elif path.is_file():
            ledger_paths.add(path)
        else:
            raise RedactionError(f"ledger path does not exist: {path}")
    if not ledger_paths:
        raise RedactionError("--check-all found no ledger files")
    checked = [
        check_ledger(path, evidence_root, repo_root) for path in sorted(ledger_paths)
    ]
    commits = {ledger["source_commit"] for ledger in checked}
    if len(commits) != 1:
        raise RedactionError("all redaction ledgers must share one source_commit")
    return checked


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="verify one ledger")
    mode.add_argument(
        "--check-all",
        nargs="+",
        metavar="PATH",
        help="verify every JSON ledger under the supplied files or directories",
    )
    parser.add_argument("--result", type=Path, help="owning result JSON")
    parser.add_argument(
        "--evidence-root",
        type=Path,
        required=True,
        help="results directory containing evidence/ (results/evidence also accepted)",
    )
    parser.add_argument("--ledger", type=Path, help="per-run ledger JSON")
    parser.add_argument("--source-commit", help="common pre-redaction Git commit")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = _parser()
    arguments = parser.parse_args(argv)
    try:
        if arguments.check_all:
            ledgers = check_all_ledgers(arguments.check_all, arguments.evidence_root)
            print(f"checked {len(ledgers)} redaction ledgers")
        elif arguments.check:
            if arguments.ledger is None:
                parser.error("--check requires --ledger")
            ledger = check_ledger(arguments.ledger, arguments.evidence_root)
            print(f"checked {len(ledger['scenarios'])} transcript re-anchors")
        else:
            if arguments.result is None or arguments.ledger is None:
                parser.error("redaction requires --result and --ledger")
            if arguments.source_commit is None:
                parser.error("redaction requires --source-commit")
            ledger = redact_run(
                arguments.result,
                arguments.evidence_root,
                arguments.ledger,
                arguments.source_commit,
            )
            print(
                f"redacted {len(ledger['scenarios'])} transcripts for {ledger['run_id']}"
            )
        return 0
    except (OSError, RedactionError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
