from pathlib import Path
from _common import done, write, commit

TITLE = 'docs: explain safe user choices and replace stale readiness guarantees'
if not done(TITLE):
    previous = Path('REVIEW.md').read_text()
    write('docs/reviews/2026-06-19.md', '> Historical report, retained for provenance. Its claims and test results\n> do not certify subsequent revisions and were not revalidated by this change.\n\n' + previous)
    write('REVIEW.md', '''# Review corrections — 2026-09-08

Reference audit: main at `f674c2006b97f91cfded5a5463f68f3714b10c87`.
Implementation: pull request #30. Read the PR's latest validation summary and
Actions runs for the exact tested commit; this file is not a test certificate.
The June report is preserved under `docs/reviews/2026-06-19.md`, not carried
forward as a current 10/10 score.

## Traceability

| Finding | Implementation / regression surface |
| --- | --- |
| Preserve user configuration | `pipeline/migration.rs`, generic filter-chain preservation and no-overwrite backup tests |
| Concurrent settings and corrupt files | `config/storage.rs`, stable lock, three-way merge, conflict and malformed-file tests |
| Backend reduction versus gate | `pipeline/mic.rs`, `tests/review_audio.rs` |
| Shared CLI/GUI graph reconciliation and honest failures | `services/reconcile.rs`, observed-state planner tests, CLI integration |
| Close without a working audio server | `ui/mic_shell.rs`, monitor lifecycle contracts |
| Device selection and volume off the UI thread | `ui/widgets/source_picker.rs` |
| Applet process retries and accessibility | Plasma `main.qml`, explicit controls, bounded retry policy |
| Reversible buffer preview | `services/preview.rs`, timed preview lease, cancellation and restoration |
| Applied versus edited tuning, reset and context | advanced view/apply, retained tuning page, navigation restoration |
| Preserve stereo or explicitly choose mono | `pipeline/output.rs`, `tests/review_stereo.rs` |
| Preserve preferences while bypassing | runtime settings projection, `tests/review_master.rs` |
| FFT scratch reuse and bounded monitoring | analyzer/capture/monitor modules and tests |
| Structured journal diagnostics and measured quality | `pwloader/denoise_watch.rs`, `config/plugin_cost.rs` |
| Optional resource tuning | `config/runtime.rs`, loader affinity/memory settings, collapsed advanced rows |
| Meter values and reduced motion | native LevelBar/text value, accessible spectrum container |
| Rust and QML translation coverage | extraction markers, `refresh-pot.sh --check`, merged gettext catalogs |
| Subprocess allow-list, deadlines and output bounds | vendored `big-os-kit/subprocess`, `tests/review_subprocess.rs` |
| XDG integration | loader arguments and `tests/review_xdg_units.rs` |
| Dependency advisories | updated Cargo locks and synchronized Flatpak registry sources |
| Real test coverage instead of silent skips | `scripts/test-ui.sh`, `scripts/test-miri.sh`, strict portable quality gate |
| Nix closure and gettext | `default.nix`, `packaging/nix/` |

## Validation boundaries

Compilation and pure regression tests do not measure sound quality, actual
hardware latency, CPU improvement, assistive-technology usability or visual
polish. Required before a production release: a disposable real PipeWire and
WirePlumber session; GTK and Plasma keyboard/AT-SPI tests; long translations,
large fonts and contrast themes; USB/Bluetooth hotplug and loaded-CPU audio;
packaged installation and upgrade tests. NixOS and Flatpak require their own
integration tests and are not certified by an Arch build.

Do not dismiss a secret scanner's unverified finding as a false positive without
locating and reviewing it. Unsupported Miri FFI is not evidence of undefined
behavior, and an unavailable required tool is not a passing check.
''')
    write('ARCHITECTURE.md', '''# Architecture — Filter noise

## Processes and ownership

One Rust library (`biglinux_microphone`) and four entry points:

| Binary | Responsibility |
| --- | --- |
| `biglinux-microphone` | Relm4 GTK4/libadwaita window and its monitoring resources |
| `biglinux-microphone-cli` | Settings mutations, diagnostics, reconciliation and session watch |
| `biglinux-microphone-pwloader` | Host one PipeWire module in a client process |
| `biglinux-microphone-probe` | Diagnostic measurements |

The mic, echo canceller and playback loaders connect to the existing PipeWire
server. They share the graph's scheduling/clock domain, not one daemon-owned
processing thread. `node.async` is an asynchronous scheduling choice, not an
adaptive-resampler switch; evaluate its added cycle latency separately.

## State contracts

Desired preferences, effective processing settings and observed graph state are
different. A master bypass changes the effective projection without erasing the
user's sub-effect choices. The persistent JSON lives under
`$XDG_CONFIG_HOME/biglinux-microphone`, with the usual home-directory fallback.
Loaders and the applet resolve that same location.

A stable sibling lock serializes participating writers. GUI edits are merged
against their original snapshot; unrelated external fields survive and a
conflicting change to the same field is reported. Invalid JSON is not silently
overwritten. Atomic replacement protects a single file; it is not a multi-file
transaction and does not by itself solve concurrent read-modify-write.

## Applying changes

The UI coalesces edits and executes blocking work outside GTK's main thread.
`services/reconcile.rs` is shared by CLI and GUI: render effective settings,
compare the previous applied snapshot, inspect actual nodes, update live
parameters where possible and reconcile only the affected loaders. AEC precedes
the microphone that consumes it. Missing/error nodes and incomplete updates
cannot be represented as fully applied settings. Failures propagate to callers.

Persistence, generated arguments, service state and plugin inference have
separate failure modes. Successful `systemctl` invocation alone is insufficient;
node presence is checked. Presence is still not proof of useful denoising, so
processing-health counters and listening/measurement tests remain necessary.

## Interface and lifecycle

Relm4 owns the main application state and apply/health generations. The device
picker performs blocking device operations on workers. The tuning page retains
its edit model and distinguishes pending choices from applied settings. A
buffer preview has a bounded lifetime and restores the prior override. Closing
the window must release its monitor without requiring a healthy audio server.

The microphone meter exposes a native GTK value and textual reading; the Cairo
spectrum is supplemental. Respect reduced motion, preserve navigation context,
and use visible labels plus accessible names for controls. Expert CPU affinity
and memory-reservation choices are opt-in, not universal performance guarantees.

## Monitoring and subprocesses

FFT plans, windows, normalization and scratch buffers are reused. Visualization
is bounded and favors recent frames; hidden capture must not intentionally block
a real-time writer. The subprocess boundary uses argv arrays, explicit policies,
nonblocking Unix I/O, bounded captured output, full-operation deadlines and
process-group cleanup. An explicitly empty allow-list denies every executable.

## Dependencies and translation

Native target: Rust >= 1.97.1, GTK >= 4.22, libadwaita >= 1.9, PipeWire >= 1.4,
WirePlumber >= 0.5, systemd user services, GTCRN and SWH LADSPA packages.
Optional neural backends require their matching inference runtimes. Nix uses
the system-allocator feature configuration rather than the BigLinux-specific
jemalloc patch. Portability of packaged integrations must be tested separately.

The gettext domain is `biglinux-microphone`. Rust `i18n` calls, deferred `mark`
literals and QML `i18nd` calls share the catalog. Extraction coverage is checked
independently of PO syntax and translation completeness. Empty UI text must not
be resolved as gettext's reserved metadata header.

## Tests and evidence

Run `scripts/quality-check.sh --ci` for required portable gates,
`scripts/test-ui.sh` for isolated graphical contracts, and
`scripts/test-miri.sh` for the selected pure data-model tests. See `REVIEW.md`
for finding-to-implementation mapping. No passing text-renderer test constitutes
an acoustic benchmark or an accessibility/visual-design certification.
''')
    write('docs/guia-rapido.pt_BR.md', '''# Filtro de ruído: escolha pelo que você precisa ouvir

## Para começar

Use a tela simples. Escolha o microfone, fale no seu volume habitual e observe
o medidor. Ligue o filtro do microfone. Comece com a qualidade automática e
ajuste a intensidade apenas se ainda houver ruído ou se a voz ficar artificial.
Você não precisa escolher um modelo de inteligência artificial para começar.

Faça uma mudança por vez. Compare uma frase curta antes e depois; o resultado
mais útil é uma voz compreensível, não necessariamente silêncio absoluto.

## Qualidade e consumo

**Automática** escolhe entre os modelos disponíveis segundo a política do
aplicativo e as medições que ele consegue obter. Isso não é uma garantia de
adaptação contínua a toda mudança de carga do computador.

**Menor uso de CPU** é a primeira alternativa quando o computador está ocupado
ou o som apresenta cortes. **Melhor qualidade** prioriza o modelo de maior
qualidade da política e pode consumir mais recursos. A lista manual fica nos
controles avançados; modelos sem a biblioteca necessária não são escolhas
funcionais, mesmo que um arquivo do plugin esteja presente.

A intensidade controla quanto o filtro atua. Aumentar demais pode mudar o
timbre ou prejudicar consoantes. Reduza um pouco e compare novamente.

## Eco e retorno da própria voz

O eco acontece quando o microfone capta o que sai dos alto-falantes. A opção
automática de cancelamento considera o dispositivo de saída. Fones de ouvido
normalmente evitam esse caminho acústico.

Use **Ouvir minha voz** somente com fones. Com alto-falantes, o retorno pode
produzir realimentação. Comece com volume baixo. O retorno pertence à janela e
é encerrado ao fechá-la; não é uma gravação permanente.

## Som do sistema

O filtro de saída modifica o áudio que você escuta. Para música, filmes e
jogos, preserve o estéreo. O modo mono é uma escolha explícita para situações
centradas em voz e pode usar menos processamento; ele combina os canais e perde
a separação espacial. Compare antes de deixá-lo ligado em todo o sistema.

Desligar o controle principal suspende o processamento correspondente sem
apagar as escolhas dos efeitos. Restaurar os padrões é uma ação diferente,
confirmada antes de substituir suas preferências.

## Quando abrir os controles avançados

O equalizador muda o timbre; o compressor reduz diferenças de volume; o gate
atenua pausas. Eles não precisam ficar todos ligados. Para começar, mantenha
os padrões e habilite apenas o efeito que resolve um problema que você ouviu.

A aba de ajustes do sistema é para cortes, atrasos ou particularidades de um
dispositivo. Ela distingue alterações pendentes das que já foram aplicadas.
Aplicar esses ajustes pode reiniciar o áudio e interromper uma chamada: faça
isso fora de uma conversa importante.

A prévia do tamanho do buffer é temporária e pode ser interrompida. Ela altera
a sessão de áudio, não apenas este aplicativo. Se o resultado piorar, interrompa
a prévia e retorne ao valor anterior; não é preciso salvar para experimentar.

**Preferir CPUs rápidas** e **reservar memória** são ajustes especializados.
Deixe-os desligados inicialmente. Podem ajudar em algumas máquinas, mas também
restringir o escalonamento ou aumentar a memória indisponível para outros
programas. Não existe uma combinação universalmente mais rápida.

## Quando aparecer um erro

Leia a causa e a próxima ação. Uma preferência salva não significa que o
filtro conseguiu iniciar. Use a ação de tentar novamente depois de corrigir
a causa. A janela deve poder ser fechada mesmo quando o áudio não responde.

Se dois programas alterarem o mesmo campo ao mesmo tempo, um conflito pode
ser informado em vez de apagar silenciosamente uma das escolhas. Confira o
valor atual e refaça a alteração pretendida.

O diagnóstico de terminal está disponível em `biglinux-microphone-cli doctor`.
Ao relatar um problema, inclua a versão, o dispositivo e a mensagem apresentada;
não publique gravações privadas, credenciais ou um log inteiro sem revisá-lo.
''')
    readme = Path('README.md').read_text()
    readme = readme.replace('**Spectrum analyzer** — 30 bands at 60 fps', '**Spectrum analyzer** — 30 visual bands, bounded updates and an accessible level reading')
    readme = readme.replace('(`--ci` mirrors the exact CI gate; `--fix` applies `cargo fmt`).', '(`--ci` requires every portable tool; `--fix` formats Rust). Run\n  `scripts/test-ui.sh` separately for an isolated graphical session.')
    readme = readme.replace('watched via `gio::FileMonitor` / `inotifywait` for instant bidirectional\nsync.', 'synchronized through file monitoring and bounded fallback polling. Concurrent\nwrites use a stable lock and conflict-aware merging; saved intent is distinct\nfrom the observed audio graph.')
    readme = readme.replace('`~/.config/biglinux-microphone/settings.json`', '`$XDG_CONFIG_HOME/biglinux-microphone/settings.json` (default: `~/.config`)')
    readme = readme.replace('`pipeline/` | Filter-chain `.conf` generation + systemd unit orchestration', '`pipeline/` | Filter-chain module-argument generation and conservative migration')
    readme = readme.replace('  using the same GTCRN-based chain on the playback side.', '  with stereo preserved by default. Explicit mono mode trades channel\n  separation for lower processing cost on voice-centered material.')
    readme = readme.replace('## Source ownership', '''## Choosing settings

Start with the simple view and automatic quality. Advanced controls are optional;
master bypass preserves individual preferences. Buffer previews are reversible,
and expert affinity/memory policies are opt-in. See the
[Portuguese quick guide](docs/guia-rapido.pt_BR.md) for practical choices.

For Nix/NixOS, see [packaging/nix](packaging/nix/README.md). The experimental
Flatpak recipe is not a complete host integration: sandbox permissions do not
make host binaries/plugins appear at the sandbox's `/usr` paths. Do not treat
manifest parsing as a working audio test or add unrestricted host execution as
a shortcut. Native distribution packages remain the primary supported route.

## Source ownership''')
    readme = readme.replace('- Tuning research and PipeWire/WirePlumber config rationale live in\n  [TIPS.md](TIPS.md). The offline calibration harness lives in\n  `scripts/calibrate/` (see its README).', '- Runtime ownership and validation boundaries are documented in\n  [ARCHITECTURE.md](ARCHITECTURE.md) and [REVIEW.md](REVIEW.md).')
    Path('README.md').write_text(readme)
    commit(TITLE, ['ARCHITECTURE.md','README.md','REVIEW.md','docs/reviews/2026-06-19.md','docs/guia-rapido.pt_BR.md'])

TITLE = 'fix(nix): expose only the supported Linux target and integration module'
if not done(TITLE):
    write('flake.nix', '''{
  description = "Filter noise — native PipeWire application and NixOS integration";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
      let pkgs = import nixpkgs { inherit system; };
      in {
        packages.default = pkgs.callPackage ./default.nix { };
        packages.biglinux-noise-reduction-pipewire = self.packages.${system}.default;
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/biglinux-microphone";
        };
        devShells.default = pkgs.mkShell {
          inputsFrom = [ self.packages.${system}.default ];
          packages = with pkgs; [ rustc cargo clippy rustfmt cargo-audit cargo-deny cargo-machete gettext ];
        };
      }) // {
        nixosModules.default = import ./packaging/nix/module.nix;
      };
}
''')
    commit(TITLE, ['flake.nix'])
