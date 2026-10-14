# BigLinux microphone — AGC review and patch bundle

**Basis:** `biglinux-agc-external-agent-20260916-2341.tar.gz`, specifically its
`code/` snapshot. This is a statically reviewed, numerically tested patch proposal,
not a compiled or hardware-certified release. No Rust toolchain was installed or
executed during this review.

## Apply and test

Use a clean branch in the complete project. The directory passed below must contain
`src/agc.rs`; for the supplied archive that directory is `code/`.

```sh
/path/to/biglinux-agc-review/apply.sh /path/to/project --check
/path/to/biglinux-agc-review/apply.sh /path/to/project
/path/to/biglinux-agc-review/tests/run-core-rust.sh /path/to/project
```

The apply script verifies the exact five modified input files, refuses to overwrite
new files, checks the complete patch set, and verifies the resulting ten files.
It does not install dependencies, run services, commit, or change other files.
A different starting revision requires a deliberate manual rebase; do not bypass
the hash failure and assume the resulting patch was reviewed here.

In the complete workspace, use its existing toolchain and run formatting, the
full test suite, and Clippy. `INTEGRATION.md` gives the commands and live checklist.
The standalone Rust harness needs only `std`: it loads the actual patched policy,
extractor and adapter plus their tests, without PipeWire, GTK, models or hardware.
It should discover **54 tests: 20 preserved policy tests + 34 new core tests**.
There are **3 additional new integration/configuration tests**, for 37 new Rust
tests in total. These Rust tests have been written, not executed in this environment.

The independent numerical tests are executable here and were run:

```sh
cd tests
python3 test_reference.py
python3 matrix.py
```

**34 Python tests passed.** They test synthetic level sequences and controller
arithmetic, not real speech-recognition accuracy, Rust execution or driver behavior.

## What the three patches do

| Patch | Scope |
|---|---|
| `0001` | Preserve volume + mute readback; explicitly selected meters cannot silently fall back or move. |
| `0002` | Normalize before history; sustained relative evidence; non-decaying primary reference; separate downward peak protection; no blind startup boost; manual ownership; source binding; fresh-frame handling; actuator reconciliation, quantization and coalesced writes. |
| `0003` | Use a -12 dBFS default robust peak-reference target, with -18..-6 dBFS range. Retain the full 0..300% capture-volume range. |

`reviewed-code/` is the supplied **partial source snapshot** with all three patches
applied, for comparison; it is not a complete application distribution. The archive
does not include its Cargo manifest, several referenced modules, UI implementation,
service definitions, or model weights. Do not replace a full repository with it.

## Essential limitation

This fixes the long-run **quieter-than-an-established-primary** takeover mechanism.
It does not turn level measurements into speaker identity. At cold start, a TV or
another person can produce the same evidence as the owner and can still be enrolled.
A retained level reference is not voice biometrics. The new tests include that
counterexample explicitly. See `RATIONALE.md` and the guided-check UI contract in
`INTEGRATION.md`; do not advertise universal TV/background-speaker rejection.

## Contents

`RATIONALE.md` explains the model, constants and acoustic constraints.
`INTEGRATION.md` contains release gates, live checks and the small UI contract.
`SOURCES.md` records primary documentation/research and what was actually verified.
`VALIDATION.md` distinguishes executed checks from outstanding validation.
`validation/` contains input/output hashes and observed numerical results.
