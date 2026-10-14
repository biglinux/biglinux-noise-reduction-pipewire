"""Actual OS-pipe tests; no microphone or GTK required."""
import os
import threading
import time
from contextlib import contextmanager

import pytest
from biglinux_microphone.ui.pcm_reader import PCMReader

@contextmanager
def pipe():
    read, write = os.pipe()
    with os.fdopen(read, "rb", buffering=0) as stream:
        try:
            yield stream, write
        finally:
            os.close(write)

def test_partial_writes_are_combined_without_short_read_eof():
    with pipe() as (stream, writer):
        def send():
            os.write(writer, b"ab")
            time.sleep(0.01)
            os.write(writer, b"cd")
        thread = threading.Thread(target=send)
        thread.start()
        with PCMReader(stream, 4) as reader:
            assert reader.read(lambda: False) == b"abcd"
        thread.join(timeout=1)
        assert not thread.is_alive()

def test_alive_writer_without_audio_has_a_deadline():
    with pipe() as (stream, _writer), PCMReader(stream, 4, timeout=0.03) as reader:
        start = time.monotonic()
        with pytest.raises(TimeoutError):
            reader.read(lambda: False)
        assert time.monotonic() - start < 0.5

def test_cancellation_does_not_wait_for_audio():
    with pipe() as (stream, _writer), PCMReader(stream, 4) as reader:
        assert reader.read(lambda: True) is None

def test_mid_read_cancellation_is_bounded():
    stop = threading.Event()
    with pipe() as (stream, _writer), PCMReader(stream, 4) as reader:
        timer = threading.Timer(0.02, stop.set)
        timer.start()
        start = time.monotonic()
        assert reader.read(stop.is_set) is None
        timer.join(timeout=1)
        assert time.monotonic() - start < 0.5

def test_eof_is_not_a_complete_pcm_frame():
    read, write = os.pipe()
    os.write(write, b"a")
    os.close(write)
    with os.fdopen(read, "rb", buffering=0) as stream, PCMReader(stream, 4) as reader:
        assert reader.read(lambda: False) is None

@pytest.mark.parametrize("size,timeout", [(0, 1), (4, 0), (4, float("nan")), (4, float("inf"))])
def test_invalid_config_rejected_before_fd_access(size, timeout):
    with pytest.raises(ValueError):
        PCMReader(None, size, timeout)
