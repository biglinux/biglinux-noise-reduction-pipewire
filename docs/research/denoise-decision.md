# DENOISE-DECISION (recomendação, rev 2026-09-09)

> **Status:** dated research snapshot. Measurements and candidate architectures here are evidence, not the current runtime contract. Verify current behavior in the source code and `docs/architecture.md` before implementing changes.

Recomendação baseada nas medições de `denoise-benchmark.md`. É **contingente** a 3
ações de fechamento listadas no fim (port ORT, qualidade completa, experimento ao
vivo). O que já é sólido: **viabilidade por hardware** e **runtime**.

## Arquitetura recomendada (multi-tier, seleção automática)

```
RUNTIME ÚNICO: ONNX Runtime (ort). OpenVINO removido.
  motivo: paridade de qualidade (128 dB SDR) e, em CPU antiga sem AVX2,
  OpenVINO é 3–7× mais lento (inviável ao vivo); ORT ~50 MB mais leve.

PADRÃO (auto por auto-medição da máquina):
  CPU moderna (ex.: i5-class)  → DPDFNet2-SB (ORT)   [full-band, melhor denoise]
  CPU antiga viável            → DPDFNet baseline-SB (ORT)  [se qualidade > GTCRN]
  PISO universal / fallback    → GTCRN-DNS3 (ORT)    [roda em qualquer x86-64]

MODO OPCIONAL (qualidade máxima, só onde couber): DPDFNet2-HR (ORT).

NUNCA como padrão em hw fraco: DPDFNet-HR/v8 (DEAD no i3), qualquer OpenVINO.
```

## Campos (§53)

```
SOLUÇÃO PADRÃO (CPU moderna):
  modelo:            DPDFNet2 split-band (16 kHz rodando full-band 48 kHz)
  engine:            ONNX Runtime (CPU EP), 1 thread
  linguagem:         Rust (plugin LADSPA) + ort
  formato dos pesos: ONNX/.ort (~10 MB)
  sample rate:       48 kHz nativo (sem resampler)
  CPU mínima:        x86-64 baseline (roda onde o ORT roda)
  ISA:               sem exigência acima do baseline do ORT
  threads:           1
  qualidade:         ≈ DPDFNet2-HR na percepção do usuário; > GTCRN (a confirmar
                     em VoiceBank+DEMAND)
  CPU:               i5 p99 20% do hop; i3 p99 77% (MARGINAL) — não é o default do i3
  PSS/RSS:           ~85 MB
  p99:               i5 20% · i3 77% (do hop de 10 ms)
  latência algorítmica: 70 ms (split-band)
  XRUNs:             0 no i5; a confirmar ao vivo no i3
  tamanho instalado: ~10 MB pesos + runtime ort
  licença:           DPDFNet Apache-2.0 (confirmar pesos/notices)

FALLBACK (CPU antiga / piso):
  modelo:  GTCRN-DNS3 (16 kHz) — ou DPDFNet baseline-SB se a qualidade justificar
  engine:  ONNX Runtime, 1 thread
  CPU:     i3 p99 26% (GTCRN) / 34% (baseline-SB) do hop → folga confortável
  XRUNs:   0 medido ao vivo (GTCRN no i3)
  latência: GTCRN 44 ms · baseline-SB 70 ms

SELEÇÃO:
  ISA gate por engine + auto-medição (measure-model na própria máquina) +
  step-down em XRUN ao vivo. Sem taxonomia de CPU embarcada.
```

## Fallback por ISA (§42/§51)
Hoje há **uma** regra de ISA relevante: um eventual FastEnhancer-C exigiria
AVX2+FMA3+F16C (não roda no i3) → seria tier-i5 auto-desativado no i3. Todos os
modelos ORT rodam no baseline. A máquina se auto-mede e escolhe o melhor cujo
p99 medido ≤ 50%; senão cai pro GTCRN.

## Ações pra fechar a decisão
1. **Portar DPDFNet OpenVINO→ORT — FEITO e validado.** Paridade 128 dB (inferência)
   / plugin corr 0.9993 (diferença = jitter de scheduling, não runtime); roda no
   i3 sem AVX2; corrige o crash de teardown (measure-model do DPDFNet volta). −50 MB
   RAM, runtime unificado.
2. **Experimento sob carga — FEITO** (measure-model paced + 1 core ocupado no i3):
   baseline-SB 43% (SHIPS sob carga); dpdfnet2-SB cai atrás sob contenção (DEAD);
   HR DEAD. **Confirma:** i3 fica em baseline-SB/GTCRN; dpdfnet2-SB é tier i5. (Um
   run de 30 min ao vivo no PipeWire com navegador continua recomendado como
   aceitação final, mas o resultado paced já é conclusivo.)
3. **FastEnhancer-Medium — testado; wrapper LADSPA feito.** RTF i5 0.089 (~9% do
   hop), MIT, pesos 565 KB, zero-heap. Wrapper `fastenhancer_medium_mono` com gate
   CPUID (passthrough bit-exato no i3, sem crash), engine-global-único, paridade
   corr 1.0 vs o engine. Qualidade: PESQ publicado 3.06 (entre GTCRN 2.53 e
   DPDFNet-HR 3.12), DNSMOS o melhor do conjunto, escuta boa (leve rolloff de
   agudos em voz limpa). **Candidato ao default de CPU moderna**, competindo com
   dpdfnet2-SB. Precisa AVX2+FMA+F16C → auto-desativado no i3.
4. **Falta:** ABX cego FastEnh-Medium vs dpdfnet2-SB no i5 (decide o tier moderno)
   e baseline-SB vs GTCRN (piso i3); PESQ-WB local no VoiceBank (harness pronto,
   P.862 lento); no app, o filtro por auto-medição + step-down por XRUN (Passo 5).

## Não recomendado agora
OpenVINO (cortado); FastEnhancer como padrão universal (piso de ISA).

**AOT/nativo/INT8 do DPDFNet2-HR — testado e descartado** (ver
`dpdfnet-native-feasibility.md`; auditado por 2º revisor). Medido no i5: tract
1,75× + lento; INT8 dinâmico 1,55× + lento; INT8 estático **corrigido** (U8S8,
só Gemm, calibração real) só ~9% + rápido (1,41 vs 1,55 ms) e regride pra 14 dB em
áudio fora da calibração. O gargalo está *dentro* dos kernels do ORT (banda das
GEMV memory-bound + laço da GRU), não em dispatch → nenhum gerador C genérico bate.
O esquema de quant do FastEnhancer é o **mesmo** W8A8 do ORT; a vantagem dele é o
engine C à mão (78 vs 14 GMAC/s) fazendo 2× os MACs — não replicável por conversão.
HR fica como tier de qualidade do i5 em ORT-FP32 (RTF 0,16 = 1,8× o FastEnhancer,
não "muito mais pesado").
