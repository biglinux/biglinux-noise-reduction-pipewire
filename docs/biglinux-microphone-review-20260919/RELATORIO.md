# Revisão técnica — desempenho do volume automático e confiabilidade

Data: 19/09/2026. Origem e hashes: `MANIFEST.json`. Estado: patches concretos, validação local parcial; **integração Rust e homologação acústica pendentes**.

## 1. Diagnóstico e alcance

O custo evitável não estava apenas no cálculo do ganho. Havia uma FFT/análise espectral a cada 512 amostras, trabalho de consulta do grafo por subprocessos, um cache Python que nunca reutilizava corretamente os pesos espectrais e consulta do grafo no mesmo fluxo responsável por escoar PCM. Na parte funcional, calibração e daemon tinham caminhos de atuação diferentes, com garantias insuficientes sobre leitura de retorno, atualidade da observação e posse do volume.

A revisão aprofundou AGC, monitor/captura, fontes PipeWire, transações de configuração, CLI, perfis, latência, prévia de quantum e comunicação assíncrona da interface. Foi feito inventário do restante do código e verificação sintática/políticas disponíveis. **Inventariar ou executar testes sintáticos não equivale a provar, linha por linha, a correção semântica dos 138 arquivos/32.220 linhas inventariados.** Nenhum terceiro pode honestamente inferir ausência de todos os bugs destes resultados. Vendor ausente, plugins DSP externos, drivers e execução GTK não foram certificados.

Não foi feita uma troca de modelo de redução de ruído, tradução em massa, redesign do espectro ou reimplementação do mixer ALSA. Os achados abaixo se referem ao código do anexo, não a uma suposição sobre o estado atual de outro repositório.

## 2. Mudanças no caminho crítico

### 2.1 Taxa de análise e medição completa dos blocos — patches 02/08

A 48 kHz, o salto anterior de 512 amostras produzia 93,75 análises/s. A configuração dedicada do AGC passa a salto de 2.400 amostras: **20 análises/s**, com FFT de 2.048. É uma redução aritmética de **78,67% no número de análises espectrais**, não uma medição de CPU do processo. O espectro da UI não foi simplesmente reduzido para compensar o backend.

Como o salto ultrapassa a janela FFT, o pico e o RMS são calculados sobre **todas as 2.400 amostras**, incluindo as 352 que não entram na janela espectral final. Acumulação de energia em f64 e rejeição de PCM não finito evitam contaminar o controle. O espectro continua representando uma janela, não todo o salto.

O monitor do AGC solicita 50 ms de latência; o padrão dos outros usos continua 100 ms. Isso é uma solicitação, não uma promessa de latência ponta a ponta. A documentação explica que reduzir buffers eleva o overhead [D1]; medir a consequência real em equipamentos antigos é obrigatório.

`captured_at` indica quando o bloco chegou ao processo, e `capture_epoch` diferencia reinícios. **Não é timestamp de ADC.** A fila latest-only continua limitada: se uma atualização não for entregue, um pico intermediário pode ser descartado. Medição de pico por bloco não transforma o AGC em limitador de true peak ou proteção infalível contra clipping.

### 2.2 Cache de grafo por eventos — patches 04/05/08/13

`source_watch.rs` mantém identidade de nodes, links, default metadata e cookie do core em um cliente PipeWire dedicado. Elimina as consultas periódicas de grafo/demanda por CLI no caminho do daemon. Os objetos nativos ficam na própria thread; o controle lê um cache pequeno, limitado a 4.096 nodes, 16.384 links e 8 objetos de metadata. Excesso, ambiguidade ou perda da conexão fecham a autorização, em vez de selecionar uma fonte por aproximação.

O startup aguarda sincronização também depois de vincular a metadata; esperar só o sync inicial do registro podia expor um default ainda incompleto. O encerramento usa canal, não timer periódico dedicado só para descobrir que deve sair [D4–D6]. Há custo adicional de uma thread persistente, a comparar com o custo retirado das consultas.

