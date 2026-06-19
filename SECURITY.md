# Security Policy — biglinux-microphone

## Supported versions

| Version | Supported |
|---------|-----------|
| latest stable (main) | ✅ |
| previous minor       | ✅ (security patches) |
| older                | ❌ |

## Reporting a vulnerability

**Do NOT open public issues for security bugs.**

- **Preferred:** GitHub Security Advisory — https://github.com/biglinux/biglinux-noise-reduction-pipewire/security/advisories/new
- **Backup:** email `security@biglinux.com.br` (PGP key on keys.openpgp.org, fingerprint TBD)

Include: affected version, reproduction steps, impact, suggested fix (optional).

## Response SLA

| Severity | First response | Patch target |
|----------|---------------:|-------------:|
| CRITICAL (RCE, privilege escalation, data loss) | 24h | 72h |
| HIGH (auth bypass, sandbox escape)              | 72h | 7d  |
| MEDIUM (info leak, DoS)                         | 7d  | 30d |
| LOW (defense-in-depth)                          | 14d | next minor |

## In scope

- **Subprocess safety**: `pw-cli`/`wpctl`/`journalctl` invoked via argv arrays,
  never a shell; node IDs numeric (`services/pipewire/live.rs`). No interpolation.
- **`unsafe`/RT boundary**: libc real-time setup in `pwloader.rs` and GTK/FFI in
  `worker.rs`/`window.rs` — each block carries a `SAFETY` note and is the only UB
  surface.
- **Atomic config writes**: `settings.json` + generated `.conf` use temp →
  `sync_all` → rename; a crash cannot leave a partial chain definition that a
  privileged unit then loads.
- **Generated PipeWire/WirePlumber drop-ins** (`services/pipewire/user_tweaks.rs`):
  parse/merge of user tweak files must not be coerced into unexpected types.
- **systemd user units** (no system units; least privilege).

## Out of scope

- Vulnerabilities in upstream PipeWire/WirePlumber/GTK/libadwaita.
- Bugs in the LADSPA backends (`gtcrn-ladspa`, `swh-plugins`,
  `deepfilternet-ladspa`) — report against those projects.
- The offline calibration harness (`scripts/calibrate/`, dev-only).

## Disclosure

Coordinated. CVE requested when applicable. Credit in CHANGELOG + release notes.
90-day default embargo unless severity dictates faster public.

## STRIDE mapping

| Threat | Mitigation |
|--------|-----------|
| Spoofing | no network control surface; settings file is user-owned |
| Tampering | atomic temp+fsync+rename writes; single config source of truth |
| Repudiation | structured logs; `doctor` diagnostics |
| Information disclosure | local audio processing only; no telemetry/upload |
| DoS | live-vs-topology discipline avoids graph churn; bounded subprocess capture |
| Elevation of privilege | user systemd units only, no setuid, no polkit actions; `unsafe` limited to RT/FFI with SAFETY notes |
