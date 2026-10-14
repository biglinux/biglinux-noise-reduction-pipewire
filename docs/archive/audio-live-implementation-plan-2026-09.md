# BigLinux — plano técnico de implementação de áudio ao vivo

**Versão:** 1.0 · **Data:** 9 de setembro de 2026  
**Destinatário:** agente responsável pela implementação e pelos pacotes do BigLinux.  
**Projeto principal:** `biglinux/biglinux-noise-reduction-pipewire`.  
**Estado deste documento:** especificação de trabalho; não é declaração de implementação, de aprovação acústica nem de execução de testes.

## 1. Missão e decisões obrigatórias

Melhorar a voz **durante a captura ao vivo** em computadores comuns, preservando timbre, palavras, consoantes e dinâmica. O trabalho inclui corrigir o processamento existente, preparar a substituição do AEC3 e acrescentar recursos opcionais de locução e correção de afinação para canto.

**Ordem de preferência:** preservação da fala e continuidade → latência adequada → baixo consumo → redução adicional de ruído/eco → efeitos opcionais. Um algoritmo que elimina ruído silenciando partes da voz não é uma melhoria.

Implementar incrementalmente, com testes de regressão e pacotes instaláveis. Não entregar apenas outra pesquisa. Não reescrever o aplicativo, o reconciliador ou toda a infraestrutura de testes. Não transformar treinamento de uma rede nova em pré-requisito para melhorar o produto.

### 1.1. Regras de escopo

- O produto deve funcionar sem GPU, internet, Python/PyTorch e serviços externos de inferência no caminho normal de uso. Ferramentas de desenvolvimento podem usar Python; não executar interpretador no callback de áudio.
- Recursos pesados devem ser pacotes opcionais. Só recebem recomendação quando houver vantagem perceptual demonstrada ao vivo, dentro de um orçamento explícito de atraso e recursos.
- Não adicionar recursos de processamento de arquivos. Gravações curtas, sinais sintéticos e reprodução controlada são permitidos **somente como instrumentos de teste**, com licença e consentimento adequados.
- Manter 48 kHz no caminho principal quando a captação e o backend permitirem. Uma rede de 16 kHz não vira full-band ao reamostrar sua saída para 48 kHz. Expor essa limitação antes da ativação.
- Não empilhar dois AECs nem vários denoisers por padrão. Um modelo conjunto substitui os estágios que já executa.
- Correção de afinação e alteração de formantes pertencem ao modo **Canto/efeitos**, nunca ao perfil natural de reuniões.
- Nenhum novo modelo com licença de pesos indefinida, restrição não comercial, licença somente para pesquisa ou exigência de autorização especial entra no plano de distribuição.
- Não substituir a escolha explícita do usuário, reativar AEC3 silenciosamente, perder configurações ou ligar o microfone apenas porque há reprodução de áudio.

### 1.2. O que este plano não promete

Ainda não existe, nesta entrega, um cancelador alternativo homologado como superior no BigLinux. A substituição de AEC3 é um objetivo com critérios de promoção, não uma conclusão antecipada. Os números publicados pelos projetos são referências upstream, não medições nossas.

Não há justificativa verificada para incluir um restaurador generativo muito pesado como “qualidade excepcional” nesta rodada. O opcional mais custoso selecionado é LocalVQE conjunto, com código e pesos sob Apache-2.0; sua promoção também depende dos testes. A aceleração opcional terá inicialmente CPU/Vulkan, que o projeto oferece; CUDA não é um requisito nem motivo para introduzir artefatos de licença incerta. [S06]

## 2. Base existente e primeiro passo do agente

### 2.1. Revisões usadas como referência

| Componente | Referência conhecida | Papel |
|---|---|---|
| Aplicativo | `7906ae269dd5ca03e6c4aee3e43bfe1ed6e5c1d0` | `main` conferida durante esta preparação; inclui o PR #30 |
| GTCRN LADSPA | `6ee156e8bcbb7fb8ec454cfc2ee76e305bda2593` | Fontes consultados na auditoria anterior |
| DPDFNet LADSPA | Conferir checkout e pacote efetivos | Wrapper externo, incluindo fila/worker e latência |
| DeepFilterNet LADSPA | Conferir checkout e pacote efetivos | Backend já integrado; não é novidade a adicionar |

O aplicativo já possui GTCRN DNS3/VCTK, DeepFilterNet3 e DPDFNet, gate, HPF, equalizador, compressor, modificador de voz, monitoramento, CLI e integração GTK/Plasma. As configurações de AEC consultadas já desativam AGC e supressão de ruído internos do WebRTC. Preservar as correções recentes de persistência, roteamento e latência. [S01–S03]

**Antes de editar:** registrar SHA de cada checkout, modificações locais, versão e SHA-256 dos plugins instalados, modelo efetivo, taxa, quantum e topologia. Comparar o código atual com os locais descritos abaixo. Se uma correção já existir, confirmar seu teste e seguir adiante; não reintroduzir uma versão antiga para reproduzir o relatório.

O código de um repositório e o `.so` distribuído podem divergir. Não afirmar que um defeito está na máquina do usuário sem reproduzi-lo no binário correspondente.

### 2.2. Áreas a reutilizar

| Área | Ação |
|---|---|
| `src/config/` | Evoluir contratos de backends, parâmetros, capacidades e migrações |
| `src/pipeline/` | Integrar novos nós e corrigir a ordem/identidade dos estágios |
| `src/services/reconcile.rs` | Manter aplicação transacional, verificação do grafo e atualização incremental |
| `src/services/echo.rs` e política WirePlumber | Corrigir/estender seleção e observação da referência acústica |
| `src/config/plugin_cost.rs` e `quality.rs` | Medir capacidade real; separar custo de preservação da voz |
| Loaders, CLI, `doctor` e probe existentes | Acrescentar capacidades, saúde e medições; evitar um segundo sistema paralelo |
| `scripts/calibrate/`, testes nativos e CI | Ampliar os testes existentes; conferir disponibilidade na revisão atual |
| Pacotes dos plugins | Corrigir o DSP no componente que o implementa; não esconder defeito do plugin no frontend |

## 3. Seleção de componentes e política de licenças

### 3.1. Componentes novos admitidos nesta rodada

“Admitido” significa que foi encontrada uma licença livre declarada adequada para avançar na integração. Não dispensa fixar versões, preservar avisos e conferir cada artefato exato no pacote.

| Componente | Licença identificada | Uso | Prioridade |
|---|---|---|---|
| `faster-enhancer.c` + `weights/fe.q8` | MIT; `NOTICE` identifica os pesos derivados e seus avisos | Denoiser FastEnhancer-Medium 48 kHz em C/int8 | Alta, após correções |
| LocalVQE v1.4-AEC 2.7K | Apache-2.0 no código e na publicação de pesos | Cancelamento linear de eco, 16 kHz | Alta, experimental |
| LocalVQE v1.4-AEC 200K e variante compacta AEC-only | Apache-2.0 no código e pesos oficiais | Alternativas de eco com máscara neural | Alta, após comparação com a variante linear |
| SpeexDSP | BSD de três cláusulas, com avisos por arquivo | AEC DSP de comparação, incluindo ensaio a 48 kHz | Baseline leve, sem pressupor superioridade |
| x42 Auto Tune / `fat1.lv2` | GPL-2.0-or-later nos fontes consultados | Afinação para canto, sem modelo neural | Opcional leve |
| LSP Plugins, somente módulos necessários | LGPL-3.0; reter licenças dos componentes | De-esser, compressor/expansor e limitador | Opcional leve, medir custo por modo |
| Rubber Band, edição livre | GPL-2.0-or-later | Alteração de altura com controle de formantes | Opcional; atraso próprio deve ser declarado |
| LocalVQE v1.2/v1.3 conjuntos | Apache-2.0 no código e pesos | AEC + redução de ruído + dereverberação | Opcional de maior custo; não padrão |

Fontes e limitações: [S04–S11]. O catálogo deve conter **somente os artefatos fixados e verificados**, não qualquer arquivo de mesmo nome obtido de terceiros.

FastEnhancer original serve como referência técnica de inferência incremental. A primeira integração distribuível deste plano usa o blob `fe.q8`, cuja atribuição está explícita no `NOTICE`. Não acrescentar automaticamente checkpoints Tiny/Base/Large ou outros exports sem um manifesto de redistribuição do artefato selecionado. [S04–S05]

### 3.2. Fora da implementação e dos pacotes desta rodada

