#!/usr/bin/env python3
"""Emit, but never install, one measured ALSA headroom override. Python 3 stdlib."""
import argparse
import json
import sys


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--node-name', required=True, help='Exact ALSA node.name from pw-dump')
    parser.add_argument('--frames', required=True, type=int, help='Measured value, 0..8192')
    args = parser.parse_args()
    if (not args.node_name.startswith(('alsa_input.', 'alsa_output.'))
            or len(args.node_name) > 512 or any(ord(c) < 32 for c in args.node_name)):
        parser.error('expected an exact ALSA node name, not a regular expression')
    if not 0 <= args.frames <= 8192:
        parser.error('frames must be between 0 and 8192')
    # JSON string quoting is accepted by SPA-JSON and prevents config injection.
    print('# Measured per-node override; inspect effective properties after restart.')
    print('# Save as a later user drop-in only after an xrun/latency comparison.')
    print('monitor.alsa.rules = [ { matches = [ { node.name = '
          + json.dumps(args.node_name, ensure_ascii=False) + ' } ]')
    print('  actions = { update-props = { api.alsa.headroom = '
          + str(args.frames) + ' } } } ]')
    return 0


if __name__ == '__main__':
    sys.exit(main())
