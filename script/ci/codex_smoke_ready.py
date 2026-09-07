#!/usr/bin/env python3
"""Wait for parser-readable cumulative usage in synthetic smoke transcripts."""

import argparse
from itertools import islice
import json
import os
from pathlib import Path
import time


MAX_FILES = 16
MAX_TAIL_BYTES = 1024 * 1024


def has_complete_usage(sessions: Path, expected_total: int) -> bool:
    for path in islice(sessions.rglob("*.jsonl"), MAX_FILES):
        try:
            with path.open("rb") as stream:
                stream.seek(0, os.SEEK_END)
                stream.seek(max(0, stream.tell() - MAX_TAIL_BYTES))
                content = stream.read(MAX_TAIL_BYTES).decode("utf-8", errors="replace")
        except (OSError, UnicodeError):
            continue
        latest_total = None
        for line in content.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict) or event.get("type") != "event_msg":
                continue
            payload = event.get("payload")
            if not isinstance(payload, dict) or payload.get("type") != "token_count":
                continue
            info = payload.get("info")
            if not isinstance(info, dict):
                continue
            usage = info.get("total_token_usage")
            if isinstance(usage, dict):
                latest_total = usage.get("total_tokens")
        if type(latest_total) is int and latest_total == expected_total:
            return True
    return False


def wait_for_usage(sessions: Path, expected_total: int, timeout: float, pid: int) -> bool:
    deadline = time.monotonic() + timeout
    while True:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        if has_complete_usage(sessions, expected_total):
            return True
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        time.sleep(min(0.05, remaining))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sessions", type=Path)
    parser.add_argument("expected_total", type=int)
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--pid", type=int, required=True)
    args = parser.parse_args()
    if args.timeout < 0 or args.pid <= 0 or args.expected_total < 0:
        parser.error("timeout and expected total must be nonnegative; pid must be positive")
    raise SystemExit(
        0 if wait_for_usage(args.sessions, args.expected_total, args.timeout, args.pid) else 1
    )
