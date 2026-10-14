# Homologação em hardware real — critérios propostos, não resultados

Nenhum cenário abaixo foi executado no equipamento do usuário nesta revisão. Usar uma sessão de teste restaurável, identificar microfone e destino, anotar ganho/mute/default/range iniciais e evitar monitoramento acústico alto. **O coletor não muda nenhum deles e não grava voz.** Alterações deliberadas da matriz são ações separadas, feitas pelo agente no ambiente de implementação autorizado.

## A. Preparar comparação justa

Comparar base e série completa no mesmo computador, microfone, distância, ambiente, perfil, sample rate e quantum. Incluir equipamento antigo e pelo menos um microfone fraco, um USB com degraus de ganho e o interno de notebook. Repetir execuções A/B alternadas para não confundir aquecimento, governor ou aplicações concorrentes com ganho do patch.

Registrar versões dos binários realmente carregados, commit do projeto e do vendor, saída de compilação, configuração dos filtros e AGC, volume/mute, nomes+serial+cookie, grupo driver e taxa negociada. Para testes com voz, usar estímulos comparáveis e registrar o procedimento; não tratar gravação reproduzida por alto-falante como equivalente universal à fala próxima.

Métricas mínimas: CPU por processo/thread, RSS, context switches, quantidade de processos criados por segundo, taxa efetiva de análises e escritas, descartes/idade de frames, tempo até faixa-alvo, overshoot, clipping, mudanças de ganho durante silêncio/TV e erros/xruns do grupo correto. Separar funcionamento normal de startup/reconexão. Não escolher apenas a melhor rodada.

```sh
# Somente diagnóstico; --out deve ser uma pasta ainda inexistente.
python3 /caminho/pacote/tests/collect_hardware.py \
  --out /caminho/resultados/agc-base-01 --seconds 60
# Repetir no candidato, com as mesmas condições.
# Para incluir a interface: acrescentar --pid PID_DA_INTERFACE.
```

O coletor faz snapshots antes/depois e amostra `/proc` durante o intervalo. Não grava áudio, não configura volume, não inicia/reinicia serviços e não instala dependências. Logs podem conter nomes de usuário, aparelhos e aplicações: revisar antes de compartilhar. Diretórios são criados privados. Uma coleta incompleta é evidência de falha, não desempenho zero. Após reiniciar serviços, executar outra coleta, porque os PIDs principais são resolvidos no início. Filhos desses PIDs são acompanhados com limite de quantidade/profundidade.

A coleta de `/proc` não mede automaticamente convergência acústica, ganho aplicado por amostra, taxa de análises ou subprocessos que nascem e morrem entre amostras. Essas métricas exigem instrumentação adicional de controle/tracing no ambiente real, fora de RT e com overhead medido. Não extrair números inexistentes do relatório.

## B. Matriz de funcionamento e segurança

