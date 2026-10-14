#!/usr/bin/env python3
"""Write a reproducible synthetic matrix; values are not microphone measurements."""
import csv
import sys
from collections import deque
from reference_model import Controller, background, tick, voice

out = csv.writer(sys.stdout)
out.writerow(['scenario', 'input_peak_at_unity_dbfs', 'noise_at_unity_dbfs',
              'initial_wpctl', 'final_wpctl', 'final_reference_peak_dbfs',
              'learned_reference_at_base_dbfs', 'write_count'])
cases = [('quiet_clean', -32, -95, 1, 0), ('very_weak', -48, -105, 1, 0),
         ('high_start', -20, -95, 3, 0), ('noise_limited', -35, -60, 1, 0),
         ('quiet_start_with_gaps', -32, -95, .3, 0),
         ('150ms_delayed_gain', -40, -100, 1, 3)]
for name, high, floor, volume, delay in cases:
    c = Controller(volume)
    background(c, floor)
    past = deque([c.applied] * (delay + 1), maxlen=delay + 1)
    writes = 0
    for i in range(4000):
        peak = floor if name == 'quiet_start_with_gaps' and i % 80 > 65 else voice(i, high)
        result = tick(c, peak, write=i % 4 == 0,
                      measured_override=peak + past[0] if delay else None)
        writes += int(i % 4 == 0 and result['request'] is not None)
        past.append(c.applied)
    out.writerow([name, high, floor, volume, f'{c.volume:.2f}',
                  f'{high+c.applied:.3f}', f'{c.fx.primary:.3f}', writes])
