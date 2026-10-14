# BigLinux — escolha do modelo e engine padrão para redução de ruído neural

> **Status:** dated research snapshot. Measurements and candidate architectures here are evidence, not the current runtime contract. Verify current behavior in the source code and `docs/architecture.md` before implementing changes.

**Revisão:** 2026-09-09  
**Objetivo:** orientar um agente de IA a determinar, por testes reproduzíveis em Linux, qual combinação **modelo + engine/runtime** deve ser o padrão do BigLinux para redução de ruído de microfone em tempo real.

> **Mudança importante em relação à versão anterior deste documento:**  
> não avaliar apenas qual runtime executa melhor os ONNX atuais.  
> **FastEnhancer-Medium + faster-enhancer.c deve ser tratado como candidato de prioridade máxima a substituir inclusive o modelo atual e tornar-se a solução padrão do BigLinux**, caso vença em qualidade, CPU, memória, latência, compatibilidade e estabilidade.

Além disso, esta revisão adiciona uma terceira linha de pesquisa: **compilar modelos ONNX antecipadamente para C/Rust/código nativo** ou criar uma implementação especializada. Se o BigLinux decidir investir em uma implementação própria especializada em C ou Rust, **o alvo definido para esse trabalho é DPDFNet**, não GTCRN.

---

# 1. Problema real a resolver

O BigLinux precisa escolher a melhor solução de redução de ruído **ao vivo**, não apenas o runtime ONNX mais rápido em benchmark isolado.

A decisão possui agora **duas dimensões independentes**:

## A. Qual é o melhor runtime para os modelos ONNX que já usamos?

Comparar, pelo menos:

### Runtimes / engines
- ONNX Runtime atual;
- ONNX Runtime Minimal / Reduced Operator Config;
- OpenVINO;
- RTen;
- tract.

### Compilação AOT / geração de código
- **Lele — ONNX → Rust especializado**;
- **emx-onnx-cgen — ONNX → C determinístico**;
- **onnx2c — ONNX → C**;
- **Burn ONNX — ONNX → código Rust/Burn**;
- IREE — compilação antecipada;
- ONNX-MLIR — LLVM/código nativo.

### Runtimes e ports especializados para áudio
- **DeepFilterNet3.c.wasm / engine C nativa**;
- **Vokra**, como runtime Rust speech-first experimental;
- ncnn/MNN apenas quando o modelo puder ser convertido corretamente.

Esses grupos respondem a perguntas diferentes. Um gerador ONNX→C/Rust não deve ser avaliado como se fosse apenas mais um runtime genérico.

## B. Qual é a melhor solução completa de redução de ruído?

Aqui **não é obrigatório manter o modelo atual**.

Comparar:

```text
modelo atual + ONNX Runtime
modelo atual + ONNX Runtime Minimal
modelo atual + OpenVINO
modelo atual + RTen
modelo atual + tract
modelo atual compilado com Lele
modelo atual compilado para C, se compatível
modelo atual compilado via ONNX-MLIR/IREE
...
versus
FastEnhancer-Medium + faster-enhancer.c
versus
DeepFilterNet3 + implementação C especializada
versus
DPDFNet + melhor runtime disponível
versus
DPDFNet + implementação C/Rust especializada, se desenvolvida
```

Se `FastEnhancer-Medium + faster-enhancer.c` entregar melhor resultado global, ele pode e deve ser recomendado como padrão, mesmo que isso torne desnecessário usar ONNX para o filtro padrão.

Se **DPDFNet** for superior em qualidade, mas seu custo através de um runtime genérico for alto, o agente deve avaliar seriamente uma implementação especializada **DPDFNet → C ou Rust**, usando os mesmos princípios que fizeram `faster-enhancer.c` reduzir drasticamente o overhead de inferência.

---

# 2. Prioridades do BigLinux

Ordem geral de importância:

1. qualidade perceptiva da voz;
2. nenhuma regressão grave ou artefato;
3. cumprir deadline de áudio com grande margem;
4. p99/pior caso de latência;
5. ausência de XRUNs/glitches;
6. CPU;
7. RAM/PSS;
8. consumo energético;
9. compatibilidade com computadores comuns;
10. estabilidade em execução longa;
11. tamanho instalado e dependências;
12. facilidade de empacotamento/manutenção;
13. licença compatível com distribuição.

Não escolher pela média de tempo de inferência apenas.

---

# 3. Baselines já existentes

O agente já testou:

- **ONNX Runtime**
- **OpenVINO**

Preservar esses resultados quando forem comparáveis, mas repetir os testes necessários para que todos os candidatos usem exatamente:

- os mesmos arquivos de áudio;
- o mesmo sample rate;
- os mesmos frames;
- o mesmo hardware;
- o mesmo governor;
- a mesma carga de sistema;
- as mesmas métricas;
- o mesmo período de warm-up;
- o mesmo modo de streaming.

Também adicionar uma baseline que ainda é essencial:

## ONNX Runtime Minimal

Não comparar novas alternativas apenas contra a distribuição completa de ONNX Runtime.

Criar uma build reduzida contendo somente:

- operadores necessários pelos modelos do BigLinux;
- tipos necessários;
- execution providers realmente usados;
- recursos efetivamente necessários.

Testar também, quando aplicável:

- formato `.ort`;
- otimizações antecipadas;
- reduced operator config;
- build sem componentes não usados;
- número de threads controlado;
- arena/memory pattern configurados para o caso real.

---

# 4. PRIORIDADE MÁXIMA — FastEnhancer-Medium + faster-enhancer.c

Projetos:

```text
FastEnhancer:
https://github.com/aask1357/fastenhancer

faster-enhancer.c:
https://github.com/kdrkdrkdr/faster-enhancer.c

paper do runtime:
https://arxiv.org/abs/2607.25350
```

## 4.1 O que é

`faster-enhancer.c` **não é um runtime ONNX genérico**.

