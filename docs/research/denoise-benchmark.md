# DENOISE-BENCHMARK — resultados medidos (rev 2026-09-09)

> **Status:** dated research snapshot. Measurements and candidate architectures here are evidence, not the current runtime contract. Verify current behavior in the source code and `docs/architecture.md` before implementing changes.

Execução do plano de `onnx-runtime-benchmark.md`. Foco desta rodada:
**triagem de viabilidade** (qual combinação modelo+runtime roda ao vivo) nas duas
máquinas disponíveis, mais **runtime** (ORT vs OpenVINO) e um cross-check de
qualidade. Fases de qualidade completa (VoiceBank+DEMAND), escuta cega (ABX) e
compiladores AOT/engine nativa ficam marcadas como pendentes (ver fim).

## Hardware

| máquina | CPU | ISA | RAM | papel |
|---|---|---|---|---|
| dev | i5-13400 | AVX2 + AVX-512 | — | classe "CPU moderna" |
| laptop `192.168.1.62` | i3-2375M (Sandy Bridge, 2011) | **só AVX** (sem AVX2/FMA3/F16C) | 3.7 GB | classe "CPU antiga" |

Sem hardware AVX-VNNI / AVX-512-VNNI → esses tiers ficam **não testados**. Não
extrapolar de n=2 para "classe AVX2" vs "classe AVX"; tratar como 2 tiers de produto.

## Metodologia

- Inferência ORT (`onnxruntime-cpu` 1.29), **1 thread** (`intra=inter=1`,
  sequencial) — RT usa 1 thread sempre. Governor `performance`; no i3 **turbo OFF**
  (`intel_pstate/no_turbo=1`; base=max=1.5 GHz, freq estável 1497 MHz, sem throttle,
  51 °C).
- Janela ≥60 s no i3 (≥2 k–20 k inferências), ≥30 s no i5. Percentis reais.
- Custo = **latência de inferência / hop do modelo** (DPDFNet hop 480/48k = 10 ms;
  GTCRN hop 768/48k = 16 ms). p99 ≤ 50% = **SHIPS**; 50–80% = **MARGINAL**; >80% ou
  não-carrega = **DEAD**. (Inferência pura; o plugin completo soma STFT+HF+OLA ~10–20%.)
- Paridade numérica ORT vs OpenVINO: mesmo `dpdfnet2.onnx`, 400 frames com estado
  recorrente carregado.

## Resultado 1 — Paridade de runtime (qualidade não muda com o runtime)

ORT vs OpenVINO, mesmo modelo/entrada: **correlação 1.000000, SDR 128.3 dB**
(bit-idêntico ao piso do float). **O runtime não altera a qualidade do denoise.**

## Resultado 2 — TRIAGEM (p99 como % do hop do modelo, ORT 1-thread)

### i3-2375M (1.5 GHz, no_turbo)
| modelo | p50 | p95 | p99 | max | RSS | veredito |
|---|---|---|---|---|---|---|
| GTCRN-DNS3 | 22% | 24% | 26% | 34% | 71 MB | **SHIPS** |
| DPDFNet baseline-SB | 29% | 31% | 34% | 52% | 84 MB | **SHIPS** |
| DPDFNet2-SB | 67% | 73% | 77% | 111% | 86 MB | **MARGINAL** |
| DPDFNet2-HR (48k) | 96% | 103% | 107% | 124% | 87 MB | **DEAD** |
| DPDFNet8-HR (48k) | 263% | 278% | 281% | 330% | 100 MB | **DEAD** |

### i5-13400
| modelo | p50 | p99 | max | RSS |
|---|---|---|---|---|
| GTCRN-DNS3 | 3% | 6% | 29% | 71 MB |
| DPDFNet baseline-SB | 4% | 7% | 34% | 84 MB |
| DPDFNet2-SB | 11% | 20% | 81% | 85 MB |
| DPDFNet2-HR | 15% | 21% | 96% | 86 MB |
| DPDFNet8-HR | 45% | 60% | 131% | 98 MB |

## Resultado 3 — Runtime no hardware fraco: ORT vs OpenVINO (i3)

Mesmos modelos, inferência/hop de 10 ms:

| modelo | ORT | OpenVINO |
|---|---|---|
| baseline | 29% | **194%** |
| dpdfnet2 | 68% | **229%** |
| dpdfnet2-HR | 96% | **260%** |

