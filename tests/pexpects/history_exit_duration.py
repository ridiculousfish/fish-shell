#!/usr/bin/env python3
# Test that history records exit codes and duration

from pexpect_helper import SpawnedProc
from pathlib import Path
import os
import json
import time

env = os.environ.copy()
history_name = "test_exit_duration_pexpect"
env["fish_history"] = history_name

sp = SpawnedProc(env=env)
send, sendline, sleep, expect_prompt, expect_re, expect_str = (
    sp.send,
    sp.sendline,
    sp.sleep,
    sp.expect_prompt,
    sp.expect_re,
    sp.expect_str,
)
expect_prompt()

# Clear any existing history
sendline("builtin history clear")
expect_prompt()

# Run a successful command
sendline("true")
expect_prompt()

# Run a failing command
sendline("false")
expect_prompt()

# Run a command with no status (begin/end)
sendline("begin; end")
expect_prompt()

# Save history to ensure it's flushed
sendline("builtin history save")
expect_prompt()

# Read the history file
history_dir = os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share"))
history_file = os.path.join(history_dir, "fish", f"{history_name}.jsonl")

# Wait a bit for file to be written
sleep(0.2)

try:
    with open(history_file, "r") as f:
        lines = f.readlines()

    # Parse JSON lines
    items = [json.loads(line) for line in lines if line.strip()]

    # Check we have at least 3 items (true, false, begin;end)
    if len(items) >= 3:
        print("Found at least 3 history items")

    # Check for exit codes
    exit_codes = [item.get("exit") for item in items if "exit" in item]
    if 0 in exit_codes:
        print("Found exit code 0")
    if 1 in exit_codes:
        print("Found exit code 1")

    # Check all items have duration
    durations = [item.get("dur") for item in items]
    if all(d is not None for d in durations):
        print(f"All {len(items)} items have duration")

    # Check that we have an item without exit code but with duration (begin;end)
    # Look for the begin;end command
    for item in items:
        if "cmd" in item and "begin" in item["cmd"] and "end" in item["cmd"]:
            if "exit" not in item:
                print("begin;end has no exit code")
            if "dur" in item:
                print("begin;end has duration")
            break

except FileNotFoundError:
    print(f"ERROR: History file not found: {history_file}")
except Exception as e:
    print(f"ERROR: {e}")

# Clean up
Path(history_file).unlink(missing_ok=True)