Ele é uma implementação C/SIMD altamente especializada do:

```text
FastEnhancer-Medium
48 kHz
streaming speech enhancement
```

A implementação usa os pesos publicados do FastEnhancer e mantém a arquitetura do modelo; o ganho vem da especialização do runtime, quantização e kernels.

Portanto, para o BigLinux, o candidato deve ser denominado:

> **FastEnhancer-Medium + faster-enhancer.c**

e avaliado como **solução completa candidata a filtro padrão**, não apenas como exemplo de otimização.

---

# 5. Características do faster-enhancer.c que precisam ser verificadas no BigLinux

No estado pesquisado em 2026-09-09, o projeto declara:

- C/SIMD;
- sem framework de inferência;
- sem dependências externas de runtime;
- CPU only;
- inferência single-thread por frame;
- FastEnhancer-Medium 48 kHz;
- mono;
- streaming causal;
- 320 amostras por chamada;
- 6,67 ms de áudio por frame;
- zero look-ahead;
- atraso fixo de alinhamento do STFT de 704 amostras, aproximadamente 14,67 ms;
- pesos W8A8 INT8;
- `511.754` parâmetros;
- blob de pesos de `565.108 bytes`;
- estado principal informado de aproximadamente `432.384 bytes`;
- nenhuma alocação de heap depois da inicialização;
- kernels específicos por ISA;
- output byte-idêntico entre tiers da mesma família arquitetural segundo os testes do projeto.

Esses números devem ser tratados como **referência externa**, não como resultado do BigLinux.

Reproduzir tudo em Linux x86-64.

---

# 6. Atenção: requisitos de CPU do faster-enhancer.c

No x86-64 atual, o projeto utiliza tiers:

```text
AVX2
AVX-VNNI
AVX-512 VNNI
```

O tier x86 mínimo exige:

```text
AVX2 + FMA3 + F16C
```

Não existe fallback escalar/SSE4.1 no runtime atual.

Isso é uma questão crítica para decidir se ele pode ser o **padrão universal** do BigLinux.

## Testar

Criar uma matriz de CPUs reais:

- CPU antiga que ainda é suportada pelo BigLinux;
- Intel com AVX2;
- AMD com AVX2;
- Intel moderno;
- AMD Zen 4 ou posterior quando disponível;
- CPUs com AVX-VNNI;
- CPUs com AVX-512 VNNI quando disponíveis.

Detectar features via CPUID.

### Possíveis resultados

Se FastEnhancer + faster-enhancer.c for claramente vencedor, mas não funcionar em CPUs antigas:

**não descartar automaticamente a solução.**

Avaliar arquitetura de seleção:

```text
CPU compatível
    → FastEnhancer-Medium + faster-enhancer.c

CPU sem AVX2/FMA3/F16C
    → backend/modelo fallback
```

O padrão lógico do produto pode ser FastEnhancer, mesmo existindo fallback para hardware legado.

---

# 7. Benchmark publicado do faster-enhancer.c NÃO substitui benchmark Linux

O trabalho publicado reporta, para a mesma rede FastEnhancer-Medium:

```text
ONNX Runtime FP32: RTF ~0,230
faster-enhancer.c: RTF ~0,069
```

no Apple M2 em sua metodologia, aproximadamente `3,3×` de diferença.

Isso demonstra que a estratégia merece prioridade, mas:

> **não extrapolar o número de 3,3× para Linux x86-64.**

O próprio projeto informa que os tiers x86 compilam e passam testes funcionais, mas os números principais publicados de desempenho foram obtidos em ARM.

A decisão do BigLinux deve vir dos nossos próprios testes.

---

# 8. Qualidade publicada do faster-enhancer.c

O projeto comparou a versão quantizada com o grafo FP32 do **mesmo FastEnhancer-Medium** em 824 utterances VoiceBank-DEMAND a 48 kHz.

Valores declarados:

| Implementação | PESQ | STOI | SNR |
|---|---:|---:|---:|
| FastEnhancer FP32/ONNX | 3,060 | 0,9512 | 19,43 dB |
| faster-enhancer.c INT8 | 3,054 | 0,9509 | 19,35 dB |

Diferença reportada:

```text
PESQ: -0,006
SNR:  -0,08 dB
```

Isso indica que o port INT8 acompanha muito de perto o modelo FP32 **naquele conjunto de testes**.

Mas isso não responde à pergunta principal do BigLinux:

> FastEnhancer é melhor ou pior que os modelos de denoise que atualmente oferecemos?

Essa comparação precisa ser feita por nós.

---

# 9. FastEnhancer deve ser avaliado como MODELO, não só como engine

O repositório oficial possui diferentes tamanhos do FastEnhancer.

Para 48 kHz, o projeto lista aproximadamente:

| Variante | Parâmetros | MACs |
|---|---:|---:|
| Tiny | 28 K | 177 M |
| Base | 101 K | 750 M |
| Small | 207 K | 1,822 G |
| Medium | 512 K | 8,0 G |
| Large | 1.132 K | 32,4 G |

`faster-enhancer.c` está especializado atualmente no **Medium 48 kHz**.

## Primeira obrigação

Testar diretamente:

```text
FastEnhancer-Medium + faster-enhancer.c
```

contra a melhor solução atual do BigLinux.

## Segunda obrigação

Se Medium for excelente em qualidade, mas pesado demais para parte das CPUs, investigar se faz sentido:

- FastEnhancer Small;
- FastEnhancer Base;
- eventualmente Tiny;

e medir a relação:

```text
qualidade / CPU / RAM / latência
```

## Terceira obrigação

Se Base ou Small forem suficientes em qualidade, estudar se as técnicas do `faster-enhancer.c` podem ser adaptadas para essas variantes.

Não fazer esse port antes de medir a necessidade.

---

# 10. O teste principal passa a ter duas tabelas diferentes

## Tabela A — comparação de RUNTIME com o MESMO modelo

Serve para responder:

> Qual runtime executa melhor cada ONNX que já usamos?

Exemplo:

```text
Modelo atual X:
  ORT
  ORT Minimal
  OpenVINO
  RTen
  tract
  IREE
```

Aqui a saída deve ser numericamente equivalente dentro da tolerância apropriada.

---

## Tabela B — comparação de SOLUÇÃO COMPLETA

Serve para responder:

> Qual filtro deve ser o padrão do usuário?

Exemplo:

```text
Modelo atual X + melhor runtime
Modelo atual Y + melhor runtime
FastEnhancer-Medium + faster-enhancer.c
FastEnhancer-Small + backend disponível, se testado
FastEnhancer-Base + backend disponível, se testado
```

Nesta tabela, as redes são diferentes.

Portanto CPU não basta.

A decisão precisa considerar fortemente:

- qualidade de voz;
- supressão de ruído;
- preservação de fala;
- artefatos;
- comportamento em SNR extremo;
- robustez a tipos diferentes de microfone e ruído.

---

# 11. Tipos de áudio que devem compor o corpus BigLinux

Montar um conjunto reproduzível representando desktop Linux real.

Incluir, pelo menos:

- ventilador de computador;
- cooler de notebook;
- ar-condicionado;
- ruído elétrico/hiss;
- teclado mecânico;
- teclado de notebook;
- cliques do mouse;
- digitação com fala;
- trânsito;
- motocicleta;
- ônibus;
- pessoas ao fundo;
- televisão;
- música ao fundo;
- café/escritório;
- chuva;
- vento;
- reverberação de quarto;
- sala grande;
- microfone de webcam;
- headset barato;
- microfone de notebook;
- microfone USB;
- voz baixa;
- voz alta;
- voz masculina;
- voz feminina;
- diferentes idiomas;
- diferentes distâncias do microfone;
- diferentes SNRs.

Sempre que legalmente possível, manter corpus de regressão fixo no projeto ou instruções reproduzíveis para obtê-lo.

---

# 12. Teste auditivo é obrigatório

As métricas objetivas não bastam.

Realizar teste cego ou, no mínimo, avaliação sem mostrar ao avaliador qual backend/modelo está sendo ouvido.

Avaliar:

- naturalidade;
- inteligibilidade;
- preservação de consoantes;
- preservação de sibilantes;
- respiração;
- voz metálica;
- musical noise;
- pumping;
- buracos na fala;
- início de palavras cortado;
- final de palavras cortado;
- mudança artificial de timbre;
- ruído residual;
- comportamento durante silêncio;
- transição silêncio ↔ fala.

Comparar obrigatoriamente o FastEnhancer com o filtro atual.

---

# 13. Métricas objetivas de qualidade

Quando houver referência limpa, medir, conforme aplicável:

- PESQ;
- STOI;
- ESTOI;
- SI-SDR;
- SNR;
- DNSMOS;
- SCOREQ ou métrica equivalente apropriada;
- WER usando ASR fixo, para verificar preservação de inteligibilidade;
- clipping;
- DC offset;
- NaN/Inf.

Não escolher o modelo apenas por uma métrica.

Registrar resultados por categoria de ruído, e não somente média global.

---

# 14. O faster-enhancer.c é especialmente interessante por ser feito para streaming

Há uma diferença crítica entre:

```text
benchmark que processa arquivo o mais rápido possível
```

e:

```text
callback de áudio que recebe um frame a cada 6,67 ms
```

O próprio trabalho do `faster-enhancer.c` chama atenção para isso.

Portanto executar dois benchmarks:

## Racing

Processar frames continuamente o mais rápido possível.

Serve para:

- throughput;
- comparação microarquitetural;
- perfil de hotspots.

## Paced / real-time

Entregar exatamente um frame no ritmo em que o áudio chega.

Serve para:

- comportamento do governor;
- frequência real da CPU;
- energia;
- wakeups;
- jitter;
- p99;
- deadlines perdidos.

**O modo paced tem maior peso para decidir o padrão do BigLinux.**

---

# 15. Integração real com PipeWire

Depois dos microbenchmarks, integrar cada finalista ao caminho real.

Medir:

```text
PipeWire input
    ↓
conversão/resampling necessário
    ↓
pré-processamento
    ↓
modelo
    ↓
pós-processamento
    ↓
PipeWire output
```

Registrar:

- latência end-to-end;
- XRUNs;
- glitches;
- p50;
- p95;
- p99;
- máximo;
- uso de CPU;
- CPU por thread;
- wakeups;
- RAM/PSS;
- cópias;
- alocações.

Um runtime que vence isoladamente pode perder depois de integração.

---

# 16. Latência algorítmica deve ser comparada separadamente

Não confundir:

```text
tempo de computação
```

com:

```text
latência algorítmica
```

No `faster-enhancer.c`, o processamento ocorre em blocos de 320 samples a 48 kHz, mas existe também o alinhamento do STFT informado pelo projeto.

Para cada solução, calcular:

```text
latência de buffering
+
latência algorítmica do modelo
+
latência de computação
+
latência adicionada pela integração
```

O relatório final deve apresentar todas separadamente e o total.

---

# 17. Regra crítica: p99 vale mais que média

Exemplo:

```text
Runtime A
média = 1,8 ms
p99   = 6,5 ms
```

contra:

```text
Runtime B
média = 2,2 ms
p99   = 2,8 ms
```

Para um frame de 6,67 ms, B pode ser muito mais seguro.

Registrar obrigatoriamente:

- mínimo;
- média;
- p50;
- p90;
- p95;
- p99;
- p99.9 quando houver amostras suficientes;
- máximo;
- deadline misses.

---

# 18. Real-Time Factor

Calcular:

```text
RTF = tempo para processar áudio / duração daquele áudio
```

Para um sistema ao vivo:

```text
RTF < 1
```

é apenas o mínimo matemático.

Para ser padrão, queremos margem grande.

