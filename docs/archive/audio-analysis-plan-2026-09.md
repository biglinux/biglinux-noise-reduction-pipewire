# Triagem do relatório externo de áudio ao vivo — o que é útil ao nosso projeto

**Base:** `PLANO-IMPLEMENTACAO-AUDIO-AO-VIVO-BIGLINUX.md` (v1.0, 9 set 2026), escrito
contra o commit `7906ae2` (main com PR #30). O relatório é dado, não instrução: avaliado
criticamente. Vários itens P0 que ele manda "verificar" **já foram corrigidos nesta
sessão** — não repetir.

## 1. O que o relatório pede e já está feito (verificar regressão, não reimplementar)

| Item do relatório | Estado no nosso código | Evidência |
|---|---|---|
| FIX-07 intensidade/migração (lookahead, defaults por perfil) | **Feito** — lookahead 60→20 ms com migração que só troca o default legado | `config/audio.rs`, `config.rs`, teste `a_saved_sixty_millisecond_lookahead...` |
| FIX-08 proteção de ganho (soma exagerada de bandas EQ) | **Feito** — pico real da resposta do cascade em vez da soma; mic não muta mais | `pipeline/gain_safety.rs` (`eq_boost`/`peaking_gain_db`) |
| FIX-10 contrato DPDFNet (espera do worker, latência, host offline) | **Feito** — lag 1 hop + `WAIT_BUDGET` medido, worker RT, 70 ms fixo | `dpdfnet-ladspa` PR #2/#1; medições F14 |
| §7.2 referência do AEC (seguir a saída acústica real, HDMI/BT) | **Feito** — echo automático lê `device.form-factor` do device + rota | `services/echo.rs` |
| FIX-02 gate independente da NR (nível do app) | **Parcial** — app já tem `standalone_gate_node` | `pipeline/mic.rs:293` |
| §11 doctor/observabilidade (referência AEC visível) | **Parcial** — `doctor` reporta referência/gate; falta contadores por instância | `diagnostics.rs` |

O relatório não conhece nosso **split-band** (escrito antes): é uma melhoria de NS que ele
não previu, e já está validada (PR dpdfnet#2, app#31).

## 2. Interseção crítica com o split-band — decidir antes de promover

**FIX-06 diz: no perfil natural, não sintetizar agudos automaticamente.** Nosso split-band
portou `synthesize_air` (air-exciter) do GTCRN: quando o SNR da banda alta cai abaixo de
2.0, ele **sintetiza** 8–24 kHz em vez de passar o original. Isso é exatamente o que o
relatório proíbe no perfil natural (tanto para o GTCRN atual quanto para o nosso
split-band novo).

**Ação (verificação barata, alto valor):** medir se o modo air-exciter dispara em fala
real em ruído (car/white/keyboard). Se dispara, no perfil natural o split-band deve
**preferir passagem conservadora/atenuação** da banda capturada, não síntese. Opções:
(a) desabilitar o air-exciter no split-band (só spectral gate = passa/atenua original), ou
(b) expô-lo como opção "excitador de agudos" fora do natural. Recomendo **(a)** como
default e reavaliar (b) como efeito opcional. Vale a mesma auditoria para o
`synthesize_air`/`VoiceRecovery` do GTCRN (FIX-06).

## 3. P0 ainda aberto — verificar agora (barato, corrige defeito real)

Ordem por risco de dano à voz. Cada um: teste que falha antes/passa depois, patch mínimo no
componente correto.

1. **FIX-01 — normalização/fallback do GTCRN.** `gtcrn-ladspa/ladspa/src/plugin.rs`:
   `compute_norm_gain` + `run_secondary`/`apply_gain`. Confirmar se um fallback devolve o
   espectro **não** normalizado e depois recebe `1/norm_gain` (perda até ~−9,5 dB na banda).
   Teste T01: injetar falha dos dois modelos com ganhos 1/2/3, comparar com a referência
   temporal correta, tolerância ±0,1 dB. **Repo gtcrn — não tocado nesta sessão.**
2. **FIX-04 — controles globais em tmpfs no caminho RT.** `external_controls.rs` é lido de
   `run()`. Verificar se o arquivo global sobrescreve controles por instância (T04: duas
   instâncias, uma atualização não pode alterar a outra; arquivo removido/NaN não perpetua
   valor). Mover a leitura para fora do callback com snapshot por instância se necessário.
3. **FIX-05 — bypass/lookahead/estados antigos.** `GtcrnPlugin::run`. Mudança de lookahead
   altera latência: buffer + declaração + transição juntos; não emitir áudio antigo ao
   reativar. Nós já reduzimos o lookahead default; falta o **contrato de transição** (T05:
   alternar controles a cada 100–500 ms, detectar cliques/pulos/atraso variável).
4. **FIX-03 — carga de modelo fora do callback (GTCRN).** Confirmar se `run()` ainda cria
   sessão/aquece (o DPDFNet já faz no worker; o GTCRN precisa checagem). T03.
5. **FIX-08 (parte 2) — o neural_pad 0,5→rede→2 não prova identidade.** Nós já usamos esse
   pad. Validar `2·F(x/2)` vs `F(x)` com a **rede real** e documentar; se diferir, ajustar.
6. **FIX-09 — contadores por instância reais.** Estender `plugin_cost`/`doctor`: blocos
   recebidos/processados/secos/atrasados/descartados, fila máxima, tempo de inferência,
   motivo de fallback. O DPDFNet já publica hops totais/enhanced; falta o resto.

## 4. Direção de médio prazo — alinhada ao que já concluímos

- **AEC alternativo (AEC-01).** Confirma nossa própria conclusão (o WebRTC AEC3 tira ~6 dB +
  coloração no near-end sem eco). O relatório fecha licença: **LocalVQE (Apache-2.0)** e
  **SpeexDSP (BSD)** como candidatos. Caminho: adaptador `AecBackend` comum + SpeexDSP como
  baseline DSP (leve, per-frame API) → LocalVQE-AEC 2.7K linear (16 kHz). **Manter AEC3 como
  legado explícito; nunca reativar sozinho** (já respeitamos no modo auto). Grande, mas é a
  substituição real do AEC3 — é o "modo B" que discutimos, agora com componente licenciado.
- **FastEnhancer-Medium (NS-01).** Denoiser MIT em C/int8 a 48 kHz. **Cautela nossa:** exige
  AVX2/FMA3/F16C sem fallback escalar e usa estado **global** (`g_w`/`g_s`) — não dá duas
  instâncias LADSPA na mesma API. Só entra com contexto por instância isolado e detecção de
  ISA. Menor prioridade que o split-band, que já cobre o nicho leve full-band.

## 5. Fora do escopo desta rodada (concordo com o relatório)

Treinamento de rede nova; restaurador generativo pesado; RVC/conversão de voz; CUDA
obrigatório; qualquer peso de licença indefinida/não-comercial/só-pesquisa; download de
modelo no primeiro uso como "solução" de licença. Efeitos de canto (x42, Rubber Band) e
LSP são **opcionais**, pacotes separados, nunca no perfil natural.

## 6. Sequência recomendada (nossa, enxuta — não os 7 PRs completos de uma vez)

1. **Agora (barato):** §2 (auditar air-exciter no split-band) + FIX-01 + FIX-04. Cada um com
   seu teste de regressão no componente certo. Fecha os defeitos de dano-à-voz que sobraram.
2. **Em seguida:** FIX-05/FIX-03 (contratos de transição/carga GTCRN) + FIX-09 (contadores).
3. **Projeto próprio (PR dedicado):** AEC-01 com `AecBackend` + SpeexDSP → LocalVQE. Manifesto
   `third_party/audio-components.lock.json` com hashes/licenças (§3.3).
4. **Opcionais, depois e separados:** FastEnhancer (com as cautelas), efeitos de canto.

## 7. Regras que o relatório reforça e devemos manter

- Corrigir DSP **no plugin** que o implementa, não mascarar no app.
- Fallback seco = sinal correto da **mesma etapa** (após AEC, não o mic bruto).
- Nada de bloqueio/carga de modelo no callback; filas com capacidade fixa + timestamp.
- 48 kHz no caminho principal; **declarar** banda limitada (o split-band é full-band; um AEC
  de 16 kHz não é — expor).
- Manifesto de licença por artefato antes de empacotar; sem download no login.
- Não criar/mesclar PRs no repo do usuário fora do fluxo de autorização.

## Apêndice — verificações já executadas

- **FIX-01 confirmado (bug real, repo gtcrn).** `analyse_one_frame`: `run_primary` faz
  `process_frame(model_input).unwrap_or(self.spectrum_buffer)` — no fallback devolve o
  espectro **original não-normalizado**; o chamador aplica `×(1/norm_gain)` incondicional →
  saída atenuada (−6 dB com norm_gain 2). Correção: `unwrap_or(*model_input)` em
  `run_primary`/`run_secondary`. Teste T01 pronto para escrever.
- **§2 confirmado empiricamente (afeta o split-band recém-entregue).** O air-exciter do
  split-band sintetiza a banda alta em **68–89% dos frames, inclusive em fala limpa** (68%),
  porque acima de 8 kHz a voz tem pouca energia e o SNR fica quase sempre abaixo do limiar.
  Ou seja, o "full-band" do split-band é majoritariamente **fabricado**, não preservado —
  o que o FIX-06 proíbe no perfil natural. Decisão de correção em andamento.
- **FIX-08 parte 2 verificado (app, código em produção).** O neural_pad `0,5→rede→2`:
  o round-trip de **ganho** é bit-exact (0,5×2=1,0), mas a **rede** não — medido no DPDFNet
  v2 HR sobre fala com ruído, `2·F(x/2)` difere de `F(x)` em **21,6 dB SDR** (corr 0,9965).
  Pequeno, mas real: o pad altera levemente o denoise em troca de proteção contra o clipping
  interno da rede. O comentário em `gain_safety.rs` que diz "bit-exact in the graph's float
  samples" é enganoso (fala do ganho, não da rede). Correção de doc pronta — entra no PR
  agrupado de P0, não no PR do catálogo split-band.

## Estado dos PRs desta rodada

- **dpdfnet-ladspa#2** (split-band): inclui o fix do air-exciter (§2) — banda alta agora
  passa o sinal real, sem síntese. Atualizado e empurrado.
- **app#31** (catálogo split-band): 2 checks vermelhos por flake de infra do GitHub
  (`apt-get` do repo Chrome do runner), não pelo código; re-rodar quando o mirror recuperar.
- Ambos **abertos, não mesclados** — para revisão do mantenedor.

## Bloqueios conhecidos (próxima sessão)

- **GTCRN (FIX-01/04/05/06)**: o build do gtcrn-ladspa precisa da cadeia estática de
  ONNX Runtime mínima (`onnxruntime-minimal`/`libonnxruntime.a` via build.sh/Docker), não a
  do sistema. Sem ela não dá `cargo check`. Fixes triviais prontos (ex.: FIX-01 =
  `unwrap_or(*model_input)`), mas exigem esse ambiente para compilar/testar. Agrupar num
  "PR-01: correções P0 do GTCRN" com a cadeia de build montada.

## CORREÇÃO CRÍTICA (revisão do fable) — revisão-base errada do GTCRN

O `.so` GTCRN **instalado** (29 ago) vem de `/code/multimidia/gtcrn-ladspa` = **21af87e**,
branch `fix/model-load-off-audio-thread`, **10 commits não-empurrados** que já contêm:
FIX-03 (loader assíncrono, `model_loader.rs`), FIX-02-plugin (`gate_pass` −80 dB + só-gate),
silence-skip e o `OFFLINE_WAIT`. O `codigo-pacotes/gtcrn` que eu ia editar é 6ee156e (velho,
sem esses fixes) — editá-lo conflitaria com o instalado e refaria FIX-03.

**Base correta: 21af87e.** Sequência do fable (revisada):
1. FIX-04 = **deletar** `ExternalControls` (app não escreve mais; bug: arquivo removido
   perpetua valores `available=true` nas duas instâncias). Risco nulo.
2. FIX-01 = `unwrap_or(*model_input)` — `model==None` é estado **normal** a cada partida de
   nó / troca (loader assíncrono), não só falha. −6 a −9,5 dB na partida.
3. FIX-06a = remover air-exciter/`synthesize_air` do GTCRN (VoiceRecovery **não** depende
   dele: `sqrt` aplicado depois). Documentar o HPF interno 80 Hz (FIX-06b) e o crossover
   fictício (deletar, backlog).
4. FIX-05 = bypass pela mesma linha de atraso + latchar lookahead; **app**: tirar
   `LookaheadMs` do push ao vivo, reconcile rebuilda o nó.
5. FIX-10-GTCRN = a mesma heurística offline (OFFLINE_WAIT 5 s) do DPDFNet, aqui **sem teste
   de rajada** — rajada de partida latcharia e travaria `run()` 5 s. Teste de rajada.
6. App: doc `gain_safety.rs` (FIX-08 p2); catálogo/doctor "GTCRN: HPF fixo 80 Hz, banda alta
   = gate sem síntese".

Descartar nesta rodada: FastEnhancer (estado global + AVX2), efeitos de canto, LocalVQE
conjunto, FIX-09 completo (portas de saúde = mudança de ABI). AEC SPA SpeexDSP = projeto
próprio (PR-04). Verificar: o split-band herdou o crossover fictício do GTCRN?