**As escritas de volume não foram convertidas em acesso direto ao node físico.** Continuam em `wpctl`, com prazo, limite de 300% e leitura de retorno. Isso conserva a política WirePlumber de mixer/route/ALSA [D2/D3]. Uma otimização nativa posterior precisa integrar corretamente `mixer-api` e confirmação assíncrona; escrever simplesmente `Props.volume` não foi presumido equivalente.

O limite de cinco escritas por segundo não significa cinco subprocessos: uma escrita pode exigir leitura antes, comando e leitura depois. Ainda existe custo de subprocessos durante convergência. Os ganhos totais de CPU do daemon dependem do teste A/B.

### 2.3 Controle consistente, sem acumulação impossível — patches 03/06/07/08/11/16

O integrador fica limitado à janela realmente autorizada, enquanto a capacidade usada para reconhecer fala fraca continua considerando a faixa física de ganho. Separar esses conceitos evita corrigir anti-windup e, inadvertidamente, tornar o controlador incapaz de reconhecer uma voz que ainda precisa de amplificação.

Mudança de volume externo rebasa todos os históricos normalizados. Observação não confiável, interrupção, retorno de idle e troca de geração não prolongam evidência de fala antiga. A janela em porcentagens usa arredondamento para dentro e tolerância para limites inteiros representados em f32. Mute/zero não disparam elevação automática. Alteração manual fora da janela suspende a atuação; dentro da janela é rebaseada segundo a semântica existente, não um desligamento universal do AGC.

O retorno real de `wpctl` deve ser finito, plausível e compatível com o pedido. Não se contabiliza ganho como se um comando sem confirmação tivesse necessariamente sido aplicado. Se o hardware quantiza e devolve o mesmo valor, o controlador pode acumular a correção necessária para o próximo degrau, sem repetir a mesma solicitação inutilmente.

O teto de 300% foi preservado. Na convenção já usada pelo projeto, `dB = 60 × log10(fração_wpctl)`, não `20 × log10`. Essa relação é do modelo de controle; não prova a curva acústica, o ganho analógico ou a calibração em dB SPL de cada microfone.

### 2.4 Calibração com posse, prazo e confirmação — patches 07/08/13/16

Calibração e daemon compartilham o mesmo arquivo de exclusão do atuador. O daemon usa um descritor persistente, sem abrir/criar o arquivo a cada tick. A calibração mantém a posse até a persistência do resultado, não apenas até terminar a captura. O lock de configurações é separado: não fica preso durante toda a fala.

Sem áudio por três segundos é erro. O período de escuta pedido e a tolerância adicional são avaliados em todos os caminhos, inclusive fila vazia e recuperação. No CLI, a escuta é de 8 s com até 12 s adicionais; **isso não é o limite total do subprocesso**, pois há aquisição de lock, startup, I/O com prazo e encerramento. Parâmetros de duração inválidos são rejeitados antes de I/O.

Não existe mais o salto cego de 30% para 100% para iniciar a calibração. A voz fraca pode justificar ganho; o silêncio não. O resultado bem-sucedido só é impresso depois de salvar. Falha ao salvar é erro explícito, inclusive avisando que o volume pode já ter mudado.

Em erro ou ausência de voz, restaura-se a entrada somente se o último volume confirmado ainda é nosso, na mesma geração, sem mute/intervenção externa. Retorno desconhecido não autoriza rollback cego. `heard_speech` continua significando que houve evidência suficiente, **não prova convergência perfeita** ao alvo: uma passagem que chega ao prazo máximo precisa ser avaliada quanto à qualidade, mesmo que tenha ouvido fala.

## 3. Achados e correções rastreáveis

“Alta” indica risco de atuação incorreta, bloqueio ou perda/falseamento de estado. “Média” indica desempenho, medição ou comportamento de UI. A confiança é na identificação pelo código; execução nativa permanece pendente.