Registrar:

- RTF racing;
- RTF paced;
- RTF médio;
- RTF p99 equivalente.

---

# 19. Alocações — vantagem potencial importante do faster-enhancer.c

O projeto declara:

> nenhuma alocação de heap depois do startup.

Verificar isso no Linux.

Após `fe_init`, perfilar várias horas e confirmar:

- `malloc`;
- `calloc`;
- `realloc`;
- `free`;
- alocadores indiretos.

Comparar com cada runtime ONNX.

Um objetivo desejável para qualquer implementação de áudio ao vivo é:

```text
0 alocações no hot path por frame
```

ou o mais próximo possível disso.

---

# 20. Cópias de memória

Instrumentar o caminho:

```text
buffer PipeWire
→ entrada do filtro
→ tensor/buffer do modelo
→ estados internos
→ saída
→ PipeWire
```

Contar cópias inevitáveis e evitáveis.

Investigar:

- processamento in-place;
- buffers persistentes;
- output prealocado;
- reuse de tensores;
- zero-copy quando a API permitir.

O `faster-enhancer.c` aceita processamento in-place segundo sua API; testar se isso pode ser aproveitado na integração.

---

# 21. Uso de memória

Medir:

- RSS;
- PSS;
- memória privada;
- pico na inicialização;
- steady state;
- heap;
- mappings;
- crescimento em 30 min;
- crescimento em 2 h.

Separar:

```text
pesos
estado do modelo
runtime
workspace/scratch
buffers do áudio
bibliotecas
```

O pequeno blob de pesos do `faster-enhancer.c` não significa automaticamente que o processo inteiro utilizará apenas esse valor.

Medir o processo real.

---

# 22. Threads

Para runtimes genéricos testar:

```text
1
2
4
default/auto
```

Para `faster-enhancer.c`, medir seu comportamento single-thread real.

Um filtro padrão deve evitar consumir vários cores apenas para ganhar um benchmark de throughput.

Avaliar interferência com:

- PipeWire;
- navegador;
- videoconferência;
- desktop;
- outros filtros.

---

# 23. Energia

Especialmente em notebooks, medir quando possível:

- package energy;
- energia por minuto de microfone ativo;
- frequência média;
- tempo em turbo;
- wakeups;
- C-states;
- temperatura.

A redução de ruído pode ficar ativa por horas.

Uma diferença pequena de CPU média pode produzir diferença relevante de bateria.

---

# 24. Teste sob carga

Executar cada finalista em:

## Cenário 1

Filtro sozinho.

## Cenário 2

Filtro + navegador com múltiplas abas.

## Cenário 3

Filtro + videoconferência.

## Cenário 4

Filtro + vídeo 1080p/4K.

## Cenário 5

Filtro + outros processamentos de áudio BigLinux.

## Cenário 6

Carga curta de compilação/descompressão em background.

Medir deadline misses e p99.

---

# 25. Teste de longa duração

Executar:

```text
30 minutos — obrigatório
2 horas — recomendado
```

Verificar:

- leak;
- crescimento de estado;
- XRUNs;
- NaN;
- Inf;
- crash;
- corrupção;
- perda de sincronização;
- spikes tardios;
- mudança de qualidade.

---

# 26. RTen — prioridade alta para os ONNX existentes

Projeto:

```text
https://github.com/robertknight/rten
```

Testar:

- suporte de todos os operadores;
- `.onnx` direto;
- formato `.rten`;
- startup;
- PSS;
- CPU;
- p50/p95/p99;
- states recorrentes;
- shapes;
- INT8 quando aplicável;
- alocações;
- cópias.

RTen continua sendo candidato forte para reduzir o custo dos ONNX existentes.

Mas **não usar uma eventual vitória do RTen para deixar de testar soluções compiladas ou especializadas**.

---

# 27. tract — prioridade alta para os ONNX existentes

Projeto:

```text
https://github.com/sonos/tract
```

Testar:

- compatibilidade;
- CPU;
- PSS;
- startup;
- p99;
- stateful/recurrent;
- número de threads;
- alocações;
- quantização;
- integração real.

Comparar diretamente com RTen, ORT Minimal e OpenVINO.

---

# 28. Lele — PRIORIDADE ALTA — ONNX → Rust especializado

Projeto:

```text
https://github.com/miuda-ai/lele
```

Lele é particularmente importante porque **não segue a abordagem de runtime ONNX genérico**.

Fluxo:

```text
modelo.onnx
    ↓
codegen AOT
    ↓
código Rust especializado
+ pesos
    ↓
rustc/LLVM
    ↓
binário nativo
```

Segundo o projeto:

- cada operador é convertido em chamada Rust direta;
- o grafo não é percorrido a cada inferência;
- há planejamento/reuso de buffers;
- existem fusões de operadores;
- constant folding;
- kernels específicos para áudio, visão e fala;
- SIMD em arquiteturas suportadas;
- não depende de ONNX Runtime no processo final de inferência.

Os benchmarks publicados pelo projeto mostram que o ganho depende muito do modelo, chegando a múltiplos do desempenho do ORT em alguns casos. **Esses números são apenas referência externa e precisam ser reproduzidos no Linux/x86-64 do BigLinux.**

## Teste obrigatório

Tentar converter **cada modelo ONNX de denoise atualmente usado pelo BigLinux**.

Para cada modelo:

1. listar operadores;
2. tentar codegen sem alterar o grafo;
3. documentar operadores ausentes;
4. se faltarem poucos operadores simples, estimar se vale implementá-los no Lele;
5. compilar Release/LTO;
6. medir CPU/PSS/p99;
7. medir número de alocações no hot path;
8. validar equivalência numérica;
9. validar streaming/state;
10. comparar diretamente com ORT, ORT Minimal, OpenVINO, RTen e tract.

### Regra

Se Lele suportar o modelo sem adaptações grandes, ele deve entrar na final.

---