Não implementar instaladores ou integração para Stream.FM, GRE-Net/RENet, RE-USE, UniPASE, RVC, LLVC, Seed-VC ou checkpoints comunitários de conversão de voz nesta rodada. Eles não pertencem à seleção fechada acima: podem ter pendências sobre o conjunto de artefatos, não atender ao escopo ou não ter vantagem ao vivo demonstrada para este produto. **Isto não afirma que todos esses projetos tenham licença restritiva.**

Não implementar treinamento de NKF 48 kHz, destilação de UniPASE, reconstrução generativa ou um pipeline experimental SDAEC como requisito da entrega. Não incluir serviços comerciais ou SDKs proprietários como motores do pacote livre.

Não “resolver” licença ausente fazendo o usuário baixar o modelo na primeira execução. Um recurso opcional não elimina restrições de redistribuição ou uso.

### 3.3. Manifesto obrigatório de distribuição

Criar `third_party/audio-components.lock.json`, ou equivalente já adotado no projeto, com um registro por biblioteca, modelo e frontend auxiliar:

```json
{
  "component_id": "identificador-estavel",
  "upstream_url": "origem-canonica",
  "source_revision": "commit-ou-tag-fixado",
  "artifact_path": "caminho-no-pacote",
  "sha256": "hash-calculado-do-artefato-real",
  "license_expression": "expressao-SPDX-conferida",
  "license_evidence": ["LICENSE", "NOTICE-ou-model-card-fixado"],
  "modified": false,
  "source_package": "pacote-de-fontes-correspondente",
  "redistribution_review": "approved"
}
```

O exemplo é um esquema proposto, não um manifesto preenchido. Bloquear o empacotamento com campos fictícios ou evidência ausente. Instalar `LICENSE`/`NOTICE`, preservar atribuições e disponibilizar fontes e patches correspondentes, cumprindo as obrigações da licença selecionada. Não copiar apenas o badge do GitHub como prova da licença de pesos.

A mesma regra vale para dependências de testes, modelos de métricas, dados, exemplos e plugins. Não redistribuir áudio de demonstração ou vozes de terceiros só porque existe um script de download. Usar fixtures próprias/sintéticas ou material expressamente permitido. Não adicionar dependência obrigatória de PESQ/POLQA ou checkpoints de MOS de licença não verificada.

## 4. Arquitetura de destino

### 4.1. Não alterar identidade vocal no modo natural

```text
Microfone físico ──► captura sincronizada ──► AEC selecionado, se necessário
                                                ▲
Saída(s) acústica(s) real(is) ──► referência ──────┘
              │
              ▼
Condicionamento mínimo declarado ──► um denoiser, se necessário
              │
              ▼
Locução opcional: EQ discreto / de-esser / nivelamento / compressão
              │
              ▼
Limitador de segurança ──► microfone virtual estável ──► aplicativos
```

No modo Canto/efeitos, inserir afinação ou pitch/formantes como recurso deliberado. No backend conjunto, remover os nós de AEC/NS/dereverb que duplicariam a mesma função. A figura descreve responsabilidades; aproveitar os processos e nós existentes em vez de criar obrigatoriamente um processo por caixa.

O condicionamento de análise pode diferir da saída audível. Um HPF necessário a um detector não autoriza retirar graves da voz silenciosamente. Não usar gate, compressor, limitador ou denoiser variável antes do AEC sem comprovar o efeito sobre a adaptação. A referência deve corresponder ao áudio efetivamente destinado aos alto-falantes, não ao áudio anterior a transformações arbitrárias.

### 4.2. Contrato de backend

Introduzir dois conceitos independentes: `AecBackend` e `EnhancementBackend`. Backends conjuntos declaram as funções que já executam. O nome da biblioteca/URI do plugin pertence a um catálogo permitido, não a texto arbitrário vindo de `settings.json`.

Cada backend deve declarar: identificador estável, versão/ABI, funções realizadas, taxas e canais suportados, requisitos de CPU/GPU, quantum interno, atraso algorítmico, disponibilidade de bypass alinhado, necessidade de referência, controles realmente suportados e procedência do modelo. Não usar o mesmo booleano `is_attenuation_only()` para todas as decisões futuras.

Contrato conceitual a adaptar ao código existente:

```text
prepare(spec, model)          # fora do callback: valida, aloca, carrega e aquece
process(input, reference?)   # blocos com estado persistente; custo limitado
update_controls(snapshot)   # publicação atômica / por fronteira de bloco
latency()                   # amostras e taxa; distinguir declarada de medida
health()                    # contadores por instância, sem áudio pessoal
reset(reason)               # política explícita para descontinuidade
release()                   # fora do callback
```

Não inventar APIs upstream com esses nomes; são responsabilidades do adaptador BigLinux.

### 4.3. Regras de tempo real

No callback: nenhuma criação/destruição de sessão, carga de modelo, `dlopen`, acesso a arquivos, JSON, log formatado, subprocesso, download, espera de GPU ou bloqueio em mutex/condvar. Pré-alocar filas, buffers, estados e scratch de FFT. Verificar também alocações feitas por bibliotecas C/C++, não somente pelo alocador Rust.

Inferência síncrona pequena é aceitável **se o pior caso medido couber no prazo com margem**. Usar worker quando necessário; não tratar “assíncrono” como sinônimo de barato ou sem atraso. A thread de áudio nunca deve esperar indefinidamente pelo worker.

Filas devem ter capacidade fixa, sequência e timestamp. Definir prazo para cada bloco; uma resposta atrasada é descartada, nunca inserida como se fosse áudio atual. Não aumentar filas indefinidamente. Executar recuperação e reconfiguração fora do callback. Sem prioridade FIFO arbitrariamente alta nem `mlockall` obrigatório para o sistema funcionar.

Guardar estados por fluxo. Nunca compartilhar estados recorrentes entre microfone e reprodução, entre canais independentes ou entre medição e uso real. Coeficientes/pesos imutáveis podem ser compartilhados com ownership correto.

### 4.4. Bypass e fallback têm semânticas diferentes

**Bypass solicitado:** preservar a escolha e a linha temporal. **Fallback por falha:** manter áudio utilizável, informar perda de processamento e respeitar a política do usuário.

Enquanto o fluxo estiver ativo, manter um atraso de sessão coerente entre caminho seco e processado. Usar crossfade alinhado e limitado; não saltar do áudio atrasado para a amostra atual. Renegociar reduções de latência em fronteira controlada, preferencialmente sem captação ativa. Não manter o maior atraso de todos os modelos instalados, apenas o orçamento acordado para a sessão/transição.

O fallback seco deve ser o sinal correto da mesma etapa: após AEC quando falha o denoiser, e não o microfone bruto anterior ao cancelamento. Um fallback de AEC pode reintroduzir eco; torná-lo visível. Não ativar AEC3 se a escolha do usuário exclui esse backend. Nunca executar dois AECs em série como recuperação.

## 5. P0 — corrigir o que já existe antes de comparar novos modelos

Os itens abaixo derivam de leitura de fontes, não de testes acústicos desta entrega. Confirmar no código atual e no pacote efetivo, criar um teste que falha antes e passa depois e limitar cada patch ao problema. [S01–S03]

### FIX-01 — normalização e fallback do GTCRN

**Locais:** `gtcrn-ladspa/ladspa/src/plugin.rs`: `run_primary`, `run_secondary`, `analyse_one_frame`.

Na versão consultada, um fallback pode devolver o espectro não normalizado e depois receber a divisão pelo ganho de normalização. Corrigir a simetria de escalas: devolver a entrada normalizada no caminho normalizado, ou carregar explicitamente o estado de sucesso antes da compensação. A falha do segundo modelo não pode introduzir zeros na mistura nem modificar o ganho do primeiro.

**Teste T01:** injetar falha de criação e inferência com ganhos 1, 2 e 3, em ambos os modelos. Comparar o sinal da etapa com sua referência temporal correta; tolerância de ganho ±0,1 dB. O valor `20·log10(1/3) ≈ −9,54 dB` é apenas a perda matemática que este caminho incorreto pode introduzir na banda afetada, não uma medição da voz inteira.

### FIX-02 — gate independente da redução de ruído

**Locais:** `src/pipeline/mic.rs`, `ai_node_in_mic_chain`, seleção do gate e `GtcrnPlugin::run`.

O aplicativo mantém GTCRN para usar o gate, mas o plugin consultado retorna cedo com `Enable=0` ou intensidade zero, antes do gate. Preferir um gate/expansor independente, reutilizando o já disponível, em vez de executar a rede apenas para gate. Quando o gate não existir em um backend, isso deve ser uma capacidade explícita.