| ID | Impacto / confiança | Local e causa | Correção / patches | Validação disponível |
|---|---|---|---|---|
| A01 | Média / alta | `ui/loudness.py`: identidade comparada com novo `id(...)`; pesos recalculados | Reter array e pesos de potência; produto escalar; 01 | Testes Python e microbenchmark |
| A02 | Média / alta | `LoudnessMeter`: bloco de 85,33 ms tratado como 100 ms | Integração por energia e duração real, corte parcial na janela; 01 | Testes Python |
| A03 | Média / alta | `agc_monitor_config`: análise mais rápida que o controle | Configuração própria de 20 Hz; 02/08 | Derivação de taxa; Rust NOT_RUN |
| A04 | Alta / alta | Pico poderia perder amostras com hop maior que FFT | Pico/RMS do bloco completo; 02 | Teste Rust novo, NOT_RUN |
| A05 | Alta / alta | Integrador acumulava fora da janela atuável | Anti-windup e alcance de evidência separados; 03/06/11 | Testes Rust, NOT_RUN |
| A06 | Alta / alta | Histórico normalizado ficava incoerente após ganho externo | Rebase dos cinco históricos; 03/08 | Testes Rust, NOT_RUN |
| A07 | Alta / alta | Fila atrasada/reinício reutilizava evidência | Clock, epoch, expiração e invalidação após ACK; 02/08 | Testes Rust, NOT_RUN |
| A08 | Alta / alta | Calibração e daemon disputavam volume | Lease compartilhada até o save; 07/08/16 | Testes de lock Rust, NOT_RUN |
| A09 | Alta / alta | Falha de áudio mantinha espera; ganho inicial cego | Prazo em todos os caminhos, stall e fala requerida; 07 | Testes de duração Rust; hardware pendente |
| A10 | Alta / alta | Valor solicitado era tratado como aplicado | Readback por escrita; erro/falha não vira ganho; 04/07/08 | Rust/hardware pendentes |
| A11 | Alta / alta | IDs/default podiam pertencer a snapshots diferentes | Snapshot JSON único; nome+serial+cookie; 04/05/08 | Testes de parsing/modelo Rust, NOT_RUN |
| A12 | Média / alta | Consultas regulares do grafo criavam processos | Cache nativo por eventos; 05/08 | Revisão API; A/B pendente |
| A13 | Alta / alta | Fonte trocada sem comprovar aplicação | Rejeitar duplicidade e confirmar default/generation; 13 | Testes Rust, NOT_RUN |
| A14 | Alta / alta | CLI emitia sucesso antes da persistência | Save antes do JSON positivo; erro preservado na UI; 13/16 | 15 casos Python; Rust pendente |
| A15 | Alta / alta | Merge de configurações ressuscitava remoções/perdia campos futuros | Merge com distinção ausência/null, conflitos e mapas de propriedade; 09 | Testes Rust, NOT_RUN |
| A16 | Média / alta | Consulta do grafo bloqueava o escoamento do espectro | Worker de consulta separado, timeout e estado anterior em falha; 10 | Inspeção; GTK real pendente |
| A17 | Alta / alta | Leitura de pipe supunha bloco completo e poderia travar | Montagem de short reads, selector, prazo e EOF; 10 | Nove testes com pipes locais |
| A18 | Média / alta | Recargas/escritas concorrentes cresciam e completavam fora de ordem | Uma recarga pendente; um write ativo e último valor por chave; 10/12 | Testes de modelo GLib |
| A19 | Média / alta | Callback verdadeiro se repetia; throttle entregava valor antigo | Callback one-shot, cancelar timer obsoleto; 12/15 | Testes de modelo GLib |
| A20 | Média / alta | Fonte vazia mostrava calibração de outro microfone | Não selecionar dispositivo de fallback indevido; 10 | Testes de estado |
| A21 | Média / alta | Dados inválidos/NaN/bool atravessavam contratos de UI | Validação de tipos, finitude e janela; 11/13 | Testes Python |
| A22 | Média / alta | Falha de readback criava retry a cada tick | Tentativas no máximo 1 Hz, retomada explicitamente fresca; 13 | Revisão estática; hardware pendente |
| A23 | Média / alta | Latência usava quantum de outro grupo e chamava estimativas de medidas | Resolver driver do microfone no último batch; proveniência conservadora; 14 | Testes Rust, NOT_RUN |
| A24 | Alta / alta | Prévia: `poll` seguido de `read_line` podia travar em linha parcial | Prazo total do handshake byte a byte e limpeza; 17 | Três testes Rust, NOT_RUN |
| A25 | Média / alta | Tap fechado podia ser iniciado de novo | Rejeitar `start` depois de `close`; 16 | Revisão estática; GTK pendente |