# 29. DeepFilterNet3.c.wasm — implementação especializada C

Projeto:

```text
https://github.com/kdrkdrkdr/DeepFilterNet3.c.wasm
```

Apesar do nome `.wasm`, o núcleo é uma **reimplementação em C do engine de inferência do DeepFilterNet3**, e o projeto também possui caminho/teste nativo.

A motivação é muito próxima de `faster-enhancer.c`:

- eliminar interpretador de grafo ONNX;
- remover overhead de runtime genérico;
- implementar diretamente GRU, Conv, matvec e demais operações necessárias;
- usar buffers de tamanho fixo;
- zero heap durante processamento;
- SIMD;
- engine voltada especificamente ao streaming do modelo.

## Por que testar

Este projeto serve para responder duas perguntas:

1. o **DeepFilterNet3 com engine C especializada** é melhor solução completa que os filtros atuais?
2. quais técnicas dele podem ser reutilizadas numa eventual implementação especializada de **DPDFNet**?

## Teste

Compilar nativamente em Linux x86-64.

Não avaliar somente WASM.

Medir:

- qualidade;
- CPU;
- PSS;
- p99;
- latência;
- alocações;
- cópias;
- estado persistente;
- comportamento sob PipeWire;
- compatibilidade AVX2/AVX-512;
- consumo energético.

Comparar contra FastEnhancer-Medium + faster-enhancer.c e contra o melhor DPDFNet disponível.

---

# 30. DPDFNet — candidato prioritário para implementação própria C/Rust

Projeto oficial:

```text
https://github.com/ceva-ip/DPDFNet
```

DPDFNet é uma família de modelos **causais, mono e adequada a speech enhancement em streaming**, derivada do DeepFilterNet2 e expandida com blocos Dual-Path RNN.

O projeto disponibiliza:

- modelos 8 kHz;
- modelos 16 kHz;
- modelos 48 kHz;
- PyTorch;
- ONNX;
- TFLite;
- inferência stateful;
- demonstração em tempo real.

O pacote oficial de inferência CPU utiliza atualmente ONNX Runtime, portanto existe espaço para verificar se uma implementação especializada reduz substancialmente o custo.

## Modelos 48 kHz particularmente relevantes

No estado pesquisado em 2026-09-09:

```text
dpdfnet2_48khz_hr
≈ 2,58 M parâmetros
≈ 2,42 G MACs
ONNX ≈ 10 MB

dpdfnet8_48khz_hr
≈ 3,63 M parâmetros
≈ 7,17 G MACs
ONNX ≈ 14,2 MB
```

Esses números são referência externa e devem ser confirmados na revisão efetivamente empacotada.

## Decisão de projeto BigLinux

> **Se formos criar uma implementação própria especializada em C ou Rust, o alvo é DPDFNet. Não direcionar esse esforço para GTCRN.**

GTCRN pode continuar aparecendo eventualmente como comparação de modelo se houver motivo técnico, mas **não é o alvo da implementação especializada definida neste trabalho**.

## Antes de implementar

Primeiro determinar qual variante DPDFNet merece o investimento:

- baseline 16 kHz;
- DPDFNet2;
- DPDFNet4;
- DPDFNet8;
- DPDFNet2 48 kHz HR;
- DPDFNet8 48 kHz HR.

Para uso geral de desktop/voz de alta qualidade, dar atenção especial às variantes 48 kHz, mas deixar os testes decidirem.

---

# 31. Estratégia para um DPDFNet especializado em C ou Rust

Não iniciar pelo port manual de todo o modelo.

Executar em fases.

## Fase A — estabelecer baseline

Medir o DPDFNet escolhido usando:

```text
ONNX Runtime
ONNX Runtime Minimal
OpenVINO
RTen
tract
Lele
ONNX-MLIR/IREE, quando compatíveis
```

## Fase B — analisar o grafo

Gerar inventário completo:

```text
operador
shape
dtype
frequência de execução
tempo acumulado
memória temporária
estado recorrente
```

Usar profiling para identificar os hotspots reais.

## Fase C — protótipo automático

Tentar:

```text
DPDFNet ONNX → Lele → Rust
DPDFNet ONNX → emx-onnx-cgen → C
DPDFNet ONNX → onnx2c → C
DPDFNet ONNX → Burn ONNX → Rust
DPDFNet ONNX → ONNX-MLIR → nativo
```

Isso mostrará se é possível obter boa parte da especialização sem portar manualmente a rede.

## Fase D — especialização manual somente dos hotspots

Se necessário, substituir kernels críticos por implementações próprias:

- Dual-Path RNN;
- GRU/RNN;
- GEMM/GEMV;
- convolution;
- normalization;
- complex operations;
- STFT/iSTFT;
- deep filtering;
- ativações;
- quantização/dequantização.

## Fase E — objetivos do runtime especializado

Buscar:

- nenhum parser ONNX no computador do usuário;
- nenhuma travessia de grafo em runtime;
- nenhuma alocação de heap por frame;
- buffers planejados antecipadamente;
- estados persistentes reutilizados;
- pesos prepacked;
- fusão de operadores;
- SIMD;
- dispatch por ISA;
- mínimo de cópias;
- processamento single-thread eficiente quando possível.

---

# 32. emx-onnx-cgen — ONNX → C determinístico

Projeto:

```text
https://github.com/emmtrix/emx-onnx-cgen
```

Este projeto converte ONNX para C explícito e determinístico.

Características relevantes:

- layout de memória conhecido em compilação;
- código C auditável;
- fluxo de controle explícito;
- pode gerar código sem alocação dinâmica;
- não exige runtime ML externo;
- ampla cobertura recente de operadores ONNX;
- saída apropriada para compilação posterior com GCC/Clang;
- pode servir de entrada para otimizações mais especializadas.

## Limitação importante

O projeto declara que **otimização agressiva de desempenho não é o objetivo principal do gerador C**.

Portanto não assumir:

