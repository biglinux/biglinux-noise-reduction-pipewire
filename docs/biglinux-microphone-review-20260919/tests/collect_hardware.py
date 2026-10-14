#!/usr/bin/env python3
"""Observe-only Linux/PipeWire diagnostics. DOES NOT record audio or set volume.
Creates a NEW private directory. May expose device/user/application names in logs.
No install, service restart, profile change or persistent settings modification.
Snapshots before/after perturb scheduling; exclude those edges from A/B results.
"""
from __future__ import annotations
import argparse
import datetime as dt
import json
import math
import os
from pathlib import Path
import platform
import selectors
import signal
import subprocess
import time

UNITS = ['pipewire.service', 'wireplumber.service', 'biglinux-microphone-agc.service',
         'biglinux-microphone-mic.service', 'biglinux-microphone-aec.service',
         'biglinux-microphone-output.service']
MAX_PIDS = 64


def bounded_command(argv: list[str], timeout: float = 6, limit: int = 4*1024*1024) -> dict:
    start = time.monotonic(); chunks = []; size = 0; stopped = None
    try:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, start_new_session=True, env={**os.environ, 'LC_ALL':'C'})
    except OSError as error:
        return {'argv':argv, 'error':str(error)}
    assert child.stdout is not None
    try:
        with selectors.DefaultSelector() as sel:
            sel.register(child.stdout, selectors.EVENT_READ)
            while sel.get_map():
                left = timeout-(time.monotonic()-start)
                if left <= 0:
                    stopped = 'timeout'; break
                for key, _ in sel.select(min(left, .1)):
                    data = os.read(key.fd, 65536)
                    if not data:
                        sel.unregister(key.fileobj); break
                    keep = min(len(data), max(0, limit-size))
                    chunks.append(data[:keep]); size += keep
                    if keep < len(data) or size >= limit:
                        stopped = 'output_limit'; break
                if stopped: break
            if not stopped:
                try: child.wait(timeout=max(.01, timeout-(time.monotonic()-start)))
                except subprocess.TimeoutExpired: stopped = 'timeout'
    finally:
        if child.poll() is None or stopped:
            try: os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError: pass
        try: child.wait(timeout=2)
        except subprocess.TimeoutExpired: stopped = 'unreaped_after_kill'
        child.stdout.close()
    return {'argv':argv, 'returncode':child.returncode, 'elapsed_s':time.monotonic()-start,
            'stopped':stopped, 'output':b''.join(chunks).decode('utf-8', 'replace')}


def parse_stat(text: str) -> dict:
    left = text.find('('); right = text.rfind(')')
    if left < 1 or right <= left: raise ValueError('invalid /proc stat')
    f = text[right+2:].split()
    if len(f) < 22: raise ValueError('short /proc stat')
    return {'pid':int(text[:left].strip()), 'comm':text[left+1:right], 'state':f[0],
            'ticks':int(f[11])+int(f[12]), 'start_ticks':int(f[19]),
            'rss_bytes':int(f[21])*os.sysconf('SC_PAGE_SIZE')}


def proc_sample(pid: int) -> dict:
    root = Path('/proc')/str(pid)
    try:
        value = parse_stat((root/'stat').read_text()[:65536])
        for line in (root/'status').read_text().splitlines():
            key, _, tail = line.partition(':')
            if key in ('voluntary_ctxt_switches', 'nonvoluntary_ctxt_switches'):
                value[key] = int(tail.strip())
        return value
    except (OSError, ValueError) as error:
        return {'pid':pid, 'error':str(error)}


def descendants(roots: set[int]) -> set[int]:
    found = set(roots); pending = [(p, 0) for p in sorted(roots)]
    while pending and len(found) < MAX_PIDS:
        pid, depth = pending.pop()
        if depth >= 4: continue
        try: children = (Path('/proc')/str(pid)/'task'/str(pid)/'children').read_text().split()
        except OSError: continue
        for child in children:
            if len(found) >= MAX_PIDS: break
            if child.isdecimal() and int(child) not in found:
                found.add(int(child)); pending.append((int(child), depth+1))
    return found