## 4. Medições e evidências locais

`evidence/loudness-benchmark.json` preserva todas as rodadas. Python 3.13.5, NumPy 2.3.5, sinal senoidal fixo de 997 Hz, 4.096 amostras, 48 kHz; sete rodadas alternadas, 1.500 chamadas por implementação por rodada, 60 chamadas de aquecimento. FFT/amplitude são pré-calculadas: o custo medido é exclusivamente `push_block`.

Medianas: base **228,707773 µs**, patch **19,104789 µs**, razão **11,9712×**, redução **91,6466% nesse trecho**. Resultado estacionário das duas implementações concordou numericamente. Um sinal estacionário não valida a resposta transitória nem certifica BS.1770/LUFS; o código explicita tratar-se de aproximação espectral ponderada, não de medidor normativo certificado.

Os logs `final-*` são os resultados repetidos sobre a árvore reconstruída a partir da base com os patches finais. Foram aprovados 66 testes Python (+33 subtestes), 19 testes de estado/agendamento com import shim de GLib e 2 testes JavaScript. Quatro testes adicionais dos parsers do coletor não exercitam PipeWire. Os 47 arquivos Python passaram por AST, não por todos os caminhos de execução. O teste integral inicial encontrou PyGObject ausente; esse log foi preservado, não ocultado.

Foram adicionadas 29 funções de teste Rust, listadas em `new-rust-tests-not-run.json`; nenhuma foi executada. A aplicação/reversão verifica bytes, não compilação. `MANIFEST.json` registra hashes antes/depois dos 33 arquivos alterados. Nenhum cache Python ou binário é distribuído como patch.

## 5. Riscos remanescentes e condições para liberar

**Atuação externa não atômica.** Conferir cookie/serial/nome antes e depois diminui risco, mas `wpctl` continua recebendo ID numérico; existe janela entre conferir e escrever. Não é compare-and-swap atômico. Outra aplicação também pode mudar ganho nesse intervalo. Testar hotplug/reutilização de ID intensivamente; uma solução absoluta exige atuação nativa com identidade e política corretas, não apenas novas verificações textuais.

**Latência acústica não inferível do ACK.** Um readback confirma o volume exposto pelo mixer, não o instante em que todas as amostras do filtro passaram a refletir esse ganho. `captured_at` e descarte pós-ACK não eliminam atraso ADC/filtro/fila. Medir degrau, overshoot e estabilidade. Constantes de atraso dos denoisers foram mantidas como estimativas do projeto, não remensuradas.

**Heurística não identifica o dono da voz.** Tilt, pico, modulação, SNR e perfil calibrado não são autenticação de locutor. TV, rádio ou outra pessoa podem produzir evidências semelhantes. Não prometer que nunca haverá falsa atuação. O teste com voz distante e TV é uma porta de liberação, não uma hipótese já resolvida.

**Demanda/cache têm escopo.** O cache acompanha criação/remoção de nodes/links/default metadata; não subscreve todo possível parâmetro de cada dispositivo. A demanda continua vinculada às saídas virtuais do produto. Consumo exclusivamente cru, bypass, `Audio/Source/Virtual`/duplex, mudanças de perfil e props sem recriação exigem testes específicos; a política preferida na ambiguidade é não atuar.

**Calibração com salvamento falho.** O erro é corretamente exposto, mas o ganho físico pode ter mudado antes da falha de disco. O pacote não promete transação ACID envolvendo filesystem, outro processo e hardware. Exibir erro e preservar intervenção externa é mais seguro que restaurar cegamente.

**Encerramento nativo e dependências.** Threads são encerradas/joinadas nos caminhos implementados, mas os limites reais de kill/reaping e startup dependem também do vendor não fornecido e da biblioteca nativa. Revisar isso após recuperar a dependência. Não converter deadlines de código em garantias físicas sem testes.

**Sem aprovação de release.** Após compilar as versões travadas, executar todos os cenários de hardware, rever regressões e publicar os logs A/B. Não usar os ganhos do microbenchmark como publicidade de redução de CPU total ou melhora acústica já demonstrada.