```text
ONNX → C
=
automaticamente mais rápido que ORT
```

O interesse para BigLinux é outro:

1. remover runtime genérico;
2. obter uma implementação C correta;
3. identificar loops/hotspots;
4. permitir que Clang/GCC otimizem;
5. substituir somente kernels críticos por SIMD especializado.

## Teste DPDFNet

Este deve ser um dos primeiros compiladores automáticos testados caso o DPDFNet escolhido tenha shapes suficientemente estáticos.

Comparar:

```text
emx C simples
emx C + -O3/LTO
emx C + kernels especializados
```

---

# 33. onnx2c — ONNX → um arquivo C

Projeto:

```text
https://github.com/kraiskil/onnx2c
```

O onnx2c:

- lê ONNX;
- gera código C;
- aloca buffers em compile time;
- evita memória dinâmica;
- praticamente só depende da biblioteca matemática C;
- gera saída em um único arquivo C.

É conceitualmente muito atraente para áudio em tempo real.

## Limitação

A cobertura de operadores é menor que em runtimes genéricos.

No estado pesquisado, o projeto informa implementação total ou parcial de uma fração do conjunto ONNX, não cobertura universal.

## Regra

Tentar DPDFNet e os ONNX atuais.

Se falharem por apenas poucos operadores simples, registrar quais são e estimar o custo de adicionar suporte.

Não gastar grande esforço adaptando o modelo se a lacuna for extensa.

---

# 34. Burn ONNX — ONNX → Rust modificável

Projeto:

```text
https://github.com/tracel-ai/burn-onnx
```

Burn ONNX converte modelos para **código Rust nativo do ecossistema Burn**.

Vantagens:

- código Rust gerado é legível/modificável;
- pesos em formato próprio;
- simplificações de grafo;
- constant folding;
- common subexpression elimination;
- dead-code elimination;
- possibilidade de substituir operadores por kernels próprios;
- múltiplos backends CPU/GPU/WGPU;
- suporte a `std` e `no_std` em partes relevantes do ecossistema.

## Interesse específico para DPDFNet

Pode ser uma ponte:

```text
DPDFNet ONNX
    ↓
Burn ONNX
    ↓
Rust gerado
    ↓
substituir apenas hotspots
    ↓
runtime Rust especializado
```

Comparar com Lele.

Se Burn adicionar muito overhead de framework, Lele ou um port Rust mais direto podem ser melhores.

---

# 35. Vokra — runtime Rust speech-first experimental

Projeto:

```text
https://github.com/ayutaz/vokra
```

Vokra é um runtime em Rust especializado em workloads de fala.

Características relevantes:

- evita ONNX no runtime final;
- conversão offline;
- operadores de áudio/fala como primeira classe;
- STFT/iSTFT;
- streaming state;
- denoise;
- backends CPU e aceleradores em desenvolvimento;
- suporte/validação de DeepFilterNet3 descrito pelo projeto.

## Estado

O próprio projeto se declara **pré-release e não pronto para produção**.

Portanto:

- usar em pesquisa/benchmark;
- estudar arquitetura;
- não tornar dependência crítica do BigLinux sem avaliação de maturidade;
- observar especialmente kernels e abstrações úteis para eventual DPDFNet Rust.

---

# 36. IREE — opção AOT

Projeto:

```text
https://iree.dev/
```

Fluxo:

```text
ONNX
→ importação
→ MLIR
→ compilação antecipada
→ artefato
→ runtime IREE
```

Pode remover trabalho de parsing/otimização da inicialização do usuário.

Medir:

- runtime final;
- artefato;
- PSS;
- CPU;
- p99;
- startup;
- dependências;
- compatibilidade entre CPUs;
- necessidade de artefatos por ISA.

A compilação deve ocorrer durante build/empacotamento.

---

# 37. ONNX-MLIR — ONNX → LLVM/código nativo

Projeto:

```text
https://github.com/onnx/onnx-mlir
```

É especialmente interessante quando o modelo é fixo e distribuído pelo BigLinux:

```text
modelo.onnx
    ↓
ONNX-MLIR
    ↓
LLVM
    ↓
biblioteca nativa
```

O ambiente pesado de compilação pode ficar somente no servidor/build system do BigLinux.

O computador do usuário recebe apenas o necessário para executar.

## Teste

Para os modelos que compilarem corretamente:

- `-O3`;
- library output;
- CPU específica e CPU genérica;
- PSS;
- p99;
- tamanho da biblioteca;
- compatibilidade;
- qualidade numérica;
- custo de integração.

---

# 38. ncnn e MNN — secundários

Testar apenas quando:

- conversão for confiável;
- operadores forem suportados;
- não houver mudança de qualidade;
- o modelo se beneficiar claramente.

Não gastar tempo excessivo tentando adaptar um modelo incompatível.

---

# 39. Princípios aprendidos com faster-enhancer.c e DeepFilterNet3.c

O maior ganho não vem simplesmente da linguagem C ou Rust.

O objetivo é substituir:

```text
grafo ONNX genérico
+ parser
+ scheduler
+ dispatch
+ tensor allocator
+ kernels genéricos
```

por algo próximo de:

```text
funções conhecidas em compile time
+ ordem de execução fixa
+ memória planejada
+ pesos prepacked
+ kernels específicos
+ fusões específicas do modelo
+ SIMD
+ INT8/FP16 quando seguro
+ zero malloc no hot path
```

Esses princípios devem orientar qualquer port especializado de **DPDFNet**.

---

# 40. Profiling obrigatório

Usar ferramentas Linux conforme apropriado:

```text
perf
hotspot
heaptrack
valgrind/massif
/usr/bin/time -v
smem
pmap
strace
```

Perguntas:

- tempo está nos GEMMs/GEMVs?
- STFT/iSTFT?
- Dual-Path RNN?
- GRU?
- convoluções?
- deep filtering?
- quantização?
- cópias?
- malloc?
- resampling?
- sincronização de threads?
- integração PipeWire?