**Teste T02:** matriz redução ligada/desligada × gate ligado/desligado × intensidade zero/não zero, executando o LADSPA empacotado. Desligar NR não pode desligar o gate pedido. Gate sozinho não deve carregar ONNX ou produzir atraso neural. Com tudo desligado, confirmar bypass real e custo correspondente.

### FIX-03 — carga de modelo e recursos fora do callback

**Locais:** `plugin.rs`, `model.rs`, inicialização do loader e adaptadores de plugins.

Mover criação/troca/aquecimento de sessões, liberação de modelos e preparação de FFT para o caminho de controle. A versão consultada chama criação de sessão e aquecimento a partir de `run()`. Preparar recursos antes de publicar a instância. Após aquecimento, zerar ou inicializar o estado conforme a referência correta do modelo; não considerar ruído artificial de warmup uma garantia de preservação da primeira sílaba.

**Teste T03:** primeira ativação, troca, runtime ausente, OOM simulado e repetidas falhas. Sem repetição ilimitada de criação por callback; usar backoff e estado de erro observável. Instrumentar alocações e chamadas bloqueantes durante regime estável e durante alterações ao vivo.

### FIX-04 — eliminar arquivo global de controles do caminho RT

**Local:** `gtcrn-ladspa/ladspa/src/external_controls.rs` e ligação com `resolve_controls`.

A implementação consultada lê um arquivo em `XDG_RUNTIME_DIR` pelo caminho de `run()` e seus valores podem sobrescrever controles por instância. Usar as portas LADSPA/controle do host como fonte normal. Se compatibilidade com o mecanismo antigo ainda for necessária, lê-lo fora do callback, publicar snapshots por instância e retirar valores obsoletos quando o arquivo desaparecer ou se tornar inválido.

**Teste T04:** duas instâncias com intensidades distintas; uma atualização não pode alterar a outra. Remoção/truncamento do arquivo legado, NaN e valores fora de faixa não podem perpetuar controles antigos ou produzir áudio inválido. Não substituir o problema por mutex no callback.

### FIX-05 — bypass, lookahead e retorno de estados antigos

**Local:** `GtcrnPlugin::run`, acumuladores de entrada/saída e lookahead.

Retorno seco imediato tem latência diferente do caminho STFT. Definir estado e transição para bypass, intensidade zero, ativação de gate e mudança de lookahead. Não tocar áudio antigo ao reativar. Mudanças que alteram latência não são apenas atualização de um número: atualizar buffer, declaração e transição em conjunto.

**Teste T05:** alternar controles a cada 100–500 ms durante sinal marcado e fala. Detectar amostras repetidas, perdidas, pulos de sequência, cliques e atraso variável. Não drenar backlog acelerando a fala. O gate deve receber detector e áudio com alinhamento equivalente.

### FIX-06 — HPF e banda alta explícitos; perfil natural sem síntese

**Locais:** `src/config/audio.rs`, `src/pipeline/mic.rs`, `gtcrn-ladspa/ladspa/src/{plugin,biquad,stft}.rs`.

O GTCRN consultado aplica HPF interno de 80 Hz mesmo com o HPF do aplicativo desligado; a recuperação da banda alta pode usar `synthesize_air()`. Separar: condicionamento exigido pelo modelo, filtro audível opcional, preservação da banda alta e síntese de efeito. Não prometer que `VoiceRecovery=1` devolve o sinal original.

No perfil natural, remover a síntese automática de agudos e implementar preservação ou atenuação conservadora da banda capturada, com envelope suave. Avaliar HPF interno por ablação antes de removê-lo da entrada da rede. Um filtro fixo necessário deve aparecer nas capacidades/diagnóstico, e o controle da interface deve refletir o que efetivamente faz.

Revisar o crossover: pesos complementares devem combinar estimativas **da mesma frequência**, não misturar bins vizinhos como se fossem iguais. STFT/iSTFT corretamente construídas podem ter reconstrução de identidade; não aceitar deformação como inevitável por haver FFT.

**Teste T06:** identidade sem modificação de espectro; varreduras e tons na região de 8 kHz; tons graves, fala grave e fricativas. Sem harmônicos sintetizados no modo natural. Mostrar resposta somada dos HPFs realmente ativos. Falha de FFT não deve emitir dados antigos ou NaN.

### FIX-07 — intensidade por backend e preservação na migração

**Locais:** `NoiseReductionConfig`, `deepfilter_attenuation_db`, controle ao vivo e presets.

A intensidade padrão consultada é 1; o mapeamento quadrático do DeepFilterNet permite limite de 100 dB, enquanto GTCRN utiliza outra escala durante fala. Não copiar cegamente um valor normalizado de um backend para outro. Preservar configurações antigas mediante perfil `legacy`/migração versionada; defaults mais naturais só para novos perfis ou restauração explícita.

Criar parâmetros específicos e limites de redução adaptativos com suavização. Faixas de ensaio para atenuação: 6, 9, 12, 15 e 20 dB. São pontos de calibração, não defaults comprovados. Não reaplicar uma nova “correção” adaptativa se o backend já a implementa sem medir a interação.

**Teste T07:** UI, JSON, CLI, grafo renderizado e atualizações ao vivo devem resolver o mesmo valor. Modelo trocado e depois restaurado recupera suas preferências. Intensidade zero deve corresponder ao contrato de bypass, não a “continua caro, mas não informamos”.

### FIX-08 — proteção de ganho e limiter

**Local:** `src/pipeline/gain_safety.rs`.

O `clamp(-1,1)` é uma barreira final contra amostras excessivas, não um limitador transparente nem true-peak. Manter proteção final, mas evitar clipping frequente com ganho correto e um limitador controlado. Separar redução de ganho audível da proteção extrema contra falhas.

A sequência pad 0,5 → rede → ganho 2 não prova identidade: `2·F(x/2)` pode diferir de `F(x)`. Validar a proteção com a rede real e documentar o efeito. Pico da resposta em frequência de um EQ não é limite absoluto de pico instantâneo de todo transiente; manter margem e ensaios de sinais compostos, sem voltar à soma exagerada de todos os ganhos de bandas.

**Teste T08:** saturação de entrada, picos compostos, EQ elevado, pitch e makeup. Medir clipping, overshoot e distorção. Limiter true-peak somente pode ser anunciado após medição independente de intersample peaks; não usar o próprio medidor como única prova.

### FIX-09 — saúde, capacidade e política de qualidade não fictícias

**Locais:** `src/config/quality.rs`, `plugin_cost.rs`, probe, loader e `doctor`.

Distinguir dados medidos de indisponíveis. Na leitura consultada, carga, xruns e estado de chamada não são obtidos; zero preenchido como padrão não comprova ausência de falhas. Reportar `unknown` onde necessário e alimentar decisões com observação real, de custo limitado.

Medição de callback quase vazio não prova que um worker processou áudio. Acrescentar por instância: blocos recebidos, processados, emitidos secos, atrasados, descartados, fila máxima, tempo de inferência e motivo do fallback. Registrar CPU total dos workers e runtimes. Cachear benchmarks por hash do plugin/modelo, hardware e política de execução; não congelar a medição indefinidamente após atualizações.

**Teste T09:** worker travado, resultado atrasado, silêncio barato, modelo ausente e CPU limitada por cgroup. Nenhum desses casos pode ser promovido como “modelo muito leve”. Não trocar backend a cada pequena variação; usar histerese, janela mínima e decisão fora da fala/transição controlada.

### FIX-10 — coerência do grafo, clocks e contrato do DPDFNet

Conferir o quantum solicitado, o efetivo e o atraso medido; comentários sobre 1024 amostras não prevalecem sobre configurações que solicitam 960. Compartilhar o daemon não faz os osciladores físicos de uma placa USB e de outra saída se tornarem idênticos. Reutilizar sincronização/resampling do PipeWire e medir drift residual antes de adicionar outro compensador. [S01, S12]

No DPDFNet, preservar correções de atraso do modelo e da linha seca. Auditar a espera limitada pelo worker e a heurística de host offline: rajadas de callbacks de um grafo ao vivo não podem autorizar espera de centenas de milissegundos. Para esta integração, estabelecer modo real-time explicitamente. Não remover a espera e simplesmente devolver áudio seco na maioria dos hops: redesenhar o handoff com prazo/atraso explícitos, medir a proporção efetivamente processada e revalidar a linha seca. Se não houver solução dentro do orçamento, manter o backend fora do perfil de baixa latência. Não desfazer patches existentes com base em comentários antigos de custo.

