#!/usr/bin/env python3
"""
Asynchronous utilities for BigLinux Microphone Settings.

Provides helpers for async operations, threading, and rate limiting.
"""

from __future__ import annotations

import functools
import logging
import threading
import time
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from typing import Any, ParamSpec, TypeVar

from gi.repository import GLib

logger = logging.getLogger(__name__)

P = ParamSpec("P")
T = TypeVar("T")

# Global thread pool for background operations
_executor = ThreadPoolExecutor(max_workers=4, thread_name_prefix="biglinux-mic-")


def run_async(
    func: Callable[P, T],
    *args: P.args,
    callback: Callable[[T], None] | None = None,
    error_callback: Callable[[Exception], None] | None = None,
    **kwargs: P.kwargs,
) -> None:
    """
    Run a function asynchronously in a thread pool.

    The callback is invoked on the main GTK thread when complete.

    Args:
        func: Function to run
        *args: Positional arguments
        callback: Called with result on main thread
        error_callback: Called with exception on main thread
        **kwargs: Keyword arguments
    """

    def task() -> None:
        try:
            result = func(*args, **kwargs)
            if callback:
                GLib.idle_add(callback, result)
        except Exception as e:
            logger.exception("Async task failed")
            if error_callback:
                GLib.idle_add(error_callback, e)

    _executor.submit(task)


def run_in_thread(
    func: Callable[P, T] | None = None,
    *,
    on_complete: Callable[[T], None] | None = None,
    on_error: Callable[[Exception], None] | None = None,
) -> Callable[[Callable[P, T]], Callable[P, None]]:
    """
    Decorator to run a function in a background thread.

    Usage:
        @run_in_thread(on_complete=handle_result)
        def slow_operation():
            return compute_something()

    Args:
        func: Function to decorate
        on_complete: Callback with result
        on_error: Callback with exception
    """

    def decorator(f: Callable[P, T]) -> Callable[P, None]:
        @functools.wraps(f)
        def wrapper(*args: P.args, **kwargs: P.kwargs) -> None:
            run_async(
                f,
                *args,
                callback=on_complete,
                error_callback=on_error,
                **kwargs,
            )

        return wrapper

    if func is not None:
        return decorator(func)
    return decorator


class Debouncer:
    """
    Debounce function calls.

    Only executes the function after no calls for the specified delay.
    """

    def __init__(self, delay_ms: int = 100) -> None:
        """
        Initialize debouncer.

        Args:
            delay_ms: Delay in milliseconds
        """
        self._delay_ms = delay_ms
        self._timer_id: int | None = None
        self._pending_call: tuple[Callable, tuple, dict] | None = None

    def call(
        self,
        func: Callable[..., Any],
        *args: object,
        **kwargs: object,
    ) -> None:
        """
        Schedule a debounced call.

        Args:
            func: Function to call
            *args: Positional arguments
            **kwargs: Keyword arguments
        """
        # Cancel pending timer
        if self._timer_id is not None:
            GLib.source_remove(self._timer_id)

        # Store pending call
        self._pending_call = (func, args, kwargs)

        # Schedule new timer
        self._timer_id = GLib.timeout_add(
            self._delay_ms,
            self._execute,
        )

    def _execute(self) -> bool:
        """Execute the pending call."""
        self._timer_id = None
        if self._pending_call:
            func, args, kwargs = self._pending_call
            self._pending_call = None
            func(*args, **kwargs)
        return False

    def cancel(self) -> None:
        """Cancel pending call."""
        if self._timer_id is not None:
            GLib.source_remove(self._timer_id)
            self._timer_id = None
        self._pending_call = None


def debounce(
    delay_ms: int = 100,
) -> Callable[[Callable[P, T]], Callable[P, None]]:
    """
    Decorator to debounce function calls.

    Usage:
        @debounce(delay_ms=200)
        def save_settings(settings):
            write_to_disk(settings)

    Args:
        delay_ms: Delay in milliseconds
    """

    def decorator(func: Callable[P, T]) -> Callable[P, None]:
        debouncer = Debouncer(delay_ms)

        @functools.wraps(func)
        def wrapper(*args: P.args, **kwargs: P.kwargs) -> None:
            debouncer.call(func, *args, **kwargs)

        wrapper.cancel = debouncer.cancel  # type: ignore
        return wrapper

    return decorator


