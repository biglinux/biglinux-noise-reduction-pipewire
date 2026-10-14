# Mapa dos patches

Aplicar exatamente na ordem de `patches/series`. A série completa é o candidato; commits intermediários não foram homologados separadamente.

## 01 — Cache do medidor de intensidade e integração por duração real

Arquivo: `patches/0001-fix-loudness-cache-and-integrate-exact-audio-durations.patch`

Arquivos tocados: `app/biglinux_microphone/ui/loudness.py`, `app/tests/test_loudness.py`.

## 02 — AGC a 20 Hz, pico/RMS de todo o bloco e geração da captura

Arquivo: `patches/0002-analyze-agc-at-20-hz-with-full-hop-peak-safety-and-capture-generation-metadata.patch`

Arquivos tocados: `src/agc.rs`, `src/services/audio_monitor.rs`, `src/services/audio_monitor/analyzer.rs`, `src/services/audio_monitor/capture.rs`, `src/services/audio_monitor/types.rs`.

## 03 — Integrador limitado e históricos coerentes com mudanças de ganho

Arquivo: `patches/0003-bound-agc-integrator-reset-stale-evidence-and-preserve-all-gain-normalized-histories.patch`

Arquivos tocados: `src/agc/control.rs`, `src/agc/control_tests.rs`, `src/agc/features.rs`, `src/agc/features_tests.rs`, `src/gain_policy.rs`.

## 04 — Fonte/default em snapshot único com identidade de geração

Arquivo: `patches/0004-resolve-source-identity-and-default-from-one-generation-aware-pipewire-snapshot.patch`

Arquivos tocados: `src/services/echo.rs`, `src/services/pipewire/sources.rs`.

## 05 — Cache nativo do grafo por eventos, com limites de memória

Arquivo: `patches/0005-observe-agc-source-identity-and-demand-through-a-bounded-native-pipewire-graph-cache.patch`

Arquivos tocados: `src/services/pipewire.rs`, `src/services/pipewire/source_watch.rs`, `src/services/pipewire/sources.rs`.

## 06 — Alcance para reconhecer fala independente da janela do integrador

Arquivo: `patches/0006-keep-speech-reachability-independent-from-a-narrowed-integrator-window.patch`

Arquivos tocados: `src/agc/control.rs`, `src/gain_policy.rs`.

## 07 — Calibração com prazo, posse e readback

Arquivo: `patches/0007-make-calibration-bounded-serialized-readback-driven-and-ownership-safe-on-failures.patch`

Arquivos tocados: `src/agc.rs`, `src/agc/calibration.rs`, `src/agc/session.rs`.

## 08 — Daemon compartilhando sessão/posse e rejeitando dados antigos

Arquivo: `patches/0008-integrate-event-driven-agc-sessions-calibration-ownership-and-stale-frame-rejection.patch`

Arquivos tocados: `src/agc.rs`, `src/agc/session.rs`.

## 09 — Transações que preservam remoções e campos futuros

Arquivo: `patches/0009-preserve-deletions-and-forward-compatible-fields-in-concurrent-settings-transactions.patch`

Arquivos tocados: `src/config/storage.rs`.

## 10 — Escoamento PCM separado da consulta e reload coalescido

Arquivo: `patches/0010-keep-spectrum-pcm-draining-during-graph-queries-and-coalesce-ui-state-reloads.patch`

Arquivos tocados: `app/biglinux_microphone/ui/audio_capture.py`, `app/biglinux_microphone/ui/pcm_reader.py`, `app/biglinux_microphone/ui/state.py`, `app/tests/test_pcm_reader.py`, `app/tests/test_state_store.py`.

## 11 — Janelas quantizadas, intervenção manual e piso de ruído

Arquivo: `patches/0011-harden-range-quantization-manual-ownership-and-raw-meter-noise-bounds.patch`

Arquivos tocados: `src/agc.rs`, `src/agc/calibration.rs`, `src/agc/control.rs`, `src/agc/control_tests.rs`, `src/config/audio.rs`.

## 12 — Escritas por chave e callbacks assíncronos sem valores obsoletos

Arquivo: `patches/0012-coalesce-pending-per-setting-writes-and-prevent-obsolete-asynchronous-callbacks.patch`

Arquivos tocados: `app/biglinux_microphone/ui/state.py`, `app/biglinux_microphone/utils/async_utils.py`.

## 13 — Confirmação de persistência/default e fechamento de corridas

Arquivo: `patches/0013-confirm-calibration-persistence-and-microphone-selection-close-startup-and-retry-races.patch`

Arquivos tocados: `app/biglinux_microphone/cli.py`, `app/biglinux_microphone/ui/audio_capture.py`, `app/tests/test_cli_calibration.py`, `src/agc.rs`, `src/bin/cli.rs`, `src/services/pipewire/source_watch.rs`, `src/services/profiles.rs`.

## 14 — Proveniência conservadora de latência e driver correto

Arquivo: `patches/0014-make-latency-provenance-conservative-and-resolve-the-actual-microphone-driver-group.patch`

Arquivos tocados: `app/biglinux_microphone/cli.py`, `src/services/latency.rs`.

## 15 — Testes das filas/cancelamento/callbacks de UI

Arquivo: `patches/0015-add-bounded-ui-queue-cancellation-and-callback-regression-contracts.patch`

Arquivos tocados: `app/tests/test_async_contracts.py`.

## 16 — Posse da calibração até salvar e tap fechado sem reinício

Arquivo: `patches/0016-hold-calibration-ownership-through-persistence-and-prevent-closed-meter-restarts.patch`

Arquivos tocados: `app/biglinux_microphone/ui/audio_capture.py`, `src/agc.rs`, `src/agc/calibration.rs`, `src/bin/cli.rs`, `src/services/latency.rs`.

## 17 — Prazo de todo o handshake da prévia de quantum

Arquivo: `patches/0017-bound-the-entire-quantum-preview-handshake-including-partial-lines.patch`

Arquivos tocados: `src/services/preview.rs`.