**OpenVINO é inviável no i3** (194–260% do hop = XRUN garantido): no Sandy Bridge
sem AVX2, o fallback `SNIPPETS_MODE=DISABLE` (imposto pelo GRU) desaba, 3–7× mais
lento que ORT. RSS ORT ~85 MB vs OpenVINO ~135 MB. No i5, ORT ≈ OpenVINO em CPU.
→ **OpenVINO cortado. DPDFNet precisa rodar via ORT pra servir hardware antigo.**

## Resultado 4 — Qualidade (cross-check DNSMOS P.835; evidência fraca)

DNSMOS numa amostra sintética (voz+ruído rosa), maior=melhor:

| modelo | SIG | BAK | OVRL |
|---|---|---|---|
| entrada ruidosa | 3.46 | 3.17 | 2.87 |
| GTCRN-DNS3 | 3.17 | 4.29 | 3.14 |
| baseline-SB | 2.86 | 4.83 | 2.97 |
| dpdfnet2-SB | 2.70 | 4.67 | 2.83 |
| dpdfnet2-HR | 2.57 | 4.56 | 2.72 |

**Ressalva forte:** amostra única, sintética, sem referência limpa → DNSMOS pouco
confiável aqui. Padrão observável: DPDFNet suprime mais ruído (BAK↑) ao custo de
SIG↓; GTCRN é mais gentil. **Conflita** com o ladder PESQ do mantenedor (GTCRN 2.53
< DPDFNet-HR 3.12 < DFN3 3.34, em `quality.rs`) e com a escuta do usuário (HR o
melhor em ruído real; SB ≈ HR). **A qualidade real exige VoiceBank+DEMAND + PESQ/
STOI/SI-SDR** (pendente) — não decidir por este número.

Evidência de qualidade já estabelecida nesta sessão (medições espectrais + escuta):
o split-band **anchor** (banda alta guiada pela máscara da rede em 5.5–7 kHz)
iguala o HR nas fricativas e remove o sopro de onset melhor que o HR; SB e HR
soaram equivalentes ao usuário.

## Resultado 5 — Port DPDFNet OpenVINO→ORT (feito) + plugin completo no i3

O port foi implementado (dpdfnet-ladspa: `engine.rs` roda `ort::Session`,
`build.rs` embute o `.onnx`+init_state, sem IR). Validação:
- **Paridade:** saída do plugin ORT vs OpenVINO SDR 28.7 dB (corr 0.9993); o
  jitter de scheduling do worker é ~34 dB nos DOIS runtimes, então a diferença é
  o agendamento, não o runtime (inferência pura é 128 dB idêntica). Qualidade
  preservada.
- **Compat:** um `.so` genérico construído no i5 **roda no i3** (Sandy Bridge sem
  AVX2) — onnxruntime despacha por CPU em runtime.
- **Corrige o crash** de teardown do OpenVINO: `measure-model` do DPDFNet agora
  sai 0 (antes SIGABRT) → o tier automático volta a medir DPDFNet.

**Plugin completo no i3** (measure-model paced, quantum 960 = deadline 20 ms;
inferência no worker/2º core, concorrente ao STFT no audio thread):

| modelo | ocioso | 1 core ocupado (carga) | veredito |
|---|---|---|---|
| baseline-SB | 38.7% | **43%** | **SHIPS mesmo sob carga** |
| dpdfnet2-SB | 45.1% | **indisponível** (worker cai atrás) | SHIPS ocioso, **DEAD sob carga** |
| dpdfnet2-HR | indisponível | indisponível | **DEAD** |

O ORT + os 2 cores tornam dpdfnet2-SB viável **ocioso** (era DEAD no OpenVINO),
mas sob contenção real (navegador/videoconf roubando um core) o worker perde o
core e cai atrás → não é default seguro no i3. baseline-SB aguenta a carga.

## Resultado 6 — FastEnhancer-Medium + faster-enhancer.c (candidato Fase 0)

Clonado (MIT, pesos `fe.q8` 565 KB inclusos, W8A8 INT8, 511 k params, sem deps,
zero-heap após init), compilado no i5 (CMake, tier **avxvnni**).
- **CPU (i5):** RTF racing **0.089** (1.12 s p/ 12.6 s de áudio) ≈ 9% do hop de
  6.67 ms — muito leve. Hop 320 amostras/48 kHz, latência algorítmica 14.67 ms.