class Throttler:
    """
    Throttle function calls.

    Limits function execution to at most once per specified interval.
    """

    def __init__(self, interval_ms: int = 100) -> None:
        """
        Initialize throttler.

        Args:
            interval_ms: Minimum interval between calls
        """
        self._interval_ms = interval_ms
        self._last_call: float = 0
        self._pending_call: tuple[Callable, tuple, dict] | None = None
        self._timer_id: int | None = None

    def call(
        self,
        func: Callable[..., Any],
        *args: object,
        **kwargs: object,
    ) -> None:
        """
        Make a throttled call.

        Args:
            func: Function to call
            *args: Positional arguments
            **kwargs: Keyword arguments
        """
        now = time.monotonic() * 1000  # Convert to ms
        elapsed = now - self._last_call

        if elapsed >= self._interval_ms:
            # Execute immediately
            self._last_call = now
            func(*args, **kwargs)
        else:
            # Schedule for later
            self._pending_call = (func, args, kwargs)
            if self._timer_id is None:
                delay = int(self._interval_ms - elapsed)
                self._timer_id = GLib.timeout_add(delay, self._execute)

    def _execute(self) -> bool:
        """Execute pending call."""
        self._timer_id = None
        self._last_call = time.monotonic() * 1000

        if self._pending_call:
            func, args, kwargs = self._pending_call
            self._pending_call = None
            func(*args, **kwargs)

        return False

    def cancel(self) -> None:
        """Cancel pending call."""
        if self._timer_id is not None:
            GLib.source_remove(self._timer_id)
            self._timer_id = None
        self._pending_call = None


def throttle(
    interval_ms: int = 100,
) -> Callable[[Callable[P, T]], Callable[P, None]]:
    """
    Decorator to throttle function calls.

    Usage:
        @throttle(interval_ms=50)
        def update_ui(value):
            widget.set_value(value)

    Args:
        interval_ms: Minimum interval between calls
    """

    def decorator(func: Callable[P, T]) -> Callable[P, None]:
        throttler = Throttler(interval_ms)

        @functools.wraps(func)
        def wrapper(*args: P.args, **kwargs: P.kwargs) -> None:
            throttler.call(func, *args, **kwargs)

        wrapper.cancel = throttler.cancel  # type: ignore
        return wrapper

    return decorator


class AsyncTaskQueue:
    """
    Queue for sequential async task execution.

    Ensures tasks are executed one at a time in order.
    """

    def __init__(self) -> None:
        """Initialize the task queue."""
        self._queue: list[tuple[Callable, tuple, dict, Callable | None]] = []
        self._is_running = False
        self._lock = threading.Lock()

    def add(
        self,
        func: Callable[..., Any],
        *args: object,
        callback: Callable[[Any], None] | None = None,
        **kwargs: object,
    ) -> None:
        """
        Add a task to the queue.

        Args:
            func: Function to execute
            *args: Positional arguments
            callback: Called with result
            **kwargs: Keyword arguments
        """
        with self._lock:
            self._queue.append((func, args, kwargs, callback))
            if not self._is_running:
                self._is_running = True
                self._process_next()

    def _process_next(self) -> None:
        """Process the next task in queue."""
        with self._lock:
            if not self._queue:
                self._is_running = False
                return

            func, args, kwargs, callback = self._queue.pop(0)

        def task() -> None:
            try:
                result = func(*args, **kwargs)
                if callback:
                    GLib.idle_add(callback, result)
            except Exception:
                logger.exception("Task queue error")
            finally:
                GLib.idle_add(self._process_next)

        _executor.submit(task)

    def clear(self) -> None:
        """Clear all pending tasks."""
        with self._lock:
            self._queue.clear()
