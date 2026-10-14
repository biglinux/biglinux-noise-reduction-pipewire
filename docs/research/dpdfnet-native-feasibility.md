# DPDFNet2-HR — viabilidade de AOT/nativo/INT8 (medido 2026-09-10)

> **Status:** dated research snapshot. Measurements and candidate architectures here are evidence, not the current runtime contract. Verify current behavior in the source code and `docs/architecture.md` before implementing changes.

Pergunta do usuário: *converter o `.onnx` do DPDFNet2-HR em C/Rust (AOT) ou
quantizar deixaria ele tão leve quanto o FastEnhancer?*

**Resposta medida: não.** Todo caminho automático (interpretador pure-Rust, AOT
por kernels genéricos, e quantização INT8) roda **mais lento** que o ORT-FP32 atual,
e o INT8 ainda **degrada a qualidade**. O ORT (MLAS) já é o melhor runtime pra este
modelo.

## O modelo (favorável a AOT no papel, mas…)
- opset 17, IR 8, **grafo totalmente estático** (in `[1,1,481,2]`+`[56436]`, sem
  Loop/If/Scan/Shape). 493 nós, 26 tipos.
- 4 GRU **bidirecionais** (hidden 64, intra-chunk), 30 Conv, 22 Gemm, 14 MatMul,
  8 LayerNorm. Pesos FP32 ~10,2 MB.
- No papel isso favorece AOT (estático, sem controle de fluxo). Na prática as ops
  de math são **pequenas** (largura 64) — INT8/VNNI não tem massa pra ganhar, e o
  overhead de quant domina.

## Medições (i5-13400, 1 thread, ORT 1.28, streaming realimentando o state)
Entrada = STFT de fala real (nfft 960). SDR = fidelidade vs FP32. Números da 1ª
rodada corrigidos pela auditoria (subagent): o static QDQ inicial dava 0 dB por
**bug de config** (todas as 457 ativações quantizadas, incl. GRU/LayerNorm; 43 Q
com scale 1.0 por calibrar com `state_in`=zeros; scale do `spec` zerava a entrada;
ativação int8 assimétrica que o x86 não suporta). Refeito certo abaixo — e mesmo
certo, perde.

| caminho | ms/inf (mean) | vs ORT-FP32 | SDR in-dist / out-dist |
|---|---|---|---|
| **ORT FP32 (atual)** | **1,55–1,60** | — (baseline) | — |
| `quant_pre_process` + onnxsim (só limpa grafo) | 1,54 | ~igual | idêntico |
| tract 0.22 (pure-Rust, `-O`, funde grafo) | 5,06 | **1,75× + lento** | 132 dB |
| ORT INT8 dinâmico (W8A8, esquema do FastEnhancer) | 2,68–2,79 | 1,55× + lento | 14–28 dB |
| ORT INT8 estático **corrigido** (U8S8, per-channel, **só Gemm**, calibração c/ state real) | **1,41–1,43** | ~9% + rápido | **39 dB / 14 dB** |
| ORT INT8 estático, Conv only / Conv+Gemm | 1,57–1,85 | igual/+lento | 17 dB |
| FP16 (`transformers.float16`, keep_io_types) | 1,91 | + lento | 28 dB |

FastEnhancer-Medium, referência: ~0,59 ms/inf, RTF **0,089**.

## Onde o tempo FP32 realmente vai (perfil ORT, 1 thread)
23,05 MMAC/frame (GRU 8,65M · MatMul 6,8M · Conv 5,5M · Gemm 2,1M) em ~1,6 ms =
**14 GMAC/s** (só 15–20% do pico FP32 de 1 core). Gemm 20% (as 10 GEMVs 768×256 da
GRU-cell, M=1, são **memory-bound**: ~786 KB de peso a ~20 GB/s), GRU 19%, Conv 8%,
e o glue (Reshape/Transpose/Add/memcpy em tensores de 120 KB) mais ~40%. **Piso de
dispatch medido = 0,24 µs/nó → ~0,11 ms pros 476 nós** (pequeno). Ou seja: **não é
dispatch-bound** — a perda está *dentro* dos kernels do ORT (banda das GEMV, laço da
GRU, cópias de glue), não na travessia do grafo.