| Cenário | Procedimento | Aprovação proposta |
|---|---|---|
| Desabilitado/idle | AGC sem janela; depois com janela e sem consumidores por mais de 2 s | Nenhuma escrita indevida; captura desativada sem demanda; sem loop de reconexão ocupado |
| Fala fraca | Ganho inicial de 30%, fala próxima comparável, range permitido até 300% | Subida somente com evidência de fala; alcançar nível útil sem travar em limite artificial; 300% disponível quando necessário, não obrigatório para todo microfone |
| Fala forte | Ganho inicial de 300%, falar próximo, incluindo início súbito | Descida estável; medir clipping/overshoot; não tratar o monitor como limitador infalível |
| Silêncio/ruído contínuo | 30 s de silêncio, ventilador/ruído branco; repetir teclado/batidas | Não iniciar a antiga elevação cega; contar toda atuação e justificar; bloquear release se houver subida repetida claramente não atribuível à fala |
| TV/voz distante | Televisor e outras pessoas, com e sem pessoa calibrada perto | Medir falsos positivos/negativos; não declarar identificação de locutor. Ajustar heurística só com dados e testes de regressão |
| Eco/double-talk | Reprodução local e fala simultânea, AEC ativo/inativo | Não elevar ganho só por far-end; verificar evidência suspensa/retomada e qualidade de fala |
| Manual/mute/zero | Mudar volume dentro/fora da janela e mutar em cada fase da calibração e daemon | Nenhum unmute/elevação partindo de zero; fora da janela respeitar hold; dentro rebase sem salto; mudança externa aborta calibração sem rollback destrutivo |
| Degraus de hardware | Dispositivo que arredonda volume e retornos iguais/diferentes | Sem tempestade do mesmo pedido, sem acumulação impossível; readback real guia histórico |
| Calibração + daemon | Iniciar calibração com daemon atuando; tentar segunda calibração | Um único dono; daemon pausa captura/atuação e só retoma depois do save com estado fresco |
| Sem voz | Calibrar em silêncio por toda a passagem | Sem sucesso salvo; erro/resultado “não ouviu”; restaurar apenas volume comprovadamente nosso |
| Stall/pipe parcial | Interromper PCM, enviar bloco parcial, encerrar pw-cat | Erro de ausência de áudio por ~3 s mais I/O/teardown limitado; sem espera infinita ou zumbi |
| Troca/hotplug | Trocar default, desconectar/reconectar, reutilizar ID, reiniciar PipeWire | Observações antigas invalidadas; nenhum ajuste na fonte errada; registrar qualquer corrida como bloqueadora |
| Metadata/perfil | Remover/recriar metadata, nomes duplicados, UCM/ACP/duplex | Não aceitar alvo ambíguo; retomada correta ou falha explicável; sem loop ocupado |
| Filtros/bypass | Alternar filtros e bypass, recriar node com mesmo nome | Identidade/normalização reavaliadas; medir consumidor cru e virtual; ausência de suporte não deve atuar em dispositivo arbitrário |
| Fonte/volume indisponível | Fazer readback falhar/expirar sem encerrar daemon | Tentativas limitadas a 1 Hz; não continuar aprendizado/aplicação com estado desconhecido |
| Disco/estado | Falhar save, deletar perfil em edição concorrente, adicionar campo futuro | Sem JSON positivo falso; remoção não ressuscita; campo não pertencente à edição preservado; conflito explícito |
| UI sob carga | Arrastar slider rapidamente, abrir/fechar, reload simultâneo, backend lento | Uma operação por chave + último pendente; sem callback depois de fechar; espectro não espera consulta do grafo |
| Quantum/latência | Dois grupos drivers distintos, rate/quantum diferentes, prévia READY parcial | Relatório usa grupo do microfone ou assume desconhecido; handshake expira; prévia restaura apenas override ainda seu |
| Pressão de fila | Introduzir atraso no controlador/CLI acima de um hop | Sem crescimento ilimitado; registrar descartes e clipping perdido; não chamar isso de proteção lossless |

Repetir cenários principais em 44,1/48 kHz negociados, mono/estéreo e diferentes quantums suportados. O monitor solicita 48 kHz; eventual resampling precisa entrar na comparação de CPU/latência. Não forçar parâmetros globais fora das capacidades do hardware.

## C. Portas quantitativas e interpretação

A arquitetura propõe 20 análises/s em captura saudável e no máximo 5 escritas/s, não exige fluxo artificial de escrita constante. Confirmar contadores instrumentados. Delays de agendamento podem reduzir a taxa efetiva; investigar, não ocultar.

Para CPU/RSS/xruns, aceitar só melhora/retenção consistente nas rodadas controladas. Não há número percentual de CPU total prometido neste pacote. Exigir ausência de crescimento sustentado de memória, ausência de tempestade de processos, ausência de regressão consistente de xruns e de atraso de controle. Definir metas acústicas de alvo/overshoot a partir do produto e das medições; não inventar um limiar universal de dB SPL.

Calibração de 8 s pode usar até 12 s adicionais na escuta; aquisição da posse, startup, comandos, restauração e shutdown acrescentam tempos limitados que devem ser medidos separadamente. Verificar tanto “ouviu fala” quanto estabilidade/convergência: são critérios diferentes.

A latência exibida é estimativa quando não há medição. Para alegar latência ponta a ponta, usar método de loopback/medição acústica apropriado e registrar caminho, filtros, buffering e incerteza. O quantum do driver ou o readback de volume, sozinhos, não dão essa medida [D1/D7].

## D. Registro final por execução

Entregar identificação da base/candidato, PASS/FAIL/NOT_RUN por linha, logs brutos, resumo estatístico sem selecionar melhores amostras, limitações e patches adicionais necessários. Distinguir defeito já presente na base de regressão nova. Falhas de posse, dispositivo errado, mute, persistência ou ausência de prazo bloqueiam liberação, ainda que o benchmark de CPU seja melhor.
