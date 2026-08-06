from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

from tools import redact_local_paths


OLD_MODEL = "/Users/example/.ollama/models/blobs/sha256-" + "a" * 64
NEW_MODEL = "sha256-" + "a" * 64
RUN_ID = "20260805T120000Z-test0001"


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


class Fixture:
    def __init__(
        self,
        bad_hash_index: int | None = None,
        unexpected_path: bool = False,
        streaming: bool = False,
        unsafe_streaming: bool = False,
    ) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.evidence_root = self.root / "results" / "evidence"
        self.run_root = self.evidence_root / RUN_ID
        self.result_path = self.root / "results" / "run.json"
        self.ledger_path = self.root / "docs" / "migrations" / "run-ledger.json"
        self.run_root.mkdir(parents=True)
        scenarios = []
        for index in range(redact_local_paths.EXPECTED_SCENARIOS):
            scenario_id = f"scenario-{index:02d}"
            body_raw = (
                '{"choices": [ {"message": {"content": "keep response bytes"}} ], '
                '"model" : "'
                + OLD_MODEL
                + '", "usage": { "total_tokens": 1 }}'
            )
            if streaming:
                streamed_content = OLD_MODEL if unsafe_streaming else "keep bytes"
                body_raw = (
                    'data: {"choices":[],"created":1,"model":"'
                    + OLD_MODEL
                    + '","object":"chat.completion.chunk"}\r\n\r\n'
                    'data: {"choices":[{"delta":{"content":"'
                    + streamed_content
                    + '"}}], "model" : "'
                    + OLD_MODEL
                    + '"}\n\ndata: [DONE]\r\n'
                )
            transcript = {
                "run_id": RUN_ID,
                "scenario_id": scenario_id,
                "turns": [
                    {
                        "request": {
                            "body": {
                                "model": OLD_MODEL,
                                "prompt": OLD_MODEL if unexpected_path else "keep me",
                            }
                        },
                        "response": {"body_raw": body_raw},
                    }
                ],
                "unrelated": "keep me too",
            }
            transcript_bytes = (json.dumps(transcript, indent=2) + "\n").encode()
            transcript_path = self.run_root / f"{scenario_id}.json"
            transcript_path.write_bytes(transcript_bytes)
            evidence_hash = sha256(transcript_bytes)
            if index == bad_hash_index:
                evidence_hash = "sha256:" + "0" * 64
            scenarios.append(
                {
                    "id": scenario_id,
                    "status": "pass",
                    "evidence_hash": evidence_hash,
                    "evidence_path": f"evidence/{RUN_ID}/{scenario_id}.json",
                }
            )
        result = {
            "schema_version": 2,
            "metadata": {
                "run_id": RUN_ID,
                "timestamp": "2026-08-05T12:00:00Z",
                "model_id": OLD_MODEL,
            },
            "scenarios": scenarios,
            "totals": {"passed": 50, "failed": 0},
        }
        self.result_path.write_text(json.dumps(result, indent=2) + "\n")
        self._git("init", "-q")
        self._git("config", "user.email", "test@example.com")
        self._git("config", "user.name", "Test")
        self._git("add", ".")
        self._git("commit", "-qm", "fixture")
        self.source_commit = self._git("rev-parse", "HEAD").strip()

    def _git(self, *arguments: str) -> str:
        return subprocess.run(
            ["git", "-C", str(self.root), *arguments],
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout

    def redact(self, evidence_root: Path | None = None) -> dict:
        return redact_local_paths.redact_run(
            self.result_path,
            evidence_root or self.evidence_root,
            self.ledger_path,
            self.source_commit,
            self.root,
        )

    def close(self) -> None:
        self.temporary.cleanup()


class RedactLocalPathsTests(unittest.TestCase):
    def test_preflight_aborts_entire_run_on_hash_mismatch(self) -> None:
        fixture = Fixture(bad_hash_index=49)
        self.addCleanup(fixture.close)
        first_path = fixture.run_root / "scenario-00.json"
        result_before = fixture.result_path.read_bytes()
        transcript_before = first_path.read_bytes()

        with self.assertRaisesRegex(redact_local_paths.RedactionError, "hash mismatch"):
            fixture.redact()

        self.assertEqual(fixture.result_path.read_bytes(), result_before)
        self.assertEqual(first_path.read_bytes(), transcript_before)
        self.assertFalse(fixture.ledger_path.exists())

    def test_rewrites_only_permitted_model_locations_and_preserves_bytes(self) -> None:
        fixture = Fixture()
        self.addCleanup(fixture.close)
        transcript_path = fixture.run_root / "scenario-00.json"
        before = transcript_path.read_bytes()

        ledger = fixture.redact()

        after = transcript_path.read_bytes()
        document = json.loads(after)
        self.assertEqual(document["turns"][0]["request"]["body"]["model"], NEW_MODEL)
        before_document = json.loads(before)
        old_body_raw = before_document["turns"][0]["response"]["body_raw"]
        self.assertEqual(
            document["turns"][0]["response"]["body_raw"],
            old_body_raw.replace(OLD_MODEL, NEW_MODEL),
        )
        self.assertEqual(document["unrelated"], "keep me too")
        self.assertEqual(
            after.count(OLD_MODEL.encode()), before.count(OLD_MODEL.encode()) - 2
        )
        self.assertEqual(
            ledger["scenarios"][0]["permitted_pointers"],
            [
                "/turns/0/request/body/model",
                "/turns/0/response/body_raw",
            ],
        )
        self.assertEqual(
            ledger["scenarios"][0]["replacement_counts"],
            {
                "/turns/0/request/body/model": 1,
                "/turns/0/response/body_raw": 1,
            },
        )
        self.assertEqual(ledger["scenarios"][0]["mutation_count"], 2)

    def test_response_echo_is_fully_clean_and_checkable(self) -> None:
        fixture = Fixture()
        self.addCleanup(fixture.close)

        fixture.redact(fixture.evidence_root.parent)

        transcript = (fixture.run_root / "scenario-00.json").read_bytes()
        self.assertNotIn(OLD_MODEL.encode(), transcript)
        self.assertIn(NEW_MODEL.encode(), transcript)
        redact_local_paths.check_ledger(
            fixture.ledger_path, fixture.evidence_root.parent, fixture.root
        )

    def test_check_accepts_v3_owning_result_for_v2_ledger(self) -> None:
        fixture = Fixture()
        self.addCleanup(fixture.close)
        fixture.redact()
        result = json.loads(fixture.result_path.read_bytes())
        model_id = result["metadata"].pop("model_id")
        result["metadata"]["model"] = {"endpoint_id": model_id}
        result["schema_version"] = 3
        fixture.result_path.write_text(json.dumps(result, indent=2) + "\n")

        redact_local_paths.check_ledger(
            fixture.ledger_path, fixture.evidence_root, fixture.root
        )

    def test_streaming_response_is_fully_clean_and_preserves_sse_bytes(self) -> None:
        fixture = Fixture(streaming=True)
        self.addCleanup(fixture.close)
        transcript_path = fixture.run_root / "scenario-00.json"
        before_body = json.loads(transcript_path.read_bytes())["turns"][0]["response"][
            "body_raw"
        ].encode()

        ledger = fixture.redact()

        after_body = json.loads(transcript_path.read_bytes())["turns"][0]["response"][
            "body_raw"
        ].encode()
        self.assertEqual(
            after_body, before_body.replace(OLD_MODEL.encode(), NEW_MODEL.encode())
        )
        self.assertEqual(after_body.count(b"data: "), before_body.count(b"data: "))
        self.assertTrue(after_body.endswith(b"data: [DONE]\r\n"))
        self.assertNotIn(OLD_MODEL.encode(), after_body)
        self.assertEqual(
            ledger["scenarios"][0]["replacement_counts"][
                "/turns/0/response/body_raw"
            ],
            2,
        )
        redact_local_paths.check_ledger(
            fixture.ledger_path, fixture.evidence_root, fixture.root
        )

    def test_streaming_response_aborts_on_path_outside_model_value(self) -> None:
        fixture = Fixture(streaming=True, unsafe_streaming=True)
        self.addCleanup(fixture.close)
        transcript_path = fixture.run_root / "scenario-00.json"
        result_before = fixture.result_path.read_bytes()
        transcript_before = transcript_path.read_bytes()

        with self.assertRaisesRegex(
            redact_local_paths.RedactionError, 'not a complete "model" string value'
        ):
            fixture.redact()

        self.assertEqual(fixture.result_path.read_bytes(), result_before)
        self.assertEqual(transcript_path.read_bytes(), transcript_before)
        self.assertFalse(fixture.ledger_path.exists())

    def test_aborts_when_path_is_not_a_model_value(self) -> None:
        fixture = Fixture(unexpected_path=True)
        self.addCleanup(fixture.close)
        transcript_path = fixture.run_root / "scenario-00.json"
        result_before = fixture.result_path.read_bytes()
        transcript_before = transcript_path.read_bytes()

        with self.assertRaisesRegex(
            redact_local_paths.RedactionError, "not at a permitted"
        ):
            fixture.redact()

        self.assertEqual(fixture.result_path.read_bytes(), result_before)
        self.assertEqual(transcript_path.read_bytes(), transcript_before)
        self.assertFalse(fixture.ledger_path.exists())

    def test_rehashes_transcripts_and_updates_only_result_fields(self) -> None:
        fixture = Fixture()
        self.addCleanup(fixture.close)
        old_result = json.loads(fixture.result_path.read_bytes())

        ledger = fixture.redact()

        new_result = json.loads(fixture.result_path.read_bytes())
        self.assertEqual(new_result["metadata"]["model_id"], NEW_MODEL)
        for old_scenario, new_scenario in zip(
            old_result["scenarios"], new_result["scenarios"], strict=True
        ):
            transcript_path = (
                fixture.evidence_root / RUN_ID / (new_scenario["id"] + ".json")
            )
            self.assertEqual(
                new_scenario["evidence_hash"], sha256(transcript_path.read_bytes())
            )
            old_scenario["evidence_hash"] = new_scenario["evidence_hash"]
        old_result["metadata"]["model_id"] = NEW_MODEL
        self.assertEqual(new_result, old_result)
        self.assertNotIn(OLD_MODEL, fixture.ledger_path.read_text())
        self.assertEqual(len(ledger["scenarios"]), 50)
        redact_local_paths.check_ledger(
            fixture.ledger_path, fixture.evidence_root, fixture.root
        )

    def test_check_detects_tamper_outside_permitted_pointer(self) -> None:
        fixture = Fixture()
        self.addCleanup(fixture.close)
        fixture.redact()
        transcript_path = fixture.run_root / "scenario-00.json"
        transcript = json.loads(transcript_path.read_bytes())
        transcript["turns"][0]["request"]["body"]["prompt"] = "tampered"
        transcript_path.write_text(json.dumps(transcript, indent=2) + "\n")
        tampered_hash = sha256(transcript_path.read_bytes())

        result = json.loads(fixture.result_path.read_bytes())
        result["scenarios"][0]["evidence_hash"] = tampered_hash
        fixture.result_path.write_text(json.dumps(result, indent=2) + "\n")
        ledger = json.loads(fixture.ledger_path.read_bytes())
        ledger["scenarios"][0]["new_sha256"] = tampered_hash
        fixture.ledger_path.write_text(json.dumps(ledger, indent=2) + "\n")

        with self.assertRaisesRegex(
            redact_local_paths.RedactionError, "changed outside"
        ):
            redact_local_paths.check_ledger(
                fixture.ledger_path, fixture.evidence_root, fixture.root
            )


if __name__ == "__main__":
    unittest.main()