## Por que perde
- **Melhor teto de PTQ no ORT ≈ 1,41 ms vs 1,55 FP32 (~9%)** e a calibração estática
  **não generaliza** (39 dB no clip calibrado, **14 dB em áudio fora da calibração**)
  — regressão auditável. Não compensa.
- tract e FP16 são mais lentos no CPU; Conv-INT8 é mais lento e 17 dB.
- **O esquema de quantização do FastEnhancer é o MESMO** W8A8 dinâmico que o ORT já
  faz (README dele: *"Training-free. Post-training quantization only… ORT
  DynamicQuantizeLinear semantics"*). Nada exótico pra copiar.
- A vantagem do FastEnhancer é o **runtime**: engine C escrito à mão pra um modelo
  fixo — microkernels int8 a **85–93% do pico da ISA**, Winograd, transpose+quantize
  fundidos, zero dispatch → **~78 GMAC/s vs 14 do ORT**. E ele faz **2× os MACs**
  (46 MMAC/frame) e ainda roda em 0,59 ms. O gap de 4–5× é **eficiência de engine**,
  não o esquema de quantização nem o tamanho do modelo.

## Conversores automáticos onnx→C/Rust — checados
- **onnx2c** (kraiskil): os arquivos de operador **não têm GRU nem ReduceSum** (tem
  LSTM, não GRU). Nosso modelo tem 4 GRU + 6 ReduceSum → onnx2c **não compila o
  modelo**. Muro empírico, não predição.
- **tract** (melhor engine pure-Rust, TEM GRU): compila, mas **1,75× mais lento** que
  o ORT (medido acima). Interpretador otimizado ainda perde pro MLAS.
- **Lele/Burn/deepC/emx-onnx-cgen**: emitem kernels ingênuos e/ou GRU imaturo; o
  datapoint público de GRU bidirecional no CPU (C++ TorchScript 120 ms vs ORT 40 ms,
  3× + lento) e a análise de perfil (gargalo = GEMV memory-bound + laço da GRU, que
  o MLAS já faz perto do ótimo) preveem derrota. Nenhum vale um teste.
- **DeepFilterNet3.c.wasm** (kdrkdrkdr): NÃO é conversor — é **port C escrito à mão**
  de um modelo **diferente** (DeepFilterNet3), FP32, GRU/Conv/matvec custom + KissFFT,
  sem runtime (~1 ms M2). Confirma que a via "engine à mão" funciona, mas é um port
  específico de outro modelo — **não produz um DPDFNet2-HR mais leve**. (Só seria
  relevante se trocássemos o modelo pra DFN3, que já é candidato via ORT e não foi o
  preferido do usuário.)

## O único caminho que restaria (não recomendado)
Escrever um engine nativo à mão pro modelo fixo do DPDFNet (FP32, depois W8A8), à la
FastEnhancer. O perfil aponta onde: laços da GRU + 10 GEMVs = 40% do tempo, glue
~40%. Estimativa: **0,5–0,8 ms FP32 com fidelidade exata**. Custo: **semanas, um
projeto** — não um experimento. Sem retorno claro (ver reenquadramento).

## Conclusão prática
Não há tier onde "deixar o HR leve como o FastEnhancer" destrave algo:
- **i5**: o HR em ORT-FP32 já **roda folgado** (~1,55–1,6 ms; **RTF 0,16** vs FE
  0,089 → só **1,8× mais pesado**, não "muito"). Já é o tier de qualidade e já SHIPS.
- **i3**: o alvo do FastEnhancer (AVX2+FMA+F16C) **nem existe** ali; HR também não é
  o default. O i3 fica em GTCRN/baseline-SB de qualquer forma.

**Recomendação: encerrar a linha AOT/nativo/quantização.** Manter DPDFNet2-HR como
opção de qualidade do i5 em **ORT-FP32** (o mais natural no ouvido do usuário), com
FastEnhancer como alternativa mais leve do i5. Bate com o gate do Passo 6 do plano
("se o tempo está em GEMM/MLAS, nenhum gerador C bate; parar").