**Teste T10:** quantum variável, captação/reprodução em dispositivos diferentes, rajadas, suspensão/retomada, plug/unplug e worker lento. Falta de resposta não pode ampliar o deadline nem bloquear o resto do grafo.

## 6. NS-01 — novo denoiser leve: FastEnhancer-Medium em C

### 6.1. Implementação escolhida e limites conhecidos

Integrar `kdrkdrkdr/faster-enhancer.c` como backend adicional, não em série com GTCRN/DFN3/DPDFNet. A versão publicada trabalha a 48 kHz, recebe/emite 320 amostras e declara 704 amostras de atraso de alinhamento. O blob `fe.q8` é pequeno e sua atribuição MIT está explícita. Em x86, exige AVX2/FMA3/F16C; não oferece fallback escalar. Desempenho publicado em ARM não comprova desempenho nos Intel/AMD do BigLinux. [S04]

**Atenção de integração:** a API consultada utiliza `g_w` e `g_s` globais. Não criar duas instâncias LADSPA chamando a mesma API global. [S04b]

### 6.2. Tarefas

1. Fixar commit e hash do blob; compilar uma referência upstream e armazenar vetores de saída para regressão. Não instalar clipes de demonstração sem licença.
2. Preferir uma adaptação pequena com contexto opaco por instância: pesos imutáveis e estado/scratch por fluxo. Auditar também tabelas de dispatch, normalização e caches. Alternativa aceitável inicial: um processo isolado por fluxo; documentar seu custo. **Não serializar fluxos com um mutex no callback.**
3. Encapsular em plugin/adaptador nativo que reutilize o host atual. Suportar blocos de host diferentes de 320 com ring buffers pré-alocados. Preservar sobras entre chamadas; nunca descartar amostras para alinhar um hop.
4. Declarar taxa, atraso do algoritmo e atraso do adaptador separadamente. Medir a latência total do nó; não gravar “14,67 ms ponta a ponta” na interface.
5. Validar CPU/estado SIMD do SO antes de carregar o caminho otimizado. O launcher e o código de detecção precisam executar no baseline da distribuição. Compilar kernels por alvo, sem `-march=native` global. Falta de ISA resulta em indisponibilidade explicada e fallback conhecido, não `SIGILL`.
6. Conferir ownership do blob durante toda a vida do contexto. Inicialização, aquecimento e liberação fora de RT. Reset após warmup conforme referência; FTZ/DAZ no thread efetivo deve ter política explícita e não alterar indevidamente outros processadores no host.
7. Começar com um controle de **mistura/intensidade** usando caminho seco corretamente alinhado e rampa suave. Não chamar esse controle de limite exato em dB: mistura de waveforms não garante um piso espectral quando há diferença de fase. Só expor `attenuation_limit_db` se for realmente implementado e validado.
8. Manter o motor escolhido durante a sessão. Seleção automática deve usar qualidade e custo medidos; “mais novo” não define o default.

### 6.3. Testes específicos

**T11:** execução isolada upstream × adaptador BigLinux com framing causal equivalente, mesma janela e compensação de atraso; quantização separada de erro de alinhamento. Testar in-place, out-of-place, reset, uma amostra por chamada e blocos 128/256/320/480/512/768/960/1024/2048/8192, além de sequências variáveis.

**T12:** duas instâncias processando sinais diferentes, execução concorrente, destruição de uma enquanto a outra continua e isolamento do probe. Cada saída deve corresponder à execução isolada dentro da tolerância numérica acordada. Verificar sanitizers e ausência de dados de um fluxo no outro.

**T13:** CPU sem a ISA requerida, peso corrompido/truncado/incompatível, falha de init, NaN/Inf de entrada e blocos fora do limite. Zero leitura fora de buffer e nenhum retorno com saída não inicializada. Medir custo com entrega cadenciada, não apenas processando o mais rápido possível.

## 7. AEC-01 — substituição do AEC3 com promoção baseada em evidência

### 7.1. Ordem dos candidatos

Implementar um adaptador comum de AEC e testar, nesta ordem:

| Candidato | Objetivo do teste | Limitação a tornar visível |
|---|---|---|
| SpeexDSP sem preprocessor residual inicialmente | Baseline DSP leve e caminho de ensaio a 48 kHz | Não presumir melhor supressão que AEC3; validar conversão de formato e desempenho |
| LocalVQE v1.4-AEC 2.7K | Preservação com cancelamento linear | 16 kHz; eco não linear/reverberante pode persistir |
| LocalVQE compact AEC-only 49K | Equilíbrio custo/preservação | 16 kHz; verificar frontend requerido no artefato |
| LocalVQE v1.4-AEC 200K | Avaliar benefício da máscara residual | 16 kHz; verificar dano à voz no double-talk |

Usar AEC3 apenas como comparação e opção legada explícita durante a migração, nunca como requisito oculto do substituto. Se nenhum candidato cumprir os critérios, entregar adaptadores e relatório negativo; não remover uma opção funcional e anunciar vitória. O usuário deve poder escolher não usar AEC3 sem o sistema reativá-lo por conta própria.

O SpeexDSP possui API explícita de microfone/referência e separa AEC do pós-processamento. A integração deve conferir a API e formato da versão atual; não confundir a taxa do codec Speex com a taxa aceita pelo DSP. Ajustar tamanho do filtro e blocos por ensaio, sem fixar um tail enorme “por segurança”. [S07]

No LocalVQE, usar a chamada **por quadro**, com estado persistente, não a função de arquivo/clip a cada callback. A API C consultada tem opções de backend, dispositivo, threads e frontend. O catálogo deve declarar ambos os GGUFs quando forem necessários; não procurar um arquivo arbitrário vizinho como política de produção. [S06b]

### 7.2. Referência, atraso e drift

**Escolha da referência:** seguir as ligações efetivas até a saída acústica, inclusive saídas virtuais e smart filters. Não assumir que todo aplicativo reproduz no default. Registrar identificadores resolvidos e sinal de referência válido. Considerar mudança de default, aplicativo preso a outra saída, HDMI, USB e Bluetooth.

**Estéreo:** manter referências independentes para um cancelador multicanal quando suportado. Uma média `L+R` pode anular informação; a existência de dois canais idênticos não constitui estéreo acústico. Para backend mono, delimitar os cenários homologados; não prometer cancelamento universal com referências estéreo descorrelacionadas. Não aplicar dois AECs mono sequenciais como solução automática.

**Temporização:** associar cada bloco a sequência e tempo. Obter a referência antes de seu eco precisar ser cancelado e dentro da janela de adaptação. Reutilizar timestamps e compensação de clocks do grafo; acrescentar estimador de atraso grosseiro/fino apenas quando o backend/host não os fornecer adequadamente. Evitar dois controladores de drift corrigindo o mesmo erro.

**Mudanças:** reaquisição de atraso e readaptação têm política explícita. Testar deslocamentos bruscos de atraso, ganho de saída e caminho acústico. A estimativa não pode ficar congelada para sempre após a primeira aquisição. Não normalizar referência com mudanças bruscas de ganho; documentar o ponto do volume de hardware, que pode estar depois do monitor digital.

### 7.3. Ativação sem cortar voz nem perder a cauda do eco

Distinguir ausência de consumidor, referência silenciosa, ausência de acoplamento acústico e falha de referência. São estados diferentes.

Sem aplicativo consumidor, suspender captura/inferência sem acordar microfone por playback. Com consumidor e alto-falantes, manter histórico e cauda mesmo após uma pausa da referência. Para reduzir atuação, usar estado/histerese e transição suave; não ligar/desligar serviço a cada pacote ou palavra. A detecção de double-talk deve ser do canceller ou uma política validada, não apenas VAD do microfone, que também pode detectar a voz dos alto-falantes.

Para uma sessão com AEC de 16 kHz, declarar o caminho limitado em banda e seu atraso. Não reconstruir agudos com IA ou recolocar banda alta bruta para esconder a limitação: isso pode reintroduzir eco. Uma implementação multibanda full-band seria outro trabalho com testes próprios, não um pequeno ajuste de configuração.

### 7.4. Testes específicos

**T14:** near-end-only, far-end-only e double-talk, com e sem movimento. Comparar voz limpa processada mesmo sem playback; registrar sinal de voz e eco separadamente nos casos sintéticos. ERLE só é critério de eco em trechos far-end-only com as condições apropriadas.

**T15:** caixas mono/estéreo, canais correlacionados, descorrelacionados e em oposição de fase, referência ausente/incorreta, volume alterado, clipping de alto-falante, troca de sink, retorno dos fones para caixas e saída virtual. O sistema não deve mostrar “cancelamento ativo” sem referência funcional.