Não otimizar por suposição.

---

# 41. Compilação

Para todos os candidatos:

- Release;
- `-O3`/equivalente;
- LTO quando seguro;
- sem debug no benchmark;
- símbolos separados quando necessário;
- registrar compilador e versão;
- registrar `CFLAGS`/`CXXFLAGS`/`RUSTFLAGS`;
- não esconder uso de `-march=native`.

Testar separadamente:

```text
build genérica da distribuição
build x86-64-v2, se aplicável
build x86-64-v3 / AVX2
build AVX-VNNI
build AVX-512/VNNI quando houver hardware
```

Avaliar runtime dispatch em vez de distribuir um único binário incompatível com CPUs antigas.

---

# 42. Compatibilidade e fallback

Criar tabela ampliada:

| CPU/ISA | FastEnhancer C | DPDFNet especializado | ORT Min | OpenVINO | RTen | tract | Lele |
|---|---|---|---|---|---|---|---|
| baseline BigLinux | | | | | | | |
| AVX2/FMA | | | | | | | |
| AVX-VNNI | | | | | | | |
| AVX-512 VNNI | | | | | | | |

Se um backend extremamente eficiente exigir CPU recente, isso **não o elimina automaticamente**.

Pode haver:

```text
backend principal para CPU moderna
+
fallback para CPU antiga
```

A seleção deve ser automática.

---

# 43. Licenças

Confirmar a licença da revisão exata de:

- FastEnhancer;
- faster-enhancer.c;
- DeepFilterNet3.c.wasm;
- DPDFNet;
- Lele;
- emx-onnx-cgen;
- onnx2c;
- Burn/Burn ONNX;
- Vokra;
- RTen;
- tract;
- IREE;
- ONNX-MLIR;
- OpenVINO;
- ONNX Runtime.

DPDFNet atualmente publica seu projeto principal sob **Apache-2.0**, mas confirmar também:

- licença dos pesos;
- notices;
- datasets;
- componentes incorporados;
- arquivos de terceiros.

Não confundir licença de código com licença de dados de treinamento.

---

# 44. Benchmark reproduzível

Cada execução deve registrar:

```text
git commit BigLinux
modelo
variante do modelo
SHA-256 dos pesos
backend/compilador
versão/commit do backend
FP32/FP16/BF16/INT8
CPU
microarquitetura
CPUID/features
RAM total
kernel
governor
SMT
threads
sample rate
frame/hop
warm-up
duração
racing/paced
flags de compilação
```

Salvar em JSON/CSV.

---

# 45. Métricas mínimas por solução

| Métrica | Obrigatória |
|---|---|
| Qualidade auditiva | sim |
| PESQ/STOI/SI-SDR/etc. | sim quando aplicável |
| Compatibilidade | sim |
| Latência algorítmica | sim |
| Latência computacional | sim |
| End-to-end | sim |
| p50 | sim |
| p95 | sim |
| p99 | sim |
| máximo | sim |
| RTF racing | sim |
| RTF paced | sim |
| CPU média | sim |
| CPU pico | sim |
| PSS | sim |
| RSS | sim |
| pico RAM | sim |
| alocações no hot path | sim |
| cópias evitáveis | sim |
| threads | sim |
| XRUNs | sim |
| energia | recomendado |
| tamanho instalado | sim |
| dependências | sim |
| licença | sim |

---

# 46. Critérios eliminatórios

Eliminar como padrão se houver:

- qualidade perceptivelmente inferior;
- perda relevante de inteligibilidade;
- artefatos graves;
- crash;
- corrupção do estado de streaming;
- NaN/Inf;
- XRUN recorrente;
- p99 sem margem;
- memory leak;
- licença incompatível;
- incompatibilidade de hardware sem fallback viável;
- integração excessivamente frágil.

---

# 47. Margem de deadline

Não aceitar:

```text
p99 ≈ duração do frame
```

Buscar inicialmente:

```text
p99 do processamento <= 50% do orçamento disponível
```

e preferencialmente margem maior.

O teste decisivo é o pipeline PipeWire real.

---

# 48. Ordem de execução recomendada — revisão atual

## Fase 0 — soluções completas mais promissoras

Testar primeiro:

```text
FastEnhancer-Medium + faster-enhancer.c
DeepFilterNet3 + engine C especializada
DPDFNet + implementação atual/baseline
```

Objetivo: descobrir qual **modelo** entrega a melhor qualidade para o BigLinux.

---

## Fase 1 — baselines corretas dos ONNX

Para os modelos relevantes:

```text
ONNX Runtime
ONNX Runtime Minimal
OpenVINO
RTen
tract
```

---

## Fase 2 — compilação AOT de alta prioridade

Testar:

```text
Lele              → Rust especializado
emx-onnx-cgen     → C
onnx2c            → C, se compatível
Burn ONNX         → Rust/Burn
ONNX-MLIR         → código nativo
IREE              → código/artefato AOT
```

**Lele e emx-onnx-cgen devem ser testados cedo**, pois são diretamente relevantes à hipótese de eliminar o runtime genérico.

---

## Fase 3 — decisão sobre DPDFNet especializado

Se DPDFNet mostrar a melhor qualidade ou uma vantagem relevante, mas estiver limitado por CPU/RAM/latência:

```text
AUTORIZAR PROTÓTIPO DPDFNET C/RUST
```

Começar por codegen automático e profiling.

Port manual somente onde houver ganho provável.

---

## Fase 4 — final

Comparar, no mínimo:

```text
melhor modelo atual + melhor runtime/AOT
FastEnhancer-Medium + faster-enhancer.c
DeepFilterNet3 + C especializado
DPDFNet + melhor runtime/AOT encontrado
DPDFNet C/Rust especializado, se desenvolvido
```

---

# 49. FastEnhancer pode ser o padrão

Se:

