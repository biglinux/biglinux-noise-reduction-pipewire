# Documentação oficial consultada

Consulta: **19/09/2026**. São fontes primárias dos projetos. Versões exibidas nas páginas não equivalem às bibliotecas instaladas no computador do usuário. O anexo fixa `pipewire`/`libspa` 0.10.0, `rustfft` 6.4.1, `async-channel` 2.5.0, `glib`/`gio` 0.22.8, `libc` 0.2.186, `serde` 1.0.228 e `serde_json` 1.0.150; Rust requerido pelo projeto: 1.97.1. Nesta revisão Python foi executado em 3.13.5 com NumPy 2.3.5.

| ID | Documento e endereço | Aplicação na revisão |
|---|---|---|
| D1 | PipeWire `pw-cat`, página exibindo 1.6.9 — https://docs.pipewire.org/page_man_pw-cat_1.html | `--target` aceita `auto`, `node.name` e `object.serial`; não confundir serial com ID global. PCM raw/f32 e latência solicitada; menor buffer pode elevar overhead. |
| D2 | WirePlumber `wpctl`, documentação 0.5.17 — https://pipewire.pages.freedesktop.org/wireplumber/man/wpctl.html | Leitura de volume/mute, escala percentual e `--limit`; `set-default` seleciona alvo de novos streams e deve ter retorno confirmado. |
| D3 | WirePlumber ALSA configuration, 0.5.17 — https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/alsa.html | Mixer de hardware/software, perfis ACP/UCM, routes e política: não substituir wpctl por escrita direta no node presumindo equivalência. |
| D4 | pipewire-rs `RegistryRc`, página 0.10.1 — https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/registry/struct.RegistryRc.html | Registry/listeners/proxies confinados à thread; bind e propriedade dos objetos. A API exata 0.10.0 ainda exige compilação. |
| D5 | pipewire-rs metadata listener, 0.10.1 — https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/metadata/struct.MetadataListenerLocalBuilder.html | Subject, chave/valor opcionais, remoção e limpeza da metadata do default; listener mantido vivo. |
| D6 | pipewire-rs channel, 0.10.1 — https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/channel/index.html | Canal anexado ao loop para parada sem timer de polling dedicado. |
| D7 | PipeWire `pw-top`, 1.6.9 — https://docs.pipewire.org/page_man_pw-top_1.html | Driver versus follower (`+`/`=`), quantum atual versus sugerido, RATE, B/Q, ERR. Não escolher quantum de outro grupo nem chamá-lo de latência ponta a ponta. |
| D8 | PyGObject threading — https://pygobject.gnome.org/guide/threading.html | I/O fora do thread principal; agendamento da atualização da UI com GLib. |
| D9 | Python selectors, página 3.14.7 — https://docs.python.org/3/library/selectors.html | Espera cancelável e limitada de pipes em Unix, short reads, EOF e DefaultSelector. Testado aqui com Python 3.13.5, não 3.14.7. |
| D10 | Python subprocess — https://docs.python.org/3/library/subprocess.html | Gestão de subprocessos, pipes, cancelamento e reaping. |
| D11 | Rust `std::fs::File`, documentação exibindo 1.98.1 — https://doc.rust-lang.org/std/fs/struct.File.html | `try_lock`/`unlock`, locking de arquivo; verificar o ambiente 1.97.1 do projeto, sem instalar toolchain. |
| D12 | RustFFT `Fft`, API 6.4.1 — https://docs.rs/rustfft/6.4.1/rustfft/trait.Fft.html | Planejamento/buffers/scratch do analisador; não introduzir planejamento por bloco nem alegar nova otimização onde o original já usava scratch. |
| D13 | PipeWire `pw-dump` — https://docs.pipewire.org/page_man_pw-dump_1.html | Snapshot JSON usado para resolver fonte/default de forma coerente. |
| D14 | PipeWire properties — https://docs.pipewire.org/page_man_pipewire-props_7.html | Propriedades de stream/target, identidade e política de relink; referência da captura explícita. |

## Como estas referências foram usadas

Os contratos acima foram confrontados com funções reais do anexo. A assinatura e o padrão de propriedade das APIs nativas foram comparados também com `src/services/pipewire/native.rs`, já existente; o construtor `.timeout(Duration)` tem uso prévio em `src/services/pipewire/live.rs`. Isso fornece evidência de integração, não substitui os tipos e testes do vendor ausente.

Não foi possível obter uma página utilizável para parte das APIs exatamente em `docs.rs/pipewire/0.10.0`; a comparação com documentação 0.10.1 ficou registrada como diferença de versão, não como verificação da compilação 0.10.0. O agente precisa conferir o crate travado depois de recuperar o ambiente.

As escolhas numéricas de controle (20 Hz, expiração de 250 ms, limite de 5 escritas/s e prazo de stall) são decisões desta revisão a validar em hardware, não prescrições das documentações. A melhora de 12× vem do experimento local descrito no relatório, não de uma alegação upstream. Nenhuma norma de loudness foi declarada integralmente implementada/certificada.