**T16:** atrasos de referência de 0/20/80/200 ms e saltos entre eles; drift sintético de ±50/±100/±300 ppm e sessões longas com dispositivos distintos. Os valores são estímulos de teste propostos, não garantia de faixa suportada. Cenários fora da capacidade precisam resultar em estado degradado explícito, não instabilidade.

O candidato só se torna recomendado quando melhorar a preservação da voz em double-talk, sem regressão inaceitável de eco residual, latência ou estabilidade no conjunto de hardware. A avaliação deve separar o efeito do AEC da limitação 16 kHz: comparação na banda comum para o algoritmo e avaliação full-band para o produto final, sem esconder a perda da banda superior.

## 8. DSP-01 — perfil natural e locução com processamento mínimo

### 8.1. Perfil natural

Fazer o perfil natural conservar a voz limpa. AEC apenas quando necessário; um denoiser de atuação calibrada; filtro de graves somente quando indicado ou declarado como parte inseparável do backend. Sem excitador, afinação, alteração de formantes, gate forte ou compressão “rádio” por padrão.

Adicionar diagnóstico de nível de entrada/clipping e instrução de ajuste do ganho físico. Não anunciar que software recupera informação destruída por saturação. Separar otimização da captação da maquiagem do som.

### 8.2. Nivelamento orientado à fala

Implementar um leveler leve reutilizando medição e envelope existentes. Medir energia de fala em janela suavizada, limitar ganho máximo e velocidade de aumento e congelar aumento durante pausas/ruído. Se VAD confiável não estiver disponível, usar comportamento conservador; não adicionar uma rede grande só para decidir o ganho.

Pontos iniciais de ensaio, ajustáveis após medição: aumento máximo de aproximadamente 6 dB, subida de ganho da ordem de 1–2 dB/s e compressão suave somente nos excessos. Redução protetiva de pico é função do limiter, não dessa subida lenta. Não equiparar automaticamente RMS em dBFS, LUFS e intensidade percebida.

### 8.3. Efeitos LSP

Integrar somente os módulos necessários em LADSPA/LV2: de-esser mono, compressor/expansor e limitador. Reutilizar EQ existente quando adequado. Não incluir interfaces gráficas de todos os plugins nem formatos/SDKs sem necessidade.

Para de-esser, comparar processamento por banda e parâmetros conservadores; para baixo atraso, não selecionar modo linear-phase por padrão. A interface deve oferecer poucos controles e leitura de redução de ganho, sem converter todas as portas do plugin em sliders obrigatórios. [S09]

**T17:** sibilância, consoantes suaves, graves, suspiros, pausas e fala distante. Verificar que o leveler não amplifica a sala durante pausas, que o de-esser não apaga inteligibilidade e que o compressor não bombeia. Testar clipping/true-peak e atraso adicional dos modos escolhidos.

## 9. FX-01 — afinação ao vivo e formantes, instaláveis separadamente

### 9.1. Correção de afinação: x42 Auto Tune

Criar o perfil **Canto/afinação** com `fat1.lv2`, sem rede neural. O plugin é voltado a correções pequenas de canto e não fornece correção de formantes. Não prometer transposição extrema natural nem apresentar o efeito como limpeza de ruído. [S08]

Implementar escolha de tonalidade/escala ou notas permitidas, grau/velocidade de correção conforme as portas realmente suportadas, bypass e comparação. Ler metadados LV2 e a latência do plugin; não adivinhar índices de portas. Com tonalidade desconhecida, permitir opção cromática claramente indicada; não inventar detector automático de tonalidade nesta etapa.

Usar o host PipeWire existente quando todas as extensões LV2 necessárias forem atendidas. Testar Atom/URID, portas MIDI vazias válidas, state e worker se exigidos pela variante escolhida. Suporte genérico a LV2 não garante que toda extensão de todo plugin funciona. Se faltar suporte essencial, documentar o bloqueio do recurso e seguir com outras entregas; não iniciar uma DAW/host genérico do zero. [S12]

Por padrão, não executar denoiser de fala no canto: habilitá-lo somente por opção e com testes de vibrato, notas sustentadas e respiração. Evitar gate que corte o final de notas.

### 9.2. Pitch e formantes: Rubber Band

Substituir ou oferecer alternativa ao pitch shifter existente usando a edição GPL da biblioteca/plugin Rubber Band, com preservação/ajuste de formantes quando a API selecionada suportar. Consultar o atraso e tamanho de bloco reais da API; `RubberBandLiveShifter` não significa monitoramento sem atraso. Não adicionar um estágio Rubber Band em série com x42 por padrão; cada um atende a uma função distinta. [S10]

Quando pitch e formantes estiverem neutros, a política de bypass deve continuar temporalmente correta e permitir eliminar o processamento no momento apropriado. Não pagar FFT adicional apenas para o usuário ver o controle em zero.

### 9.3. Retorno para o cantor

Separar sinal enviado aos aplicativos do retorno aos fones. Oferecer retorno direto/seco de baixo atraso ou retorno processado com atraso declarado. Não misturar retorno direto e atrasado sem intenção explícita. Nunca rotear monitor automaticamente para caixas após desconexão de fones; evitar feedback acústico e respeitar volume seguro inicial.

**T18:** tons com erro de ±20/±50 cents, escala restrita, glissando, vibrato, fala não cantada, ruído e pausas; medir erro de nota em regime estável com corretor configurado para correção forte e verificar falsos saltos de oitava. Para formantes, comparar timbre em transposições pequenas, clipping e bypass. Medir retorno real aos fones; nenhuma aprovação apenas porque `run()` é rápido.

## 10. OPT-01 — opcional de maior qualidade/custo, sem dependência obrigatória de GPU

### 10.1. Seleção fechada

Comparar LocalVQE v1.2/v1.3 conjuntos como um único motor de AEC + NS + dereverberação. A versão v1.3 publica pesos F32 em torno de 19 MB, tem custo maior que as opções AEC-only e permanece em 16 kHz. Código/pesos oficiais são Apache-2.0. Há backend Vulkan selecionável pela API C. Nada disso prova superioridade para toda voz ou máquina. [S06]

Usar primeiro CPU com número controlado de threads. Experimentar Vulkan somente se reduzir custo/prazos no hardware alvo. GPU executando os mesmos pesos não é um novo nível de fidelidade. Não portar para CUDA só para marcar presença de aceleração.

### 10.2. Arquitetura e instalação

O pacote adicional deve carregar o modelo e a GPU em processo auxiliar, fora do daemon principal e fora da GUI. Compartilhar áudio por filas limitadas, com os mesmos contratos de sequência, latência e fallback da seção 4. Não aguardar sincronização de GPU no thread de áudio. Enumerar dispositivos fora de RT e selecionar explicitamente o backend efetivo; detectar fallback silencioso para CPU.

Dependências de GPU só entram no subpacote opcional. Não embutir drivers ou runtimes proprietários nos artefatos livres. A base deve instalar e executar sem headers, bibliotecas ou hardware GPU. O sistema de pacotes, e não `pip`/downloads no login, controla a instalação. A infraestrutura pode admitir novos backends no futuro, mas nenhum modelo de licença indefinida entra nessa lista.

No perfil conjunto, desativar os AEC/NS/dereverb anteriores. Fallback não pode conservar o modelo conjunto e somar AEC3. Manter o perfil natural disponível e preservar sua configuração.

### 10.3. Critério de merecimento do pacote

**T19:** comparar o opcional contra a melhor cadeia leve homologada, com volume equiparado, em cenários difíceis e limpos. Medir atraso total, CPU, GPU, memória e disputa com jogo, vídeo e compositor.

Para chamá-lo de superior, exigir preferência auditiva pareada estatisticamente sustentada no cenário anunciado, sem degradação relevante da fala limpa ou da inteligibilidade. Usar intervalos de confiança por locutor/ouvinte, não contar milhares de frames como ouvintes independentes. Não promover por uma diferença mínima em uma métrica isolada. Caso o resultado seja empate, pior ou não medido, manter fora do pacote recomendado e registrar a conclusão.

## 11. UX, configuração e observabilidade

### 11.1. Modos e significado dos controles

Expor **Natural**, **Locução** e **Canto/efeitos**; o perfil conjunto opcional aparece apenas se estiver instalado e homologado. Não multiplicar os menus com cada variante experimental. A escolha detalhada de backend fica no avançado, com finalidade, banda processada, requisitos e atraso.