def service_pids(output: str) -> set[int]:
    return {int(line.partition('=')[2]) for line in output.splitlines()
            if line.startswith('MainPID=') and line.partition('=')[2].isdecimal()
            and int(line.partition('=')[2]) > 0}


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out', type=Path, required=True, help='new directory; existing path is refused')
    p.add_argument('--seconds', type=float, default=60)
    p.add_argument('--interval', type=float, default=1)
    p.add_argument('--pid', type=int, action='append', default=[], help='extra PID, e.g. GUI; repeatable')
    args = p.parse_args()
    if not (math.isfinite(args.seconds) and 1 <= args.seconds <= 600
            and math.isfinite(args.interval) and .1 <= args.interval <= 10):
        p.error('seconds: 1..600; interval: 0.1..10; both must be finite')
    if len(args.pid) > 32 or any(pid <= 0 for pid in args.pid): p.error('up to 32 positive PIDs')
    if platform.system() != 'Linux': p.error('Linux only')
    os.umask(0o077)
    try: args.out.mkdir(mode=0o700, parents=False, exist_ok=False)
    except OSError as error: p.error(str(error))
    facts = {'utc':dt.datetime.now(dt.timezone.utc).isoformat(), 'kernel':platform.release(),
        'machine':platform.machine(), 'python':platform.python_version(), 'seconds':args.seconds,
        'interval':args.interval, 'clock_ticks_per_s':os.sysconf('SC_CLK_TCK'),
        'scope':'observation only; no recorded PCM; no calibration/volume changes'}
    versions = [bounded_command([x, '--version']) for x in ['pipewire', 'wireplumber', 'wpctl', 'pw-cat']]
    services = bounded_command(['systemctl', '--user', 'show', '--no-pager', '--property=Id',
                               '--property=MainPID', *UNITS])
    roots = service_pids(services.get('output', '')) | set(args.pid)
    facts.update({'versions':versions, 'services':services, 'root_pids':sorted(roots),
        'pid_restart_note':'unit MainPID is sampled at start only; after service restart rerun collector',
        'cpu_scale':'100% means one fully utilized CPU; cumulative utime+stime, not DSP-only'})
    def snapshot(name: str) -> None:
        results = [bounded_command(a) for a in [['wpctl','status','--name'], ['pw-dump'],
            ['wpctl','get-volume','@DEFAULT_AUDIO_SOURCE@'], ['pw-top','--batch-mode','--iterations=2']]]
        (args.out/f'{name}.json').write_text(json.dumps(results, indent=2)+'\n')
    snapshot('before')
    start = time.monotonic(); end = start+args.seconds; previous: dict[int, tuple] = {}
    try:
        with (args.out/'samples.jsonl').open('w') as output:
            while time.monotonic() < end:
                now = time.monotonic(); values = []
                for pid in sorted(descendants(roots))[:MAX_PIDS]:
                    value = proc_sample(pid)
                    if 'ticks' in value:
                        prior = previous.get(pid)
                        if prior and prior[1] == value['start_ticks'] and now > prior[0] and value['ticks'] >= prior[2]:
                            value['cpu_percent_one_core'] = 100*(value['ticks']-prior[2])/(now-prior[0])/facts['clock_ticks_per_s']
                        previous[pid] = (now, value['start_ticks'], value['ticks'])
                    values.append(value)
                output.write(json.dumps({'elapsed_s':now-start, 'processes':values})+'\n'); output.flush()
                time.sleep(max(0, min(args.interval-(time.monotonic()-now), end-time.monotonic())))
    except KeyboardInterrupt:
        facts['interrupted'] = True
    facts['sample_duration_s'] = time.monotonic()-start
    snapshot('after')
    (args.out/'manifest.json').write_text(json.dumps(facts, indent=2)+'\n')
    print(args.out)
    return 0


if __name__ == '__main__': raise SystemExit(main())
