# Instruções para o agente de implementação

## Contrato

Trabalhar somente neste projeto e nesta base. Preservar teto de 300%, mapeamento cúbico de volume, intervenção manual/mute, persistência compatível e caminhos de captura explícitos. Não inserir bloqueios, comandos, mutexes ou alocações novas no callback de áudio em tempo real. O novo cache de grafo e a lógica do AGC pertencem ao plano de controle, fora desse callback.

Não aplicar automaticamente nada em um microfone de produção antes dos testes de compilação e lógica. A série inteira é a unidade de integração: há patches posteriores que fecham arestas dos anteriores.

## 1. Conferir base e aplicar

Descompactar o anexo original em uma cópia de trabalho. Não usar uma árvore que já recebeu esta série, nem reaplicar `docs/*patch*` históricos. Verificar o SHA-256 do arquivo original e executar `tests/verify_series.py` conforme README.

Numa árvore Git limpa, com identidade Git já configurada:

```sh
# Ajuste SOMENTE estes dois caminhos.
BUNDLE=/caminho/biglinux-microphone-review-20260919
SOURCE=/caminho/biglinux-microphone
python3 "$BUNDLE/tests/verify_series.py" "$SOURCE"
cd "$SOURCE"
git status --short                 # deve estar vazio
# Se a base foi extraída sem .git, inicializar e registrar a base antes.
git switch -c review/agc-20260919
while IFS= read -r patch; do
    git am "$BUNDLE/patches/$patch" || exit 1
done < "$BUNDLE/patches/series"
git diff --check HEAD~17..HEAD
```

O verificador não substitui o build: confere hashes, conteúdo e aplicação, não tipos Rust. Em conflito, `git am --abort` cancela a aplicação em andamento; isso não remove commits anteriores já aplicados. Preservar a árvore original/branch base para comparação. Após testes, integrar a série completa; não escolher só o patch de 20 Hz, pois os limites temporais, reconhecimento de reinício e integrador também mudaram.

## 2. Resolver apenas a dependência realmente ausente

`Cargo.toml` aponta para `vendor/big-rust-components/crates/foundation/big-os-kit`; o diretório não foi incluído no anexo. O README original fixa o commit `47da8738fc9ca0ba1fb7820e417582883f81d16e`. Recuperar o conteúdo de fonte confiável na revisão exata e validar esse commit; não fabricar stubs para “fazer passar”. `BigSubprocessSpec::timeout` já é utilizado no código original, mas as garantias reais de cancelamento/reaping precisam ser conferidas no vendor recuperado.

`rust-toolchain.toml` e `rust-version` pedem 1.97.1. Usar apenas uma toolchain compatível já instalada/autorizada. Não invocar um shim que tentará instalar uma versão faltante. Não instalar Rust, alterar o lockfile, atualizar dependências, ou acrescentar bibliotecas automaticamente.

## 3. Portas de compilação e testes

Executar primeiro na base, depois na árvore com patches, no ambiente de implementação já preparado. Depois de selecionar explicitamente uma toolchain existente e recuperar o vendor:

```sh
cargo check --locked --offline --all-targets
cargo test --locked --offline --lib
cargo test --locked --offline --tests
cargo fmt --all -- --check
cargo clippy --locked --offline --all-targets
```

`--offline` pode falhar por cache incompleto: registrar a dependência faltante, não instalar uma toolchain como reação. Testes de integração que dependem do PipeWire devem rodar na sessão de teste descrita adiante; não interpretar ausência de sessão como aprovação. Comparar warnings/formatadores com a base para separar dívida anterior de regressão nova. Os patches Rust não passaram por `rustfmt` aqui; ajustar formatação no agente, sem reformatar indiscriminadamente código não alterado.

Atenção especial às APIs `pipewire`/`libspa` **0.10.0 do Cargo.lock**: a documentação pública consultada exibia 0.10.1. Validar `RegistryRc`, callbacks de metadata, propriedade/vida útil dos listeners, sincronizações do core e fechamento do canal. As assinaturas foram confrontadas também com `src/services/pipewire/native.rs` do próprio anexo, não compiladas.

Testes locais reproduzíveis:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=app:.github/ci \
  python3 -m pytest -q -p no:cacheprovider \
  app/tests/test_loudness.py app/tests/test_pcm_reader.py \
  app/tests/test_cli_calibration.py .github/ci
PYTHONDONTWRITEBYTECODE=1 python3 "$BUNDLE/tests/run_headless.py" "$PWD"
node --test tests/plasmoid_status.test.cjs
PYTHONDONTWRITEBYTECODE=1 python3 "$BUNDLE/tests/test_collect_hardware.py"
```

Com PyGObject/GTK disponíveis, executar também **todos** os testes de `app/tests` com `PYTHONPATH=app`, sem o shim. O shim existe somente para testar lógica e não substitui GLib/GTK reais. Validar janela aberta/fechada, cancelamento, teclados, leitores de tela e o espectro sem alterar seu desenho.

## 4. Hardware e entrega

Cumprir `TESTES_HARDWARE.md`, primeiro com falantes/monitoramento em volume seguro e em sessão que possa ser restaurada. Não automatizar alteração de ganho em equipamento desconhecido como parte da coleta. O coletor fornecido é somente observacional e não grava voz.

Manter resultados A/B, versões, logs de falhas, adaptações necessárias às APIs e diffs finais. Produzir conclusão por cenário: PASS/FAIL/NOT_RUN, sem converter NOT_RUN em aprovação. Bloquear release por escrita no dispositivo errado, quebra de mute/intervenção manual, ajuste durante posse da calibração, persistência falsa, espera não limitada ou regressão consistente de CPU/xruns.