Separar `preferência`, `configuração efetiva` e `estado observado`. Mostrar “indisponível”, “carregando”, “processando” e “fallback” de forma acessível, sem depender só de cor. Não mostrar “IA ativa” apenas porque existe um nó ou um `.so` no disco.

Preservar configurações por backend e por perfil. Não renumerar os identificadores serializados existentes. Acrescentar versão de esquema e migrações testadas; alterações simultâneas de GUI/CLI/Plasma devem continuar respeitando o lock e a mesclagem existentes. Se uma opção exigir recarga, informar estado pendente até confirmação efetiva.

### 11.2. Diagnóstico

Ampliar `doctor`/probe com:

```text
backend desejado / efetivo e motivo de diferença
versão e hash de plugin/modelo; licença registrada
taxa do grafo / taxa nativa processada / canais
quantum solicitado / efetivo; hop interno
atraso declarado / atraso medido / atraso de filas
referência AEC resolvida / válida / ausente
blocos processados / secos / atrasados / descartados
uso total do worker; requisitos de CPU/GPU e backend efetivo
filtros internos ativos; síntese de banda alta proibida no perfil natural
```

Contadores e logs não devem armazenar fala. Captura diagnóstica deve ser opt-in, limitada, local, com indicação visível e forma de apagar. Não gravar continuamente nem enviar áudio para avaliação externa.

### 11.3. Acessibilidade e tradução

Manter GTK4/Relm4 e os componentes atuais. Labels e mensagens em inglês no código com gettext, traduções pt_BR e sincronização QML/Plasma. Navegação por teclado, foco preservado após atualização, nomes acessíveis e valores anunciáveis. Não colocar operação bloqueante, benchmark ou carga de modelo no main loop GTK.

**T20:** instalar/remover plugin com a GUI aberta, trocar perfil, restaurar defaults, atualizar por CLI, ligar/desligar pelo applet e fechar a GUI durante preparação. Preferências e estado observado continuam coerentes; não deixar processo/microfone ativo por vazamento de lifecycle.

## 12. Plano de testes e critérios de aceitação

### 12.1. Evidência em camadas

| Camada | O que comprova | O que não comprova |
|---|---|---|
| Unitários/propriedades | Matemática, estados, limites, migração e invariantes | Qualidade acústica do plugin instalado |
| ABI e DSP do plugin real | Contrato de portas, framing, sinais de saída e memória | Roteamento real, clocks físicos ou retorno aos fones |
| Grafo PipeWire isolado | Links, serviços, controles, lifecycle e negociação | Comportamento em toda placa/sala |
| Hardware ao vivo | Atraso, estabilidade e áudio no conjunto testado | Superioridade universal |
| Escuta pareada cega | Preferência e preservação no corpus avaliado | Garantia para vozes/ambientes não testados |

Reutilizar as suítes existentes. Executar testes focalizados por patch; suíte completa e matriz de hardware nos marcos de integração, não dezenas de benchmarks completos por pequena alteração de texto.

### 12.2. Testes adicionais obrigatórios

**T21 — causalidade e continuidade.** Alterar amostras futuras não pode mudar saída já emitida. Não usar STFT centrada ou padding de futuro no runtime se a referência foi treinada/exportada para framing causal. Um teste em arquivo precisa simular entrega por blocos com estado persistente; não carregar a gravação inteira na rede e chamar isso de streaming.

**T22 — contratos de formato.** Validar float, clipping e conversões exigidas por cada biblioteca. Testar 16/44,1/48/96 kHz na entrada do host, identificando quais são nativas e quais exigem conversão; backend não suporta taxa arbitrária só porque o host aceita. Testar mono, dual-mono e referência estéreo; contagem exata de amostras após resampling e buffers de tamanho variável.

**T23 — regressão de pacote.** Instalação limpa, atualização a partir da versão anterior, configuração antiga/malformada, plugin removido/atualizado, runtime ausente, sem internet, sem GPU e CPU sem AVX2. Não sobrescrever JSON inválido silenciosamente. Nenhum modelo deve ficar aparentemente ativo sem processamento.

**T24 — integração com aplicativos.** Validar clientes nativos e browser/WebRTC, inclusive aplicativos sandboxed via o mecanismo de áudio já suportado no host. Não mudar configurações privadas do browser à força. Verificar o caso de dois AECs/NS em cadeia porque o aplicativo também processa áudio; oferecer instrução/diagnóstico e modo de compatibilidade. Comparar áudio antes e depois dessa segunda camada para localizar degradação.

**T25 — privacidade e ciclo de vida.** Sem consumidor, nenhum modelo deve executar continuamente e o microfone não pode ser mantido ativo por playback, medidor oculto ou vazamento. Desligar o recurso/fechar monitor interrompe a captura correspondente. Os logs padrão não contêm áudio, transcrições, nomes de arquivo pessoais ou amostras dos usuários. Fones desconectados não devem criar loop de monitor em caixas.

### 12.3. Corpus mínimo e coleta ao vivo

Montar um corpus de homologação com ao menos 8 locutores em português, incluindo vozes graves/agudas, intensidades suaves/fortes, sotaques distintos e frases com muitas fricativas. Para canto, adicionar escalas, vibrato, notas sustentadas e intervalos. Incluir respiração e expressão vocal não neutra para detectar supressão indevida. Não usar todo o corpus para escolher parâmetros e depois apresentá-lo como avaliação independente: reservar locutores e ambientes de teste.

Usar pelo menos microfone interno de notebook, webcam USB e microfone/headset comum. Testar fones, caixas do notebook e caixas externas; incluir captação e reprodução em interfaces diferentes. Um array só entra em testes espaciais se houver canais físicos independentes comprovados; dual-mono não conta.

Ruídos: ventoinha, teclado, tráfego, fala concorrente, impulsos e sala reverberante. Incluir silêncio, voz já limpa, playback musical e fala dos alto-falantes. Fazer comparação simultânea a partir da mesma captação quando tecnicamente possível; ensaios sucessivos com pessoas falando diferente não isolam o efeito do algoritmo.

A coleta não deve ser um recurso de gravação do produto. Utilizar ferramenta diagnóstica opt-in, amostras sintéticas ou corpus permitido. Registrar consentimento e permissão de distribuição, separando material privado dos artefatos públicos.

### 12.4. Métricas

**Preservação:** ganho real da voz, resposta espectral, erro/ruído adicionado em entrada limpa, SI-SDR quando existir referência apropriada, distância espectral com fórmula e banda documentadas, cortes nos primeiros/finais de fonemas e escuta pareada. Equalizar loudness somente na comparação perceptual; guardar também a diferença de nível original para não ocultar perda de ganho.

**Eco:** eco residual, tempo de convergência/reconvergência e ERLE em far-end-only. Não usar ERLE de mistura com near-end como prova de bom AEC. Em double-talk, avaliar simultaneamente voz local e eco restante. Para avaliar cadeias limitadas em banda, registrar tanto métricas na banda comum quanto o resultado perceptual do áudio final.

**Tempo real:** duração do callback e da inferência, p50/p95/p99/p99,9, máximo, deadlines perdidos, xruns, fila máxima, idade máxima da resposta e blocos em fallback. Um benchmark de poucos segundos não estima confiavelmente p99,9. Guardar contagens e série/resumo suficientes para reproduzir os percentis.

**Recursos:** CPU total em equivalentes de um núcleo, threads, RSS/VRAM, uso e memória de GPU, frequência/temperatura quando disponíveis, falhas de página e comportamento em bateria. Não confundir 10% de uma CPU de vários núcleos com 10% de um núcleo.

**Licenciamento das métricas:** começar com cálculos numéricos implementáveis com bibliotecas livres já aprovadas e escuta humana. Não acrescentar pesos MOS ou ferramenta de métrica com restrição de uso. Ausência de uma métrica proprietária não bloqueia a implementação nem autoriza inventar valores.

### 12.5. Latência: medir três coisas separadas

1. **Atraso do algoritmo/adaptador:** medido por sinais e sequência de amostras; inclui janelas, lookahead e buffering interno.
2. **Tempo de computação:** tempo gasto para produzir um bloco. Não substitui a primeira medida.
3. **Atraso real do produto:** microfone físico → fonte virtual/aplicativo e, separadamente, microfone → retorno físico aos fones.

Medir com bypass equivalente como baseline, controlando taxas/quantum e relatando a diferença atribuível ao processamento. Não incluir latência de rede em uma medida e excluí-la na outra. Para efeitos que alteram o sinal ou pitch, não depender exclusivamente de correlação de waveform: usar marcadores e instrumentação apropriados.

