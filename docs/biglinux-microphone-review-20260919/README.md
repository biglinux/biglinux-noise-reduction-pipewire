# BigLinux Microphone — revisão e patches — 19/09/2026

**17 patches ordenados; 33 arquivos de código/teste alterados.** Base exclusiva: `biglinux-microphone-source.tar.gz`, SHA-256 `ceea62839287b3d845407f2eb3da4b21dca1d58dd4688fd673f8ee894b16f234`.

O pacote contém alterações concretas, não pseudocódigo. É uma proposta de integração: **a aplicação/reversão dos patches e os testes Python/JavaScript foram verificados; a compilação Rust, GTK real e o comportamento acústico NÃO foram validados.** Não liberar diretamente como versão estável.

## Resultado principal

O AGC passa de 93,75 para 20 análises espectrais por segundo, com picos/RMS calculados em todas as amostras de cada bloco. O controle recebe proteção contra integrador saturado, observações antigas, disputa com calibração, alterações manuais, mute e troca de geração da fonte. O teto de 300% e a convenção cúbica de ganho foram preservados.

No microbenchmark da função Python `LoudnessMeter.push_block`, a mediana passou de **228,71 para 19,10 µs**, aproximadamente **12×**. Isto **não** é a redução de CPU do programa inteiro, nem uma medição do AGC Rust.

## Leitura e aplicação

Começar por `AGENTE_IMPLEMENTACAO.md`. `RELATORIO.md` explica causas e decisões; `TESTES_HARDWARE.md` define a homologação; `FONTES.md` registra documentação oficial e diferenças de versão. `patches/series` fixa a ordem. **Não reaplicar os patches históricos que já estavam dentro do anexo original.**

Antes de modificar a árvore:

```sh
cd /caminho/deste-pacote
sha256sum -c SHA256SUMS
python3 tests/verify_series.py /caminho/da/base-original-extraida
```

O verificador usa uma cópia temporária dos arquivos tocados e não altera a base. Não compila, instala ou executa áudio.

## Evidências

- 66 testes Python, com 33 subtestes adicionais, passaram na árvore final.
- 19 testes de lógica passaram com GLib simulado; **não equivalem a testes GTK**.
- 2 testes do redutor do indicador Plasma passaram em Node.
- 4 testes dos parsers do coletor de hardware passaram; não executam coleta real.
- 47 arquivos Python passaram por análise sintática e 8 scripts por `bash -n`.
- Aplicação e reversão dos 17 patches passaram, com hashes dos 33 arquivos conferidos.
- 29 novas funções de teste Rust foram incluídas: **NOT_RUN**.

**Bloqueio de compilação identificado:** o anexo não contém `vendor/big-rust-components`, dependência por caminho. A revisão não instalou nem invocou Rust. O agente deve usar o ambiente previamente preparado e recuperar a dependência na revisão fixada pelo projeto, sem alterar silenciosamente `Cargo.lock` ou instalar outra toolchain.