```text
FastEnhancer-Medium + faster-enhancer.c
= qualidade melhor ou equivalente
+ CPU menor
+ PSS menor
+ p99 melhor
+ estabilidade adequada
```

recomendar como padrão, com fallback quando necessário.

---

# 50. DPDFNet pode ser o padrão — inclusive com engine própria

Se os testes demonstrarem:

```text
DPDFNet
= melhor qualidade perceptiva
ou melhor robustez em ruído real
```

mas o ONNX Runtime/OpenVINO/etc. consumirem recursos demais, **não descartar DPDFNet por causa do runtime**.

Executar a investigação:

```text
DPDFNet
→ Lele/Rust
→ emx/C
→ ONNX-MLIR
→ kernels especializados
→ eventual engine C/Rust própria
```

A decisão final pode ser:

```text
PADRÃO BIGLINUX:
DPDFNet + engine C/Rust especializada
```

se os dados justificarem.

---

# 51. Possibilidade de múltiplos níveis

Exemplo possível:

```text
Padrão:
DPDFNet especializado ou FastEnhancer C

Fallback para CPU antiga:
modelo leve + ORT Minimal/RTen/tract

Qualidade máxima opcional:
variante DPDFNet mais pesada ou outro modelo vencedor
```

A UI deve continuar simples.

Seleção automática sempre que possível.

---

# 52. O que NÃO fazer

Não:

- escolher linguagem pela reputação;
- assumir que C sempre vence Rust;
- assumir que Rust sempre terá overhead;
- confundir geração de código com otimização automática;
- declarar Lele vencedor sem benchmark Linux;
- declarar emx-onnx-cgen rápido apenas porque remove ORT;
- declarar `faster-enhancer.c` 3,3× mais rápido no BigLinux sem reproduzir;
- ignorar ORT Minimal;
- comparar modelos diferentes e atribuir ganho ao runtime;
- medir somente média;
- ignorar modo paced;
- ignorar PipeWire;
- ignorar CPU antiga;
- otimizar sem `perf`;
- iniciar um port completo de DPDFNet antes de identificar hotspots;
- direcionar a implementação própria para GTCRN: **o alvo definido é DPDFNet**.

---

# 53. Entregáveis obrigatórios

## `denoise-benchmark.md`

Incluir:

- modelos;
- engines;
- geradores de código;
- hardware;
- metodologia;
- qualidade;
- CPU;
- memória;
- latência;
- XRUNs;
- energia;
- compatibilidade;
- licenças;
- problemas encontrados.

## `denoise-benchmark.json`

Dados brutos.

## `DENOISE-AOT-COMPILERS.md`

Registrar para cada modelo:

```text
Lele:
  compilou?
  operadores ausentes?
  resultado numérico?
  CPU/PSS/p99?

emx-onnx-cgen:
  compilou?
  malloc no hot path?
  CPU/PSS/p99?

onnx2c:
  compilou?
  operadores ausentes?
  CPU/PSS/p99?

Burn ONNX:
  compilou?
  overhead do backend?
  CPU/PSS/p99?

ONNX-MLIR/IREE:
  compilou?
  tamanho do artefato?
  CPU/PSS/p99?
```

## `dpdfnet-native-feasibility.md`

Se DPDFNet permanecer finalista, registrar:

```text
variante escolhida:
qualidade:
operadores:
hotspots:
estado recorrente:
memória:
alocações:
cópias:
kernels a especializar:
viabilidade C:
viabilidade Rust:
ganho estimado:
protótipo automático disponível:
necessidade de port manual:
```

## `DENOISE-DECISION.md`

Formato mínimo:

```text
SOLUÇÃO PADRÃO:
modelo:
engine:
linguagem:
formato dos pesos:
sample rate:
CPU mínima:
ISA:
threads:
qualidade:
CPU:
PSS:
p99:
latência algorítmica:
latência end-to-end:
RTF paced:
XRUNs:
tamanho instalado:
licença:

FALLBACK:
...

MODO OPCIONAL:
...
```

---

# 54. Perguntas finais que o agente deve responder

1. **Qual modelo produz a melhor redução de ruído para uso real no BigLinux?**
2. **Qual runtime executa melhor os ONNX existentes?**
3. **Compilar o modelo antecipadamente para C/Rust/nativo supera ORT/OpenVINO?**
4. **FastEnhancer-Medium + faster-enhancer.c deve ser o padrão?**
5. **DeepFilterNet3.c deve ser o padrão?**
6. **DPDFNet deve ser o padrão?**
7. **Se DPDFNet for o melhor modelo, vale criar uma engine C/Rust especializada para ele?**
8. **Qual é o fallback correto para CPUs antigas?**

A decisão deve ser baseada em medições do BigLinux.

---

# 55. Fontes técnicas principais

Consultadas/revisadas em 2026-09-09:

```text
FastEnhancer
https://github.com/aask1357/fastenhancer

FastEnhancer paper
https://arxiv.org/abs/2509.21867

faster-enhancer.c
https://github.com/kdrkdrkdr/faster-enhancer.c

faster-enhancer.c paper
https://arxiv.org/abs/2607.25350

DeepFilterNet3.c.wasm
https://github.com/kdrkdrkdr/DeepFilterNet3.c.wasm

DPDFNet
https://github.com/ceva-ip/DPDFNet

Lele
https://github.com/miuda-ai/lele

emx-onnx-cgen
https://github.com/emmtrix/emx-onnx-cgen

onnx2c
https://github.com/kraiskil/onnx2c

Burn ONNX
https://github.com/tracel-ai/burn-onnx

Vokra
https://github.com/ayutaz/vokra

RTen
https://github.com/robertknight/rten

tract
https://github.com/sonos/tract

IREE
https://iree.dev/

ONNX-MLIR
https://github.com/onnx/onnx-mlir

ONNX Runtime
https://onnxruntime.ai/

OpenVINO
https://github.com/openvinotoolkit/openvino
```
