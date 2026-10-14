#!/usr/bin/env python3
"""Run one process group with a lock, deadline, memory/disk guards and disk logs."""
from __future__ import annotations

import argparse
import contextlib
import fcntl
import json
import math
import os
import shutil
import signal
import subprocess
import time
from pathlib import Path

CGROUP = Path("/sys/fs/cgroup")


def memory_current() -> int:
    try:
        return int((CGROUP / "memory.current").read_text())
    except (OSError, ValueError):
        return 0


def memory_limit() -> int | None:
    try:
        return int((CGROUP / "memory.max").read_text())
    except (OSError, ValueError):
        return None  # 'max' is unlimited; never invent an observed limit.


def memory_events() -> dict[str, int]:
    try:
        return {key: int(value) for key, value in
                (line.split() for line in (CGROUP / "memory.events").read_text().splitlines())}
    except (OSError, ValueError):
        return {}


def terminate_group(child: subprocess.Popen) -> None:
    with contextlib.suppress(ProcessLookupError):
        os.killpg(child.pid, signal.SIGTERM)
    try:
        child.wait(timeout=5)
    except subprocess.TimeoutExpired:
        pass
    # Descendants can survive even after their parent has exited.
    with contextlib.suppress(ProcessLookupError):
        os.killpg(child.pid, signal.SIGKILL)
    child.wait(timeout=5)


def execute(command: list[str], log: Path, lock_path: Path, seconds: float,
            fraction: float = .80, min_free_mib: int = 512) -> dict:
    started = time.monotonic()
    before = memory_events()
    limit = memory_limit()
    ceiling = int(limit * fraction) if limit is not None else None
    result = {"command": command, "rc": 125, "classification": "ENV", "reason": "preflight",
              "memory_limit": limit, "memory_ceiling": ceiling, "peak_memory": memory_current()}
    child = None
    lock = None
    log.parent.mkdir(parents=True, exist_ok=True)
    try:
        lock = lock_path.open("a")
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            result["reason"] = "another validation is active"
            return result
        if ceiling is not None and memory_current() >= ceiling:
            result["reason"] = "insufficient cgroup headroom"
            return result
        if shutil.disk_usage(log.parent).free < min_free_mib * 1024**2:
            result["reason"] = "insufficient disk space"
            return result
        with log.open("wb") as output:
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=output,
                                     stderr=subprocess.STDOUT, start_new_session=True)
            while child.poll() is None:
                current = memory_current()
                result["peak_memory"] = max(result["peak_memory"], current)
                if ceiling is not None and current >= ceiling:
                    result["reason"] = "memory guard"
                    break
                if shutil.disk_usage(log.parent).free < min_free_mib * 1024**2:
                    result["reason"] = "disk guard"
                    break
                if time.monotonic() - started >= seconds:
                    result.update(rc=124, classification="UNKNOWN", reason="deadline exceeded")
                    break
                time.sleep(.1)
            else:
                rc = child.returncode
                result.update(rc=rc if rc >= 0 else 128 - rc,
                              classification="PASS" if rc == 0 else "UNKNOWN",
                              reason="completed" if rc == 0 else "child failed")
    except FileNotFoundError as error:
        result.update(rc=127, classification="DEPENDENCY", reason=str(error))
    except (OSError, subprocess.SubprocessError) as error:
        result.update(rc=125, classification="ENV", reason=str(error))
    except KeyboardInterrupt:
        result.update(rc=130, classification="ENV", reason="interrupted")
    finally:
        if child is not None:
            try:
                terminate_group(child)
            except (OSError, subprocess.SubprocessError) as error:
                result.update(rc=125, classification="ENV", reason=f"cleanup failed: {error}")
        after = memory_events()
        result["event_delta"] = {key: value - before.get(key, 0) for key, value in after.items()}
        if any(result["event_delta"].get(key, 0) > 0 for key in ("max", "oom", "oom_kill")):
            result.update(rc=125, classification="ENV", reason="cgroup pressure increased")
        result["seconds"] = round(time.monotonic() - started, 3)
        log.with_suffix(log.suffix + ".json").write_text(json.dumps(result, indent=2) + "\n")
        if lock is not None:
            lock.close()
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--lock", type=Path,
                        default=Path(__file__).resolve().parents[1] / ".release-check.lock")
    parser.add_argument("--seconds", type=float, default=300)
    parser.add_argument("--fraction", type=float, default=.80)
    parser.add_argument("--min-free-mib", type=int, default=512)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command or not math.isfinite(args.seconds) or args.seconds <= 0:
        parser.error("a command and positive finite timeout are required")
    if not math.isfinite(args.fraction) or not .1 <= args.fraction <= .9 or args.min_free_mib < 0:
        parser.error("fraction must be 0.1..0.9 and disk reserve nonnegative")
    result = execute(command, args.log, args.lock, args.seconds, args.fraction, args.min_free_mib)
    print(f"{args.log.stem}: {result['classification']} rc={result['rc']}; "
          f"{result['reason']}; {result['seconds']}s; peak={result['peak_memory']}", flush=True)
    return result["rc"]


if __name__ == "__main__":
    raise SystemExit(main())
