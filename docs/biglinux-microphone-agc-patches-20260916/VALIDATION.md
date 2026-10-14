# Validation record

## Executed here

| Check | Observed result |
|---|---|
| Inspect/extract supplied archive and identify partial source baseline | Completed. Original tree retained separately; no user source overwritten. |
| Check all three unified patches against an untouched copy | Passed; `validation/patch-apply-check.txt`. |
| Actually apply the complete series | Passed; all ten result-file hashes match `validation/patched-files.sha256`. |
| Compare applied output with bundled `reviewed-code/` | Byte-identical partial source trees. |
| Patch whitespace checks | `git diff --check` and `git apply --whitespace=error-all` passed. |
| Original policy test preservation | All 20 original tests retained byte-for-byte as a module; checksum recorded in `validation/legacy-tests.txt`. |
| Independent Python numerical suite | 34 tests passed; `validation/python-tests.txt`. |
| Synthetic scenario matrix | Executed; `validation/synthetic-matrix.csv`. |
| Shell script syntax | `bash -n` passed for apply, core-Rust harness and live observer. |

These are real patch application and numerical checks, not claims that the Rust
code was executed. A separate Python implementation can miss a Rust typing,
module-resolution, dependency, feature-selection or integration defect.

## Written but not executed here

**37 new Rust tests**: 13 extractor + 14 source-controller + 7 policy-extension
cases, and 3 source/capture/configuration tests. The standalone harness should run
54 tests including the 20 existing policy tests. It requires the implementation
agent's already installed compiler. No Rust toolchain was installed or invoked.

Rust compilation, Rust unit tests, rustfmt, Clippy, complete Cargo workspace checks,
actual PipeWire/WirePlumber tests, recordings, listening tests, driver-response
measurements, CPU/xrun profiling, and neural-model accuracy tests remain unexecuted.
The live-observation script was syntax checked but not run on audio hardware.
A prebuilt Python parser-wheel retrieval attempt failed due to network resolution;
no Rust parser validation is claimed either.

## What the numerical evidence means

The tests use deterministic synthetic *level sequences*, not actual PCM recordings
or recognized speech. They exercise normalization, duration-based evidence,
non-decaying primary reference, pauses, steady noise, raw censored noise, offsets,
manual ownership, bounds, quantization, backend caps, failures and rate behavior.

The long-run case follows an established primary with ten minutes of lower-level
speech. The counterexample test explicitly records that identical owner/TV traces
cannot be distinguished. No false-accept/false-reject rate for real speech can be
inferred from synthetic success.

Selected output of the reproducible 200-second, 20 Hz synthetic matrix follows.
Volume commands are coalesced to at most 5 Hz. Values are not measurements of the
AKG or Lenovo devices.

| Scenario | Initial wpctl | Final wpctl | Final peak-reference dBFS | Interpretation |
|---|---:|---:|---:|---|
| Quiet clean source | 1.00 | 1.85 | -15.970 | Within default target comfort band. |
| Very weak source | 1.00 | 3.00 | -19.373 | Full supported gain reached; available gain cannot reach target. |
| Excessive start | 3.00 | 1.58 | -8.081 | Downward protection reaches upper comfort edge. |
| Noisy source | 1.00 | 1.25 | -29.185 | Intentionally noise-limited, not incorrectly declared at target. |
| Quiet start with natural gaps | 0.30 | 1.83 | -16.253 | Censored raw-noise bound permits recovery without a blind startup jump. |
| 150 ms delayed-gain stress | 1.00 | 2.51 | -16.020 | One synthetic delay profile; not proof of real frame/gain synchronization. |

The target is -12 dBFS with a ±4 dB policy comfort zone, not exact equality to the
target on every utterance. Small extra deviation follows command quantization and
write thresholds. The controlled statistic is not instantaneous sample peak, RMS,
LUFS or a professional active-speech measurement. A microphone limited by noise or
maximum gain cannot be forced to target without violating another constraint.

## Scope and remaining integration risks

The source snapshot lacks `Cargo.toml`, some referenced modules, UI/service code
and the actual `gain_safety` implementation. The denoiser sources were inspected
for integration opportunities, not comprehensively audited line by line. No model
weights are present. The original plugin code and DSP chain are unchanged.

Important remaining boundaries: cold-start identity ambiguity; a loud competing
speaker can meet the same evidence; atypically steady or very low-contrast voices
can fail acquisition; a sustained shout can raise a reference that needs explicit
relearning; a restarted source session does not persist an owner reference;
manual hold is conservative; source CLI calls are synchronous and not atomic;
frame/gain epochs are not available; hardware gain response may not match ideal
cubic increments; final processed loudness and limiting need full-chain tests.

These limits are explained in `RATIONALE.md` and tested/reviewed explicitly in
`INTEGRATION.md`. They must not be hidden behind a “perfect automatic microphone”
claim or interpreted as fulfilled universal background-speaker rejection.

## Provenance and reproducibility

`validation/input-archive.sha256` identifies the supplied archive.
`validation/original-snapshot-files.sha256` covers the original partial code tree.
The exact five modified originals are in `validation/base-files.sha256` and the
five new paths in `validation/new-files.txt`. `validation/patched-files.sha256`
covers the ten changed/new resulting source files.

`SHA256SUMS` covers the delivered bundle files except itself. Verify it from the
bundle root with `sha256sum -c SHA256SUMS`. Apply only to the exact baseline or
perform an explicit reviewed rebase in the full repository. The reference Python
suite and scenario generator are included for independent reruns.
