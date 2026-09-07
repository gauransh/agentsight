#!/usr/bin/env python3
"""The smoke must wait for parser-readable cumulative usage, not a substring."""

import json
import os
from pathlib import Path
import tempfile
import unittest

from codex_smoke_ready import MAX_TAIL_BYTES, has_complete_usage, wait_for_usage


class CodexSmokeReadyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.sessions = Path(self.directory.name)
        self.rollout = self.sessions / "session.jsonl"

    def event(self, total=15):
        return {
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": total - 4,
                        "output_tokens": 4,
                        "total_tokens": total,
                    }
                },
            },
        }

    def test_waits_for_the_complete_json_record(self):
        record = json.dumps(self.event(), separators=(",", ":"))
        partial = record[: record.index('"total_tokens":15') + len('"total_tokens":15')]
        self.rollout.write_text(partial, encoding="utf-8")
        self.assertFalse(has_complete_usage(self.sessions, 15))
        with self.rollout.open("a", encoding="utf-8") as stream:
            stream.write(record[len(partial) :] + "\n")
        self.assertTrue(has_complete_usage(self.sessions, 15))

    def test_accepts_valid_usage_with_whitespace(self):
        self.rollout.write_text(json.dumps(self.event()) + "\n", encoding="utf-8")
        self.assertTrue(has_complete_usage(self.sessions, 15))

    def test_ignores_unrelated_and_non_cumulative_totals(self):
        event = self.event(30)
        event["payload"]["info"]["last_token_usage"] = {"total_tokens": 15}
        self.rollout.write_text(
            json.dumps({"type": "response_item", "total_tokens": 15}, separators=(",", ":"))
            + "\n"
            + json.dumps(event, separators=(",", ":"))
            + "\n",
            encoding="utf-8",
        )
        self.assertFalse(has_complete_usage(self.sessions, 15))

    def test_uses_latest_cumulative_record(self):
        self.rollout.write_text(
            json.dumps(self.event(), separators=(",", ":"))
            + "\n"
            + json.dumps(self.event(30), separators=(",", ":"))
            + "\n",
            encoding="utf-8",
        )
        self.assertFalse(has_complete_usage(self.sessions, 15))

    def test_ignores_malformed_records_and_missing_directory(self):
        self.assertFalse(has_complete_usage(self.sessions / "missing", 15))
        self.rollout.write_text(
            "not json\n[]\n" + json.dumps(self.event()) + "\n", encoding="utf-8"
        )
        self.assertTrue(has_complete_usage(self.sessions, 15))

    def test_reads_a_bounded_tail_and_requires_an_integer_total(self):
        self.rollout.write_text(
            "x" * (MAX_TAIL_BYTES + 1) + "\n" + json.dumps(self.event()) + "\n",
            encoding="utf-8",
        )
        self.assertTrue(has_complete_usage(self.sessions, 15))
        for total in [15.0, "15", True]:
            event = self.event()
            event["payload"]["info"]["total_token_usage"]["total_tokens"] = total
            self.rollout.write_text(json.dumps(event) + "\n", encoding="utf-8")
            self.assertFalse(has_complete_usage(self.sessions, 15))
        event = self.event()
        event["payload"]["info"]["total_token_usage"]["total_tokens"] = True
        self.rollout.write_text(json.dumps(event) + "\n", encoding="utf-8")
        self.assertFalse(has_complete_usage(self.sessions, 1))

    def test_deadline_does_not_wait_for_missing_usage(self):
        self.assertFalse(wait_for_usage(self.sessions, 15, 0, os.getpid()))


if __name__ == "__main__":
    unittest.main()