As metas abaixo são **critérios de engenharia propostos**, não valores garantidos pelos projetos:

| Caminho | Meta inicial | Tratamento se não atingir |
|---|---|---|
| Denoiser natural leve, sem AEC | Até 30 ms adicionais sobre bypass equivalente | Investigar framing/filas; não promover como baixa latência sem explicar |
| AEC + denoiser natural | Alvo até 60 ms adicionais; acima de 80 ms exige perfil/aviso específico | Não esconder atraso atrás de “tempo real” |
| Retorno aos fones para canto | Alvo até 10 ms totais; preferir alternativa direta quando passar de 20 ms | Informar atraso; não prometer retorno imperceptível com qualquer autotuner |
| Opcional de maior custo para conversa | Alvo até 100 ms adicionais, somente opt-in | Se incompatível com conversa interativa, não recomendá-lo para esse uso |
| DSP próprio de locução | Menor atraso possível, idealmente até 5 ms adicionais | Contabilizar lookahead/oversampling real de cada plugin |

Os alvos de retorno são para rotas cabeadas/USB controladas. Bluetooth e outros dispositivos podem impor atraso superior antes do processamento; registrar essa limitação sem imputá-la ao modelo.

### 12.6. Limites de aceitação objetiva

- Caminho explicitamente de identidade, sem filtros ou conversões: erro RMS alvo abaixo de −100 dBFS para entrada nominal e ganho unitário; comparação alinhada ao atraso declarado. Onde houver resampling, usar tolerância definida para o conversor, não alegar identidade bit a bit.
- Nenhuma amostra NaN/Inf produzida; nenhuma leitura/escrita fora de limites; nenhum estado de uma instância contaminando outra.
- Caminho seco/fallback: erro de ganho abaixo de ±0,1 dB no teste de referência, sem conteúdo antigo. As tolerâncias são propostas e devem ser registradas antes de avaliar o patch.
- RT: p99,9 do callback abaixo de 50% do período no hardware declarado compatível, deixando margem ao grafo. Medir custo agregado dos hops por callback; uma chamada pode executar mais de uma inferência.
- Em capacidade normal homologada: zero xruns atribuíveis ao componente e zero fallback recorrente por falta de capacidade durante ensaio de 30 minutos sob carga definida. Sob sobrecarga deliberada: fallback limitado, sem bloqueio, acúmulo crescente de latência ou travamento.
- Sessão de 2 horas em marco de release: nenhum crescimento contínuo de filas/RSS após aquecimento, nenhum aumento acumulativo de atraso. Registrar indisponibilidade de hardware como teste não executado, nunca aprovado.
- Sem consumidor: zero inferência periódica do backend. Medição gráfica/visualização deve poder ser suspensa independentemente.
- Qualidade perceptual: nenhum default novo sem teste de voz limpa e double-talk, corpus reservado e comparação com o baseline instalado. Não aumentar artificialmente o volume do candidato nem retirar amostras problemáticas depois de ver o resultado.

### 12.7. Execução eficiente e ferramentas

Confirmar os comandos existentes na revisão atual. A arquitetura do projeto referencia:

```bash
scripts/quality-check.sh --ci
scripts/test-ui.sh
scripts/test-miri.sh
```

Esses comandos não substituem ensaio acústico. Acrescentar targets reproduzíveis para T01–T25 nos harnesses adequados, sem criar uma segunda suíte gigante independente. Testes nativos de FFI/C devem incluir ASan/UBSan e, onde aplicável, TSan; Miri é útil para partes Rust compatíveis, não comprova segurança da inferência C/ONNX.

Separar: testes rápidos por commit; integração por mudança de DSP/ABI/roteamento; hardware/percepção e soak em releases. Falta de modelo instalado deve marcar skip explicitamente, e um gate crítico com skip não permite promover o backend. Não rebaixar limiares nem trocar sinais depois de uma falha só para fazer a suíte passar.

## 13. Empacotamento e atualização

Os nomes abaixo são **propostas**; reutilizar os pacotes existentes e evitar duplicação:

| Grupo | Conteúdo | Instalação |
|---|---|---|
| Base atual | GUI/CLI/loaders, DSP e backend leve homologado | Sem GPU e sem rede na primeira captura |
| FastEnhancer nativo | Adaptador, biblioteca e blob MIT fixados | Pacote pequeno; detectar suporte da CPU |
| AEC alternativo | Runtime e modelos AEC-only homologados | Selecionável, com indicação de 16 kHz quando aplicável |
| Efeitos de voz | x42, módulos LSP e Rubber Band necessários | Opt-in, sem ativar efeitos no perfil natural |
| VQE avançado | LocalVQE conjunto e modelo aprovado | Opt-in; subpacote Vulkan separado quando útil |

Regras de pacote:

- Build com fontes/revisões e submódulos fixados, lockfiles, hashes e dependências de build separadas do runtime. Não usar download de `main` durante `prepare()` como fonte irreproduzível.
- Modelos locais, imutáveis e instalados pelo gerenciador. Validar integridade e compatibilidade antes de mapear/carregar. Não desserializar checkpoints executáveis de origem arbitrária no usuário final.
- Instalar apenas LADSPA/LV2 necessários. Evitar SDKs/formats sem necessidade e manter todas as licenças correspondentes. Usar dependências existentes da distribuição quando compatíveis.
- Definir versão mínima/ABI de plugin quando a GUI depender de comportamento corrigido. Um hash diferente exige invalidar caches de capacidade/benchmark pertinentes.
- Nenhum hook altera arquivos privados dos usuários ou inicia captação automaticamente. Não requerer root para inferência ou para mudanças de qualidade.
- Falha de GPU, runtime ou pacote adicional não impede iniciar a base. A desinstalação de um opcional não deve corromper preferências: conservar intenção e indicar indisponibilidade.
- Não tratar receita Flatpak, manifesto válido ou build de CI como prova de integração de áudio instalada. Validar a rota nativa do BigLinux antes de expandir formatos.

**T23/T25 são gates de pacote**, não apenas testes de desenvolvimento. Guardar uma VM/imagem de instalação limpa reproduzível, sem exigir acesso aos repositórios de build para executar o produto instalado.

## 14. Sequência de entregas para o agente

### PR-01 — regressões do processamento existente

Registrar baseline e evidências; implementar FIX-01 a FIX-05 nos componentes corretos e versões correspondentes. Entregar testes T01–T05 e pacote de plugin compatível com o aplicativo. Não modificar simultaneamente todos os defaults acústicos: facilitar atribuição de ganho/perda.

### PR-02 — perfil natural e contratos reais

Implementar FIX-06 a FIX-10, capacidades/estado por backend e diagnóstico. Preservar migrações, correções recentes e configurações antigas. Entregar T06–T10, T20 e evidências de ausência de regressão do grafo.

### PR-03 — FastEnhancer nativo

Implementar NS-01, inclusive contexto por instância/isolamento e fallback sem AVX2. Entregar T11–T13/T21/T22 e comparação do plugin real com a referência. Não torná-lo default antes da avaliação acústica e de CPU.

### PR-04 — backends alternativos de AEC

Implementar AEC-01, referência correta, adaptação de blocos e política de ativação. Entregar T14–T16 e comparação transparente com AEC3. Oferecer escolha explícita; a promoção para default é decisão baseada no conjunto de resultados, não consequência automática do merge.

### PR-05 — locução e canto opcionais

Implementar DSP-01 e FX-01 sem alterar o modo natural. Entregar T17/T18/T20, controle de monitor seguro, metadados de latência e pacotes separados.

### PR-06 — VQE avançado, somente após gate de mérito

Implementar/empacotar OPT-01 apenas se T19 demonstrar benefício suficiente. Se não houver ganho, documentar a rejeição e não aumentar dependências. Não substituir o modelo selecionado por outro de licença incerta para forçar um “modo GPU”.

### PR-07 — homologação integrada e release

Executar T21–T25, matriz de hardware, escuta reservada e soak. Conferir todos os manifestos de licença e hashes. Entregar instruções de instalação/remoção, compatibilidade com aplicativos, limites conhecidos e rollback.

Esses PRs podem ser divididos em patches menores, mantendo a ordem de dependências. Um bloqueio em efeito musical ou hardware GPU não bloqueia correções P0 e áudio natural. Não criar/mesclar PRs no repositório do usuário sem seguir o fluxo de autorização de escrita estabelecido no ambiente do agente.

## 15. Definição de concluído e formato da entrega

