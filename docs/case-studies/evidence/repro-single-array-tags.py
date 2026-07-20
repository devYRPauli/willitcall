#!/usr/bin/env python3
"""Minimal A/B repro for the single-array-tags empty-response failure.

Everything is byte-identical between variant A and variant B except the
single string tools[0].function.description.

Usage: repro.py <endpoint-base> <model-id> <label> [reps]
  e.g. repro.py http://localhost:11434/v1 qwen2.5:7b-instruct ollama 5
"""
import json
import sys
import urllib.request

BASE = sys.argv[1].rstrip("/")
MODEL = sys.argv[2]
LABEL = sys.argv[3]
REPS = int(sys.argv[4]) if len(sys.argv) > 4 else 5

# Exactly what willitcall sends for scenarios/single-array-tags.toml.
DESCRIPTIONS = {
    "A": "Apply tags to a document.",
    "B": "Apply a list of tags to a document.",
}

PARAMETERS = {
    "type": "object",
    "required": ["document_id", "tags"],
    "properties": {
        "document_id": {"type": "string"},
        "tags": {"type": "array", "items": {"type": "string"}},
    },
}

PROMPT = "Tag document doc-17 with exactly alpha, beta, and gamma in that order."


def payload(description):
    return {
        "model": MODEL,
        "messages": [{"role": "user", "content": PROMPT}],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "tag_document",
                    "description": description,
                    "parameters": PARAMETERS,
                },
            }
        ],
        "tool_choice": "auto",
        "stream": False,
        "temperature": 0.0,
        "top_p": 1.0,
        "seed": 42,
        "max_tokens": 1024,
    }


def call(body):
    req = urllib.request.Request(
        BASE + "/chat/completions",
        data=json.dumps(body).encode(),
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        return resp.status, resp.read().decode("utf-8", "replace")


def classify(raw):
    try:
        doc = json.loads(raw)
    except json.JSONDecodeError:
        return "unparseable", None
    choices = doc.get("choices") or []
    if not choices:
        return "no-choices", doc
    msg = choices[0].get("message") or {}
    calls = msg.get("tool_calls") or []
    content = msg.get("content")
    if calls:
        args = calls[0].get("function", {}).get("arguments")
        return "tool_call", args
    if content:
        return "text_only", content[:200]
    return "EMPTY", json.dumps(
        {
            "finish_reason": choices[0].get("finish_reason"),
            "usage": doc.get("usage"),
            "message": msg,
        }
    )


transcripts = {}
print("endpoint=%s model=%s reps=%d" % (BASE, MODEL, REPS))
for variant, desc in DESCRIPTIONS.items():
    outcomes = []
    for i in range(REPS):
        status, raw = call(payload(desc))
        kind, detail = classify(raw)
        outcomes.append(kind)
        if i == 0:
            transcripts[variant] = {
                "variant": variant,
                "description": desc,
                "request": payload(desc),
                "response_status": status,
                "response_body_raw": raw,
            }
        print("  %s rep%d http=%d -> %s | %s" % (variant, i, status, kind, detail))
    print("%s [%r] => %s" % (variant, desc, outcomes))

out = "/tmp/repro-%s.json" % LABEL
with open(out, "w") as fh:
    json.dump(transcripts, fh, indent=2)
print("transcripts written to %s" % out)
