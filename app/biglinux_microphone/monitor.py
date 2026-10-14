"""Monitor the filtered microphone with PipeWire's two-second delay filter.

The UI runs this package-owned file with ``python -I /absolute/path/monitor.py``;
no import from versioned site-packages is required. The UI owns this process;
PipeWire handles capture/playback clock differences and the audio delay.
"""

from __future__ import annotations

import ctypes
import os
import shutil
import signal
import subprocess
import sys


def _die_with_parent() -> None:
    # The single-threaded monitor owns this child, so a forced monitor shutdown
    # also stops playback. The GTK process spawns us from a short-lived thread;
    # binding directly to that thread would kill playback when startup returns.
    libc = ctypes.CDLL("libc.so.6", use_errno=True)
    if libc.prctl(1, signal.SIGKILL, 0, 0, 0) != 0:  # PR_SET_PDEATHSIG
        raise OSError(ctypes.get_errno(), "Cannot bind monitor child to its parent")


def main() -> int:
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    loader = shutil.which("biglinux-microphone-pwloader")
    pw_cat = shutil.which("pw-cat")
    if loader is None or pw_cat is None:
        print("biglinux-microphone-pwloader or pw-cat not found", file=sys.stderr)
        return 1

    # The built-in delay starts with silence; target.delay.sec in PipeWire
    # 1.6.8 reads uninitialised history during startup. Keep spare capacity
    # beyond the requested delay so vectorised blocks cannot overlap the read head.
    args = b"""{
        audio.rate = 48000
        audio.position = [ MONO ]
        filter.graph = {
            nodes = [ {
                type = builtin name = delay label = delay
                config = { max-delay = 4.0 }
                control = { "Delay (s)" = 2.0 }
            } ]
        }
        capture.props = {
            node.name = biglinux-mic-monitor-capture
            media.class = Stream/Input/Audio
            node.latency = 1024/48000
        }
        playback.props = {
            node.name = biglinux-mic-monitor
            media.class = Stream/Output/Audio
            node.latency = 1024/48000
        }
    }"""
    # The echo canceller and the microphone filter capture their sources
    # passively, so the microphone runs only while a reader without a driver of
    # its own pulls on it: an application recording, or the home page's meter.
    # The delay's playback side makes the output device this graph's driver, and
    # PipeWire then suspends the microphone, so the return went silent as soon as
    # the meter stopped. A second reader, routed like the delay's capture and
    # discarding what it hears, keeps the microphone pulled.
    reader = [
        pw_cat,
        "--record",
        "--raw",
        "--rate=48000",
        "--channels=1",
        "--format=s16",
        "-P",
        "{ node.name = biglinux-mic-monitor-reader }",
        "-",
    ]
    children: list[subprocess.Popen] = []
    descriptor = os.memfd_create("biglinux-mic-monitor")
    os.write(descriptor, args)
    try:
        # This standalone process has one thread, making preexec_fn safe here.
        children.append(
            subprocess.Popen(
                [loader, "libpipewire-module-filter-chain", f"/proc/self/fd/{descriptor}"],
                pass_fds=(descriptor,),
                preexec_fn=_die_with_parent,  # noqa: PLW1509
            )
        )
        children.append(
            subprocess.Popen(
                reader,
                stdout=subprocess.DEVNULL,
                preexec_fn=_die_with_parent,  # noqa: PLW1509
            )
        )
    except OSError:
        _stop(children)
        raise
    finally:
        os.close(descriptor)
    try:
        return children[0].wait()
    finally:
        _stop(children)


def _stop(children: list[subprocess.Popen]) -> None:
    for child in children:
        if child.poll() is None:
            child.terminate()
    for child in children:
        try:
            child.wait(timeout=1)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


if __name__ == "__main__":
    sys.exit(main())