Cada entrega deve conter código e testes, lista dos arquivos alterados, pacotes/fontes correspondentes e um relatório curto de evidências. Usar estados separados: `implementado`, `compilado`, `testado`, `homologado-em-hardware` e `não-executado`.

Estrutura proposta, adaptável à existente:

```text
artifacts/audio-live/<commit>/
    environment.json
    components-and-models.json
    test-results.json
    latency.csv
    realtime-percentiles.csv
    quality-results.csv
    listening-summary.md
    limitations.md
    SHA256SUMS
```

Não criar arquivos vazios para simular evidência. Sinais privados não entram em artefatos públicos. O relatório informa taxa, quantum, modelo/hash, rota, controles, CPU/GPU, baseline, comando e dataset/estímulo usado. Registrar amostras/rodadas e skips; screenshots, compilação ou comentários dizendo “medido” não substituem logs correspondentes.

### Checklist final

- [ ] A cadeia ao vivo permanece utilizável em máquina sem GPU e sem internet.
- [ ] Os defeitos reproduzidos em FIX-01–FIX-10 foram corrigidos e têm regressões nos componentes corretos.
- [ ] Não há trabalho bloqueante ou carga de modelo no callback; filas e estados têm limites e ownership claros.
- [ ] Bypass/fallback preservam a linha temporal, não alteram ganho indevidamente e não vazam áudio entre instâncias.
- [ ] O perfil natural não sintetiza agudos, afina notas nem modifica formantes silenciosamente.
- [ ] AEC pode ser selecionado independentemente do denoiser; desativar AEC3 é respeitado.
- [ ] Referência válida, clocks, banda efetiva e latência estão observáveis.
- [ ] Efeitos e recursos custosos são opcionais; seus controles correspondem a funções reais.
- [ ] Pacotes/modelos têm fontes, hashes e evidência de licença livre; nenhum artefato pendente entrou na distribuição.
- [ ] Resultados de qualidade e custo vêm dos plugins/grafo reais, com limites conhecidos e sem extrapolar para hardware não testado.
- [ ] Migração, CLI/GUI/Plasma, instalação/remoção, acessibilidade e tradução foram validadas.

**Instrução final ao implementador:** iniciar pelo baseline e pelo PR-01. Entregar correções funcionais e verificáveis antes de adicionar efeitos ou modelos. Não afirmar substituição bem-sucedida de AEC3 nem “qualidade superior” sem evidência ao vivo. Se uma avaliação falhar, preservar o caminho funcional, registrar a limitação e continuar os itens independentes.

## 16. Fontes primárias e rastreabilidade

Consultadas nesta preparação ou presentes nos fontes examinados anteriormente na mesma conversa. Referências a `main`/`master` são pontos de descoberta; o agente deve fixar a revisão efetivamente utilizada no manifesto de build. Esta lista documenta fatos dos projetos, não certifica os futuros pacotes do BigLinux.

**[S01] Aplicativo BigLinux — baseline e implementação.**

- [Árvore no commit de referência](https://github.com/biglinux/biglinux-noise-reduction-pipewire/tree/7906ae269dd5ca03e6c4aee3e43bfe1ed6e5c1d0).
- Nesse commit: `ARCHITECTURE.md`, `README.md`, `src/pipeline/{echo_cancel,mic,gain_safety}.rs`, `src/config/{audio,noise_model,plugin_cost,quality}.rs`, `src/services/echo.rs` e `src/config.rs`.
- [Branch conferida pelo conector](https://api.github.com/repos/biglinux/biglinux-noise-reduction-pipewire/branches/main).

**[S02] GTCRN LADSPA — fontes da auditoria.**

- [Diretório no commit de referência](https://github.com/biglinux/gtcrn-ladspa/tree/6ee156e8bcbb7fb8ec454cfc2ee76e305bda2593/ladspa/src).
- Arquivos relevantes: `plugin.rs`, `model.rs`, `external_controls.rs`, `stft.rs`, `gate_pass.rs` e `biquad.rs`.

**[S03] Plugins já utilizados pelo BigLinux.**

- [DPDFNet LADSPA](https://github.com/biglinux/dpdfnet-ladspa), especialmente `src/lib.rs`, `src/engine.rs` e os testes de deadline/falha.
- [Pacote DeepFilterNet LADSPA do BigLinux](https://github.com/biglinux/deepfilternet-ladspa).
- [DeepFilterNet upstream](https://github.com/Rikorose/DeepFilterNet).

**[S04] faster-enhancer.c — runtime, requisitos e pesos.**

- [README e requisitos](https://github.com/kdrkdrkdr/faster-enhancer.c).
- [NOTICE: atribuição do blob `fe.q8`](https://github.com/kdrkdrkdr/faster-enhancer.c/blob/main/NOTICE).
- [LICENSE](https://github.com/kdrkdrkdr/faster-enhancer.c/blob/main/LICENSE).

**[S04b] Estado global da API do runtime C.**

- [`src/fe_pipeline.c`](https://github.com/kdrkdrkdr/faster-enhancer.c/blob/main/src/fe_pipeline.c), blob consultado `ca908d6adf8bb16e6670baf2a88115996974566a`.

**[S05] FastEnhancer original — referência de modelo e framing.**

- [Repositório e licença](https://github.com/aask1357/fastenhancer).
- [Inferência ONNX incremental e caches](https://aask1357.github.io/fastenhancer/onnx/).
- [Releases oficiais; separar variantes/taxas](https://github.com/aask1357/fastenhancer/releases).

**[S06] LocalVQE — código, pesos e limitações.**

- [Repositório e licença Apache-2.0](https://github.com/localai-org/LocalVQE).
- [Model card e publicação oficial dos pesos Apache-2.0](https://huggingface.co/LocalAI-io/LocalVQE).
- O model card distingue AEC-only, variantes conjuntas, taxa 16 kHz, custo e referência far-end; comparações publicadas não são benchmarks nossos.

**[S06b] LocalVQE — API de integração.**

- [`ggml/localvqe_api.h`](https://github.com/localai-org/LocalVQE/blob/main/ggml/localvqe_api.h), blob consultado `841f050f02f1070ecdc0a0e776c04192c9f97696`.
- Consultar chamadas por quadro, opções de backend/dispositivo/threads e frontend adicional. Não confundir com a chamada de clip completo.

**[S07] SpeexDSP — licença e API.**

- [COPYING atual no repositório](https://github.com/xiph/speexdsp/blob/master/COPYING).
- [Manual oficial: AEC, referência, preprocessor e resampling](https://www.speex.org/docs/manual/speex-manual/node7.html). O manual é histórico; conferir headers e comportamento da versão efetivamente empacotada.

**[S08] x42 Auto Tune — capacidades e licença.**

- [Projeto `fat1.lv2`](https://github.com/x42/fat1.lv2).
- [Descrição oficial e ausência de correção de formantes](https://x42-plugins.com/x42/x42-autotune).
- [Cabeçalho GPL-2.0-or-later em `src/fat1.cc`](https://github.com/x42/fat1.lv2/blob/master/src/fat1.cc), blob consultado `048b5c965d965029de296fd7957e8a416fad68ee`.

**[S09] LSP Plugins — licença e de-esser.**

- [README, seção Licensing, e opções de build](https://github.com/lsp-plugins/lsp-plugins/blob/master/README.md).
- [Manual oficial do de-esser mono](https://lsp-plug.in/?page=manuals&section=deesser_mono).

**[S10] Rubber Band — edição livre e API ao vivo.**

- [Licenciamento GPL-2.0-or-later](https://breakfastquay.com/rubberband/license.html).
- [API `RubberBandLiveShifter`](https://breakfastquay.com/rubberband/code-doc/classRubberBand_1_1RubberBandLiveShifter.html).

**[S11] Critério de seleção.**

A seleção da seção 3 é uma decisão técnica deste plano baseada nas fontes S04–S10. Não existe um benchmark único demonstrando que todos esses componentes são superiores aos atuais. Obrigações de empacotamento devem acompanhar os textos de licença de cada versão/arquivo; dúvidas sobre um novo artefato impedem sua inclusão, não autorizam presumir permissão.

**[S12] PipeWire — integrações oficiais.**

- [Filter-chain: LADSPA/LV2 e exemplos](https://docs.pipewire.org/page_module_filter_chain.html).
- [Módulo echo-cancel](https://docs.pipewire.org/page_module_echo_cancel.html).
- Os nós, propriedades e extensões aceitos devem ser confirmados na versão distribuída. A existência de um exemplo não substitui teste com o plugin e o grafo do BigLinux.