- **Qualidade (DNSMOS, amostra única, fraco):** SIG **3.32** (melhor preservação
  de fala do conjunto), BAK 4.03, **OVRL 3.16** (o maior) — consistente com o
  PESQ publicado 3.06. DPDFNet suprime mais ruído (BAK↑) mas com SIG menor.
- **Incompatível com o i3:** exige **AVX2+FMA3+F16C**, sem fallback SSE/escalar;
  o i3 (só AVX) não roda. → seria **tier de CPU moderna**, auto-desativado no i3.

### Wrapper LADSPA (feito)
Escrito `fe_ladspa.c` (LADSPA inline, linka `libfe.a` PIC): label
`fastenhancer_medium_mono`, portas Input/Output/Enable/Strength, ring de 320,
FIFO wet/dry alinhada (latência 1024 = frame+STFT), limiter no fim. Guardas:
- **Gate CPUID** (`__builtin_cpu_supports` avx2+fma+f16c): sem eles → **passthrough**
  (nunca executa instrução ilegal). Verificado no i3: saída == entrada, **max|Δ| 0.00**,
  exit 0 (auto-desativa seguro em CPU velha).
- **Engine global único**: 1ª instância inicializa, 2ª passa direto (fe é global).
- **Paridade** vs `fe_run_file`: corr **1.0000** (lag 1024). Instalado no i5.
Mantido **LADSPA** (não SPA nativo): compõe na filter-chain com GTCRN/DPDFNet/EQ/gate,
testável fora do PipeWire, portável; worker-thread (se preciso) é ortogonal ao formato.

### Qualidade (evidência)
- **PESQ-WB publicado** (FastEnh-Medium, VoiceBank+DEMAND, o mesmo dataset): **3.06**
  — vs ladder do mantenedor GTCRN-DNS3 2.53, DPDFNet-HR 3.12, DFN3 3.34. FastEnh
  fica **entre GTCRN e DPDFNet-HR**, perto do HR.
- **DNSMOS** (amostra): melhor OVRL (3.16) e SIG (3.32) do conjunto.
- **Escuta:** bom em ruído real; leve rolloff de agudos (~1-2 dB) em voz limpa; os
  "ruins" foram artefatos de teste (clipping a 0 dBFS, ruído rosa sintético).
- **Nota:** VoiceBank+DEMAND baixado e venv pronto (`scripts/calibrate/.venv` +
  pesq/pystoi), mas o PESQ P.862 puro-Python é lento (~40 s/arquivo) — a corrida
  completa não fechou nesta sessão; o número publicado (3.06) é sobre o mesmo dataset.

## Conclusão da triagem

- **Runtime único: ONNX Runtime.** OpenVINO cortado (inviável sem AVX2, ~50 MB
  mais pesado, crash de teardown). Port DPDFNet→ORT **feito e validado**.
- **Piso universal:** GTCRN-DNS3 (ORT) — 26% p99 no i3, roda em qualquer x86-64.
- **i3-class (fraco, 2 cores), sob carga real:** **GTCRN ou baseline-SB** (43%
  sob carga). dpdfnet2-SB/HR **não** (worker cai atrás sob contenção).
- **i5-class:** DPDFNet2-SB / HR (ORT) — folgados.
- O **step-down por XRUN** (auto-medição ociosa a 45% não pega a falha sob carga
  do dpdfnet2-SB no i3; o guard de runtime pega) é o mecanismo certo — validado
  por este resultado.

## Pendências (requerem tempo/humano)
- **Qualidade completa** VoiceBank+DEMAND (PESQ/STOI/eSTOI/SI-SDR) — precisa deps
  (`pesq`/`pystoi`) + download ~250 MB. (DNSMOS de amostra única já feito, fraco.)
- **Escuta cega ABX** (≥20 trials, binomial) — humano — pra decidir baseline-SB
  vs GTCRN e dpdfnet2-SB vs HR.
- **App:** filtro de modelo por auto-medição + step-down por XRUN (Passo 5/6 do
  plano) — decisão de UI.
- **Empacotamento:** `prepare_model.py` ainda usa OpenVINO só pra extrair o
  init_state (build-time); porta pra onnxruntime pra dropar o makedep.
- **AOT (onnx2c/Lele) e engine nativa DPDFNet** — só se `perf` de um modelo
  MARGINAL apontar overhead, não GEMM (sem gatilho: no i3 o custo é a inferência,
  não overhead).
