#!/usr/bin/env python3
"""Paired microbenchmark of the supplied and patched loudness implementations.
Not an AGC/PipeWire CPU benchmark. Run on two extracted source trees.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import platform
import statistics
import sys
import time
import numpy as np


def load(root, name):
    path = root / 'app/biglinux_microphone/ui/loudness.py'
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('baseline', type=Path); p.add_argument('patched', type=Path)
    p.add_argument('--rounds', type=int, default=7); p.add_argument('--iterations', type=int, default=1500)
    args = p.parse_args()
    if not (1 <= args.rounds <= 30 and 10 <= args.iterations <= 100000): p.error('invalid benchmark dimensions')
    modules = [load(args.baseline, 'original_loudness'), load(args.patched, 'patched_loudness')]
    n, rate = 4096, 48000
    pcm = .05 * np.sin(2*np.pi*997*np.arange(n)/rate)
    freqs = np.fft.rfftfreq(n, 1/rate); w = np.hanning(n)
    amp = np.abs(np.fft.rfft(pcm*w)) * (2/w.sum()); ms = float(np.mean(pcm*pcm))
    timings = [[], []]; readings = []
    for round_ in range(args.rounds):
        for index in ([0, 1] if round_ % 2 == 0 else [1, 0]):
            meter = modules[index].LoudnessMeter()
            for _ in range(60): meter.push_block(ms, amp, freqs, n/rate)
            start = time.perf_counter_ns()
            for _ in range(args.iterations): result = meter.push_block(ms, amp, freqs, n/rate)
            timings[index].append((time.perf_counter_ns()-start)/args.iterations/1000)
            if round_ == 0: readings.append({'implementation': index, 'steady_tone_reading': result})
    med = [statistics.median(x) for x in timings]
    print(json.dumps({'scope': 'Python LoudnessMeter.push_block ONLY, not full spectrum or AGC CPU',
        'python': platform.python_version(), 'numpy': np.__version__, 'platform': platform.platform(),
        'fft_samples': n, 'sample_rate': rate, 'rounds': args.rounds, 'iterations_per_round': args.iterations,
        'baseline_us_per_call': timings[0], 'patched_us_per_call': timings[1],
        'median_baseline_us': med[0], 'median_patched_us': med[1], 'speedup': med[0]/med[1],
        'reduction_percent': 100*(1-med[1]/med[0]), 'steady_tone_readings': readings,
        'agc_spectral_rate_baseline_hz': 48000/512, 'agc_spectral_rate_patch_hz': 48000/2400,
        'agc_spectral_call_reduction_percent_not_cpu': 100*(1-(48000/2400)/(48000/512)),
        'hardware_audio_test': 'NOT_RUN'}, indent=2))


if __name__ == '__main__': main()
