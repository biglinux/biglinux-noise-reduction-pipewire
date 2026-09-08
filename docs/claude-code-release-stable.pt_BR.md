# Claude Code no BigLinux — preparar, corrigir e homologar um release stable

> **Instrução ao agente:** leia este documento inteiro e execute o trabalho. Inspecione o repositório e o sistema disponíveis, reproduza as falhas, implemente as correções necessárias, teste o pacote real e produza evidências. Não entregue apenas outro plano ou uma revisão estática. Não declare uma etapa concluída sem executá-la ou sem identificar precisamente a evidência já existente que a cobre.

Documento preparado em **8 de setembro de 2026** para o projeto `biglinux/biglinux-noise-reduction-pipewire`, cuja aplicação e executáveis usam o nome **BigLinux Microphone**.

## 1. Resultado esperado e limites da autorização

Deixar a revisão tecnicamente pronta e homologada para a distribuição **nativa no BigLinux**, preservando configurações, qualidade do áudio, funcionamento da interface e integração com o desktop.

Trabalhe na branch **`fix/review-reliability-accessibility-20260908`**, vinculada ao **PR #30**. Preserve alterações concorrentes. **Não abra outro PR, não faça merge, não crie tags e não publique release ou pacote no canal stable.** A publicação é uma decisão posterior do mantenedor; este trabalho deve entregar código, pacote candidato, evidências e parecer de prontidão.

O escopo principal é o pacote nativo x86_64 e as funcionalidades que ele efetivamente oferece. NixOS exige uma validação própria somente quando integrar o escopo anunciado desta liberação. Flatpak permanece experimental: não o promova silenciosamente a uma rota stable nem condicione a entrega nativa a uma reimplementação completa do Flatpak.

Uma aprovação significa que os critérios deste documento foram satisfeitos **no escopo e no ambiente explicitamente registrados**. Não significa ausência universal de defeitos em qualquer equipamento.

## 2. Ponto de partida confirmado: não reiniciar o projeto do zero

A consulta ao PR feita para preparar este documento encontrou o seguinte estado [R1–R3]:

| Referência | Valor |
| --- | --- |
| PR | `biglinux/biglinux-noise-reduction-pipewire#30` |
| Estado naquele momento | Aberto, em rascunho, sem merge |
| Último commit de aplicação com CI aprovado | `e4703e0164b4839c5537e60ad1c21187a9a4fdad` |
| Base do PR nessa execução | `f674c2006b97f91cfded5a5463f68f3714b10c87` |
| Checkout de integração usado pelos jobs do PR | `7d7ea95f7806972b1caea9c0b4ab68567e7cc07d` |
| Execução de CI aprovada | `34269733813` |
| Biblioteca na suíte comum | 278 testes aprovados; 15 ignorados naquela etapa |
| GTK | 14 contratos executados explicitamente e aprovados |
| Subprocessos | Oito regressões aprovadas |
| PipeWire privado | Contrato de carregamento e recuperação independente dos grafos aprovado |
| Release e conteúdo instalado em diretório temporário | 70 arquivos de runtime, 29 catálogos e quatro executáveis verificados |

As quantidades são um registro histórico, não números que devam permanecer fixos após novas correções. Os testes gráficos e o teste PipeWire foram executados separadamente; não foram aprovados por aparecerem como ignorados na suíte comum. A medição informativa de custo dos modelos permaneceu ignorada. Essa execução não fornece um resultado de Miri ou CodeQL a declarar.

**Ainda faltava homologar:** instalação/atualização reais, sessão BigLinux com WirePlumber e systemd, dispositivos físicos, modelos neurais distribuídos, qualidade/latência do áudio, interação no Plasma e acessibilidade com tecnologia assistiva [R1, R3].

Este documento acrescenta orientação, não novos resultados de teste. Ao começar, descubra o HEAD atual. Ele pode já conter este documento e alterações posteriores. **Não faça checkout destrutivo do SHA acima para apagar trabalho mais recente.** Diferencie SHA da aplicação, SHA do merge de teste, SHA do workflow e hash do pacote.

O `REVIEW.md` contém a matriz **R01–R20 de uma revisão posterior do PR**. Esses identificadores não são a numeração da auditoria original de 50 itens. Não afirme resolver os 50 itens apenas por conferir essa matriz [R3].

## 3. Restrições obrigatórias

### 3.1. Jemalloc e duplicação de trabalho

O mantenedor valida seu próprio fork de jemalloc separadamente. Portanto:

- Não crie testes, probes, benchmarks ou uma investigação dedicada à compatibilidade do jemalloc. Não substitua `jemalloc-gtk-fixed`, não reconstrua o alocador upstream e não altere o fork para facilitar o runner.
- Não repita as suítes funcionais com e sem jemalloc. Preserve uma configuração funcional canônica e a configuração de produção do pacote.
- Não crie workflows auxiliares que dupliquem o CI. Agrupe correções relacionadas antes do push e mantenha um responsável por suíte.
- Testar a aplicação instalada, com suas dependências normais, é necessário. Isso não autoriza transformar a homologação em uma matriz de alocadores.

Há uma inconsistência concreta a corrigir na documentação: na revisão de referência, a seção de reprodução de `REVIEW.md` ainda lista tanto `cargo test --all-features` quanto `cargo test --no-default-features`. O documento `docs/testing-native.md` e a política posterior estabelecem execução funcional única. Confira o workflow atual e alinhe a documentação, sem reintroduzir a duplicação. A menção histórica de R19 a testar comportamento do alocador não substitui esta restrição [R3, R4].

Repetir um caso que falhou para verificar sua correção é legítimo. Uma homologação do pacote no BigLinux também tem propósito diferente dos contratos isolados no runner. O que deve ser eliminado são execuções redundantes da mesma suíte, no mesmo propósito, sem nova informação.

### 3.2. Qualidade e compatibilidade

Preserve as melhorias já implementadas: migração não destrutiva, backup sem sobrescrita, separação entre estado solicitado/persistido/aplicado, recuperação por cadeia, estéreo como padrão, pausa que preserva efeitos e I/O de subprocessos com limites.

Não apague asserções, esconda erros com `|| true`, marque regressões como ignoradas, reduza cobertura ou remova uma funcionalidade anunciada para obter aprovação. Não transforme dados inválidos em gravação silenciosa de defaults. Não faça refatorações amplas ou atualizações indiscriminadas de dependências sem um problema demonstrável.

Mantenha a interface nativa, didática, consistente, visualmente cuidada e com baixa carga cognitiva. Opções potencialmente custosas devem permanecer configuráveis e explicadas. Otimização precisa preservar o resultado funcional; menor CPU com menos inferências realizadas não é uma melhoria válida.

## 4. Segurança do ambiente e permissões do Claude Code

Execute o agente e as compilações como usuário comum. Respeite as permissões e o sandbox configurados no Claude Code; não recomende desativá-los globalmente ou usar `--dangerously-skip-permissions` para acelerar a tarefa [D1].

Pode prosseguir com inspeção, edição do projeto e testes locais permitidos. Solicite autorização específica somente para ações que realmente a exijam, como instalar pacotes no sistema principal, interromper seu áudio, gravar sua voz ou controlar sua sessão desktop. Não pergunte dados que possa obter do repositório e da máquina.

**Antes de qualquer teste com efeitos externos, classifique o ambiente:**

| Ambiente | Uso permitido |
| --- | --- |
| Checkout e diretórios temporários | Compilação, análise estática, fixtures, testes que não atinjam serviços reais |
| PipeWire privado dos scripts de teste | Carregamento e ciclo de vida dos grafos, sem dispositivos/serviços do usuário |
| VM BigLinux com snapshot ou usuário de teste com sessão própria | Instalação, atualização, systemd, WirePlumber, Plasma e testes destrutivos controlados |
| Sessão principal do usuário | Inicialmente apenas observação; alterações somente com autorização delimitada e restauração preparada |

**Alterar apenas `HOME` ou variáveis XDG não isola o gerenciador systemd do usuário.** Da mesma forma, `dbus-run-session` não cria automaticamente outro `systemd --user`. Um teste pode continuar alcançando os serviços da sessão principal. Verifique UID, sockets, barramento e os processos efetivamente controlados. Para integração completa, prefira uma VM descartável ou uma sessão real de outro usuário, não uma falsa separação por variáveis de ambiente.

Não execute `killall pipewire`, `pkill` por nomes genéricos, limpeza global de configurações, `git reset --hard` ou `git clean -fdx`. Encerre somente processos/unidades que o teste criou e cujo proprietário foi verificado. Não altere globalmente `LD_PRELOAD`, políticas de segurança, permissões de `/run/user`, serviços ou repositórios de pacotes.

Faça inventário e backup privado dos arquivos e estados que serão alterados. Preserve bytes, permissões, links e também a informação de que um arquivo ou override **não existia**. A restauração deve conferir a propriedade da alteração e preservar mudanças externas mais recentes; não copie cegamente uma árvore antiga por cima da sessão atual.

Não colete um dump indiscriminado de `env`, chaves SSH, tokens, perfis pessoais ou todos os logs do usuário. Dados de `pw-dump`, journals e gravações podem conter nomes de aplicativos, dispositivos e conteúdo privado. Mantenha evidências locais com acesso restrito e publique somente versões revisadas e saneadas.

Se uma permissão ou um dispositivo faltar, registre **BLOQUEADO** no caso correspondente e prossiga com as demais etapas possíveis. Não encerre toda a tarefa na primeira limitação e não converta falta de evidência em aprovação.

## 5. Reconhecimento inicial e rastreabilidade

### 5.1. Identifique o checkout sem sobrescrever trabalho

Na raiz do repositório, comece por:

```bash
git status --short --branch
git branch --show-current
git rev-parse HEAD
git diff --stat
git diff --cached --stat
```

Confira o remote localmente, tomando cuidado para não publicar URLs que contenham credenciais. Consulte o PR atual e o diff contra sua base. Não assuma que `main` representa a versão stable instalada nem que um clone local esteja atualizado.

Se houver alterações não commitadas, identifique sua origem e preserve-as. Para builds limpos, use uma worktree descartável de um SHA conhecido. Não faça stash, rebase ou force-push automaticamente sobre trabalho de outra pessoa. Uma worktree detached para construir não deve virar outro PR.

### 5.2. Abra um diretório persistente de evidências

Exemplo para Bash, depois de confirmar o checkout correto:

```bash
set -euo pipefail
umask 077
REPO=$(git rev-parse --show-toplevel)
cd "$REPO"
STATE_BASE="${XDG_STATE_HOME:-$HOME/.local/state}/biglinux-microphone-release"
mkdir -p -- "$STATE_BASE"
OUT=$(mktemp -d "$STATE_BASE/session-$(date +%Y%m%d-%H%M%S)-XXXXXX")
export OUT

git rev-parse HEAD > "$OUT/source-sha.txt"
git status --porcelain=v1 > "$OUT/git-status.txt"
git diff --binary > "$OUT/working-tree.patch"
git diff --cached --binary > "$OUT/index.patch"
git ls-files --others --exclude-standard > "$OUT/untracked-paths.txt"
printf '%s\n' "$OUT"
```

Esses diffs não guardam o conteúdo dos arquivos não rastreados; preserve separadamente o que for necessário, sem incluir segredos. O diretório de evidências não deve ser apagado por um `trap` de limpeza. Use diretórios temporários diferentes para as fixtures descartáveis.

Registre versão/edição do BigLinux, arquitetura, kernel, CPU, memória, sessão Wayland/X11, Plasma, PipeWire, WirePlumber, GTK, libadwaita, Rust/Cargo e os pacotes relevantes. Registre também versão da aplicação instalada, procedência das dependências, dispositivos usados, taxas e buffers efetivos. Leia `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock` e o PKGBUILD; não instale automaticamente versões diferentes por supor que a documentação mais nova corresponde ao sistema.

Para cada execução, guarde comando, contexto, horário, duração, código de saída, SHA, features e logs. Use `set -o pipefail` ao passar resultados por `tee`, para uma falha não ser mascarada pelo sucesso da gravação do log. Distinga REPROVADO, BLOQUEADO por infraestrutura e APROVADO.

## 6. Leia as fontes efetivas antes de aplicar correções

Priorize os seguintes grupos; acompanhe seus imports/chamadas quando necessário:

| Área | Fontes a inspecionar |
| --- | --- |
| Arquitetura e pendências | `README.md`, `ARCHITECTURE.md`, `REVIEW.md`, `docs/testing-native.md`, `docs/guia-rapido.pt_BR.md`, descrição/comentários do PR |
| Persistência e aplicação | `src/config/`, `src/services/reconcile*`, `src/services/settings_watch*`, `src/services/preview*`, `src/services/pipewire/`, `src/pipeline/` |
| Interface e monitor | `src/ui/`, `src/services/audio_monitor/`, especialmente seletores, medidor, equalizador, configurações avançadas e encerramento |
| Execução e integração | `src/bin/`, unidades em `usr/`, arquivos WirePlumber e applet Plasma dentro de `usr/share/` |
| Distribuição e validação | `packaging/arch/`, receita de entrada da distribuição, `scripts/test-*.sh`, `scripts/verify-package.py`, `.github/workflows/`, `.github/ci/`, testes e configuração de grafia |
| Modelos e medições | `src/config/noise_model*`, `src/config/plugin_cost*`, `src/config/quality*`, `scripts/calibrate/`, diagnóstico e observação de trabalho neural |

Procure a auditoria original de 50 itens nos materiais já disponíveis. Se ela não estiver acessível, registre a ausência e não invente seu conteúdo; continue com a matriz R01–R20, o diff e os critérios deste documento.

Confira a documentação primária correspondente às APIs e versões efetivamente utilizadas antes de alterar Rust, GTK/libadwaita, PipeWire, WirePlumber, QML/KDE ou empacotamento. Uma página de desenvolvimento pode mostrar APIs que a versão mínima do pacote não oferece. Use as referências ao final como ponto de partida, não como permissão para elevar requisitos sem necessidade.

**Entregável inicial:** uma matriz requisito → código → teste existente → lacuna → evidência necessária. Atualize-a durante o trabalho, em vez de recriar relatórios contraditórios.

## 7. Validação automatizada, com uma execução funcional canônica

Leia os scripts antes de executá-los na máquina real. Confira especialmente os caminhos XDG e o acesso a barramentos e serviços. Reutilize os testes e caches existentes; não reconstrua o harness inteiro.

Na revisão de referência, a sequência funcional canônica é [R4]:

```bash
python3 -m unittest discover -s .github/ci -p 'test_*.py'
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --no-default-features --locked -- --test-threads=1
bash scripts/test-ui.sh --no-default-features
bash scripts/test-pipewire.sh --no-default-features
node --test tests/plasmoid_status.test.cjs
bash scripts/refresh-pot.sh --check
```

Execute cada etapa registrando sua saída; uma falha não autoriza declarar que as seguintes foram executadas. Primeiro corrija problemas reproduzíveis com os testes direcionados. Depois rode uma rodada final consolidada, sem repetir todo o conjunto após cada pequena edição.

O Clippy com todas as features é análise da configuração de compilação; não é uma segunda execução funcional. Os scripts GTK e PipeWire exercitam os contratos explicitamente separados da suíte comum. `--no-run` apenas compila; teste ignorado ou seleção que executou zero casos não comprova comportamento [D2].

Verifique que os contratos obrigatórios foram encontrados e executados. Uma diminuição inesperada na contagem exige investigação; novas contagens são aceitáveis quando explicadas por alterações reais.

Use o workflow atual como referência para RustSec, licenças/fontes, dependências não utilizadas, scanner de segredos, ShellCheck, complexidade, duplicação e catálogos. Preserve a checagem dos dois lockfiles distintos; isso não é repetição acidental da mesma entrada. Ferramenta ausente deve ser instalada no ambiente autorizado ou registrada como bloqueio, nunca tratada como sucesso.

Miri e CodeQL têm execução separada da rotina comum. Confira sua política atual e os riscos alterados. Não inicie ambos a cada commit, não interprete FFI não suportada por Miri como prova de erro e não alegue aprovação deles com base no CI comum.

Não rode também `scripts/quality-check.sh --ci` sem antes conferir quais etapas ele repete. Escolha o agregador ou os comandos individuais, com as verificações restantes explicitamente contabilizadas.

## 8. Persistência, migração e concorrência: provar que os dados sobrevivem

Use diretórios e arquivos de teste, nunca as configurações reais para injeção de corrupção ou falha. Estenda as regressões existentes somente onde faltar cobertura relevante.

| Caso | Ação | Critério de aceitação |
| --- | --- | --- |
| CFG-01 | Carregar configuração legada e atualizar | Preservar escolhas compatíveis, extensões suportadas e preferências manuais; não descartar todo o documento por um campo legado conhecido |
| CFG-02 | Criar `pipewire/filter-chain.conf` genérico e arquivos não pertencentes à aplicação | Migração, repair, remove e atualização não sobrescrevem nem removem esses arquivos |
| CFG-03 | Interromper migração e repetir; fornecer backup preexistente ou symlink | Retomada segura; nenhum backup sobrescrito; nenhum alvo de symlink indevido alterado |
| CFG-04 | Editar campos diferentes simultaneamente pela GUI/CLI/arquivo | Mesclar mudanças independentes; não perder silenciosamente a mais recente |
| CFG-05 | Editar o mesmo campo com valores incompatíveis | Conflito identificável, recuperação possível e nenhuma falsa confirmação de aplicação |
| CFG-06 | Usar JSON truncado, inválido, campos desconhecidos ou valores fora do domínio | Aplicar o contrato de validação documentado; não substituir o arquivo corrompido silenciosamente por defaults |
| CFG-07 | Falhar uma gravação, a segunda parte de uma transação ou o reinício após persistir | Separar persistido de aplicado; nova tentativa não conflita com a própria gravação e não absorve edição externa |
| CFG-08 | Fazer nova alteração enquanto um worker antigo conclui | Resultado obsoleto não reverte estado novo nem libera indevidamente outro worker |
| CFG-09 | Manter lock ocupado e liberar depois | Prazo limitado, erro claro e recuperação, sem travamento da interface |
| CFG-10 | Reaplicar configuração inalterada com grafos suspensos | Não gerar ciclos de gravação/reinício nem forçar processamento desnecessário |

Prove a preservação com comparação de conteúdo e metadados antes/depois. Verifique também permissões de arquivos privados, caminhos com espaços, caracteres não ASCII e XDG não padrão onde houver suporte. Não confunda um teste de comparação de strings com uma transação real em disco.

## 9. Integração real: PipeWire, WirePlumber, systemd, GUI e CLI

Execute esta fase na sessão descartável aprovada. Descubra os nomes das unidades, nós e identificadores do pacote a partir dos arquivos instalados e do código; não adivinhe IDs numéricos persistentes de objetos PipeWire.

Registre um snapshot inicial e outro depois de cada cenário importante. Use `pw-dump`, `wpctl status`, informações das unidades e logs restritos ao intervalo do ensaio. Confirme o executável carregado pelos processos, não apenas o que aparece primeiro no `PATH`.

O CLI de referência implementa `help`, `settings`, `mic-conf`, `output-conf`, `status`, `doctor`, `models`, `apply`, `autostart`, `reload`, `repair`, `live-update`, `toggle-mic`, `toggle-output` e `set`, além de comandos auxiliares. Confira o despacho e a ajuda atuais antes de usá-los; certos comentários/descrições podem estar defasados [R7]. **`status` com flags habilitadas não é, sozinho, prova de áudio processado.**

### Matriz mínima de integração

| Caso | Exercício | Resultado exigido |
| --- | --- | --- |
| INT-01 | Primeiro login da conta de teste; iniciar pelo menu e depois fechar a GUI | Autostart e processamento seguem o contrato; não dependem da janela permanecer aberta |
| INT-02 | Usar CLI com a GUI fechada e depois abrir a interface | Estado em disco, interface e áudio convergem corretamente |
| INT-03 | Ativar somente microfone, somente saída e ambos | Cada cadeia funciona e pode ser controlada independentemente |
| INT-04 | Pausar e retomar o processamento principal | Efeitos e modelos escolhidos são preservados; não há ativação inesperada de captura/saída |
| INT-05 | Gate ligado com denoiser desligado nos backends afetados; equalizador sem runtime neural opcional | Efeitos independentes não exigem nem reativam o componente dispensado |
| INT-06 | Mudar parâmetros de controle e depois uma opção que altera topologia | Caminho ao vivo ou reconstrução apropriados; os valores efetivos coincidem com uma inicialização limpa |
| INT-07 | Recriar o loader do microfone mantendo saída e depois fazer o inverso | A cadeia não afetada permanece funcional; não reiniciar todo o áudio como solução padrão |
| INT-08 | Injetar falha de uma cadeia ou do AEC; executar recuperação | Erro real é informado; outras operações independentes ainda são tentadas |
| INT-09 | Parar o PipeWire da sessão de teste, abrir/fechar GUI e restaurar o serviço | Sem bloqueio da thread gráfica, tempestade de processos ou sucesso fictício; recuperação demonstrada |
| INT-10 | Reiniciar WirePlumber na sessão de teste | Política e rotas são reconstruídas sem duplicação, realimentação ou seleção indevida |
| INT-11 | Alterar dispositivo padrão, reconectar USB e alternar perfil Bluetooth quando disponíveis | Reconciliação com IDs novos; sem volume pendente enviado ao dispositivo anterior |
| INT-12 | Testar saída física, fones e integração JamesDSP quando presente | Roteamento e referência de AEC corretos; remoção do dispositivo/filtro não deixa alvo obsoleto |
| INT-13 | Fechar/reabrir GUI, esconder/restaurar janela e observar monitor | Captura recuperável não encerra definitivamente a atualização; valores obsoletos não simulam sinal atual |
| INT-14 | Logout/login da conta descartável | Preferências e integração sobrevivem sem depender de processos deixados pelo teste anterior |

Para efeitos ao vivo, leia os parâmetros observados e confronte o sinal, quando possível. Código de saída zero de uma ferramenta não basta para provar que um controle foi aplicado ao nó pretendido. Considere separadamente nós em execução, ociosos e suspensos [R3, D3].

Não use apenas a presença de um nó como prova de rota válida. Confira links, formato, canais, destino físico e fluxo de amostras. Não produza realimentação acústica em alto volume; comece com sinais sintéticos/rotas virtuais e níveis seguros.

## 10. Prévia de buffer: testar a restauração e a ordem dos eventos

Prioridade alta: uma prévia não deve deixar uma configuração temporária imposta nem desfazer uma alteração recente de outro aplicativo.

Leia a implementação de `src/services/preview*` e o controlador da página de ajustes. Registre o valor anterior da propriedade de quantum, sua presença/ausência e o estado persistido, sem inferir um default numérico da distribuição.

Execute estes cenários no ambiente de teste:

1. Sem override anterior: iniciar, aguardar o término normal e verificar que a aplicação deixa de forçar seu valor.
2. Com override explícito anterior: verificar restauração exata, não substituição por um valor estimado.
3. Iniciar e cancelar imediatamente; repetir por fechamento normal da janela.
4. Tentar Apply/Reset durante a inicialização da prévia e tentar outra prévia durante uma aplicação.
5. Introduzir atraso/falha de leitura ou escrita e conferir tentativas limitadas, retorno final e informação na interface.
6. Alterar o override externamente durante a prévia; conferir que a restauração preserva a alteração mais nova.
7. Encerrar normalmente e por SIGTERM os processos pertencentes à prévia; verificar a política efetiva de limpeza.
8. Interromper o servidor de áudio ou matar abruptamente um processo em uma fixture dedicada; verificar diagnóstico e recuperação possível, sem promessa impossível de restauração garantida após SIGKILL.

Compare antes/depois e registre qual processo era responsável pelo override em cada momento. Um valor ausente e um valor explicitamente definido não são o mesmo estado. A interface não deve anunciar restauração confirmada quando ela não foi verificada [R3].

Não implemente um daemon/watchdog permanente só por precaução. Acrescente um mecanismo novo apenas se um defeito reproduzido, o contrato esperado e o custo de manutenção o justificarem.

## 11. Modelos neurais: executar os runtimes que serão distribuídos

O teste de grafos privados aprovado anteriormente não executa a inferência neural. Esta fase deve fechar essa lacuna, não apenas repetir o carregamento de configurações [R1, R4].

Reutilize `scripts/calibrate/` e as ferramentas existentes após lê-las. Consulte `biglinux-microphone-cli models` para obter a correspondência atual entre ID, plugin, label, taxa e capacidades. Confirme o código de origem desses campos. Em particular, `loadable` é verificação de carregamento e não certificação de qualidade ou de trabalho concluído; qualquer flag de tempo real deve ser confrontada com seu significado no código e com medições [R7].

Para cada modelo anunciado no escopo, registre: pacote/versão, biblioteca realmente carregada, identificação do modelo/controles, taxa de amostragem, tamanho dos blocos, configuração e resultado. Modelos que compartilham uma biblioteca devem continuar identificados individualmente.

### Casos obrigatórios

- Executar fala limpa, fala com ruído e trechos sem fala. Demonstrar processamento concluído, saída válida e ausência de falha nativa.
- Alternar modelo e intensidade; comparar resultado ao vivo e após recarregar a mesma configuração.
- Testar modelos manuais e seleção automática. Não substituir uma escolha manual silenciosamente; validar o comportamento documentado quando a escolha ficar indisponível.
- Simular runtime ausente/incompatível e torná-lo disponível novamente no ambiente descartável. A atualização de capacidades não pode bloquear o GTK nem exigir reinicialização desnecessária da aplicação.
- Exercitar indisponibilidade de plugin opcional sem impedir equalizador/bypass ou outros caminhos que não o utilizam.
- Verificar backpressure, atraso de inferência e contagem de blocos concluídos. Um callback rápido que só enfileira trabalho ou devolve áudio antigo não comprova capacidade em tempo real.

Não renomeie nem remova bibliotecas de `/usr/lib` da sessão principal para injetar falhas. Faça isso somente no sistema descartável ou use fixtures de resolução de plugins. Não execute strings de filtros/argumentos com `eval`; passe argumentos estruturados e validados.

Não troque os runtimes distribuídos por uma implementação Python/ONNX diferente e apresente seu resultado como homologação do pacote. Uma implementação alternativa pode servir de diagnóstico, mas precisa ser identificada como tal.

## 12. Áudio e desempenho: medir o que o usuário recebe

### 12.1. Corpus e método

Use áudio sintético e um pequeno corpus local com licença/procedência registrada: fala limpa, ventilação/ruído contínuo, teclado/ruído intermitente, pausas, fala fraca e material estéreo. Microfone e voz pessoais só entram com autorização. Não envie gravações a serviços externos.

Guarde entradas e saídas ou seus hashes e caminhos locais. Alinhe o atraso antes de comparar sinais. Controle ganho/loudness ao avaliar a redução de ruído: tornar tudo mais baixo não deve ser premiado como maior qualidade. Não use reconhecimento de fala ou uma única métrica como substituto integral da inteligibilidade.

Teste primeiro uma rota virtual determinística e depois a rota física. `pw-loopback` pode auxiliar no ensaio, mas sua latência solicitada não é a medição fim a fim [D4]. Não introduza uma ligação acidental entre saída e microfone físico.

### 12.2. Medidas a registrar

| Aspecto | Como verificar |
| --- | --- |
| Canais | Sinais distintos em L/R; conferir preservação estéreo, ordem, vazamento indevido e downmix mono explícito |
| Integridade | NaN/Inf, descontinuidades, silêncio inesperado, frames repetidos, duração e proporção de blocos efetivamente concluídos |
| Ganho | Peak/RMS ou loudness apropriado antes/depois; conferir headroom, compressor, EQ e mudança de voz |
| Distorção | Comparar fonte e saída com sinais controlados e ouvir fala/música; separar distorção preexistente da introduzida |
| Latência | Medição de atraso por loopback/correlação; separar processamento, transporte, lookahead e hardware |
| Tempo real | Erros/XRUNs e prazos sob carga, associados ao driver/nó e à configuração efetiva |
| Recursos | CPU por processo, memória RSS/PSS quando disponível, threads, descritores e crescimento ao longo do ensaio |
| Interface | Responsividade durante captura, aplicação, falha de serviço e atualização de dispositivos |

O período de processamento é `quantum / taxa`; por exemplo, 1024 amostras a 48000 Hz representam aproximadamente 21,3 ms **por período**, não a latência total. Em `pw-top`, `ERR` agrega XRUNs e erros; `W/Q` e `B/Q` têm significados distintos. Registre nós, contadores e condições antes/depois, em vez de tratar uma captura isolada como medição completa [D5].

A proteção atual é **headroom conservador e teto de picos de amostra**, não um limitador true-peak. Teste boosts combinados, transientes, estéreo/mono e o opt-out avançado. Verifique se a atenuação é excessiva para usos normais e se o clamp introduz distorção audível. Não adicione processamento caro sem medir necessidade e custo [R3, R8].

### 12.3. Carga e repetição com propósito

Como protocolo inicial de homologação, e não como norma universal, proponha: smoke de pelo menos 60 segundos por modelo anunciado; 30 minutos de uso contínuo da configuração padrão; 10 minutos sob carga concorrente representativa; 20 ciclos de alterações/recuperação relevantes. Ajuste e registre o protocolo antes de interpretar resultados. Não execute o produto cartesiano de todos os modelos, buffers e equipamentos sem necessidade.

Compare com bypass e, quando disponível, com o pacote stable anterior no mesmo equipamento e condição. Anote aquecimento, governor, carga de fundo e diferenças de ambiente; não mude afinidade ou reserva de memória globalmente para produzir um resultado artificial.

Critérios mínimos: nenhuma falha/trava, nenhuma corrupção, nenhuma perda silenciosa de trabalho neural, canais corretos, ausência de clipping introduzido em condições nominais declaradas e ausência de regressão reproduzível de XRUNs/qualidade na configuração suportada. Fixe tolerâncias numéricas conforme o caminho testado; não invente um limite universal de CPU, latência ou SNR depois de ver os resultados.

Se o hardware não sustentar um modelo, verifique se o produto seleciona/explica uma configuração utilizável sem contrariar escolhas explícitas. Se não conseguir demonstrar desempenho no hardware mínimo anunciado, registre a fronteira como pendente; uma máquina rápida não certifica toda a base instalada.

## 13. GTK, Plasma, acessibilidade e tradução instaladas

### 13.1. Interface GTK e comportamento real

Abra o binário **do pacote instalado**, não somente os testes de widgets. Teste todas as páginas, menu, seletores, controles de efeito, EQ, ajustes avançados, restauração e encerramento. Confira estados vazios, ausência de microfone/runtime e falhas recuperáveis.

Verifique foco e posição de navegação após atualizações. Arrastar sliders não deve reconstruir desnecessariamente a página, disparar processos ilimitados ou perder mudanças. Ao editar uma banda, o preset deve refletir a edição personalizada; selecionar novamente um preset deve restaurar todas as bandas sem loop de sinais.

Avalie janela mínima suportada, uma resolução desktop comum, texto ampliado em 200%, tema claro/escuro e alto contraste. Use tamanhos compatíveis com o contrato anunciado, e registre os efetivos. Procure cortes, sobreposições, rolagem inacessível, rótulos ilegíveis, mensagens incompreensíveis e erros indicados só por cor.

Inspecione o espectro com sinais conhecidos e o nível textual durante silêncio, sinal constante, desconexão e recuperação. Não deixe um medidor congelado representar entrada ainda ativa.

### 13.2. Acessibilidade funcional, não somente atributos

Inspecione a árvore acessível da aplicação com AT-SPI/GTK Inspector e teste navegação por teclado: Tab, Shift+Tab, setas, Enter, Espaço e Escape conforme cada controle. Confira nomes, papéis, valores, estados, relações e ordem de foco. Controles nativos ajudam, mas não corrigem automaticamente semântica ausente ou comportamento personalizado [D6].

Use Orca na sessão de teste quando disponível. Verifique leitura e operação dos controles principais, diálogos, mensagens de erro e valores em decibéis. A atualização do medidor não deve monopolizar anúncios do leitor de tela.

`GTK_A11Y=none`, uma árvore de teste do GTK ou o sucesso de uma chamada de API não constituem homologação real com Orca. Não desative acessibilidade para eliminar warnings e depois declare essa fase aprovada.

Use automação por AT-SPI ou ferramentas disponíveis, mas não suponha que injeção de teclas para X11 controle uma sessão Wayland. Se não houver controle gráfico confiável, guarde a evidência possível e descreva precisamente o pequeno roteiro humano restante. Não invente cliques, capturas ou resultados visuais.

### 13.3. Applet Plasma real

Instale/carregue o applet pelo mecanismo apropriado ao Plasma disponível. Descubra o ID em seus metadados. O teste Node do redutor de estado não instancia o applet.

Valide adicionar/remover da bandeja, abrir GUI, pausar/retomar mic/saída, sincronizar mudanças vindas de CLI/arquivo e apresentar falhas reais. Teste uma consulta antiga que falha após uma nova consulta bem-sucedida: ela não deve ressuscitar um erro obsoleto. Uma ação que realmente falhou também não deve ser ocultada por uma consulta não relacionada.

Verifique QML/imports, dependências declaradas, teclado, foco e tradução em `pt_BR` no **Plasma instalado**. Quando suportado pelo código, exercite a ausência de `inotify-tools` e seu fallback sem polling excessivo. Registre consumo/processos durante inatividade e interação, sem criar benchmark de alocador.

### 13.4. Gettext e textos

Rode extração Rust/QML e validação dos catálogos com as ferramentas do projeto. Confira strings indiretas, mensagens novas, placeholders, plurais, contexto, msgid vazio e mensagens obsoletas. Não basta `msgfmt` aceitar a sintaxe: confira a completude e o sentido das traduções brasileiras.

Valide o domínio e caminho dos `.mo` instalados, tanto na GUI quanto no applet. Teste outra tradução longa e pseudolocalização em ambiente temporário. Não suponha que `en_XA` ou outra locale especial já exista: confira o mecanismo suportado e prepare uma fixture de catálogo sem substituir traduções pessoais ou globais.

Atualize os textos alterados e o guia brasileiro. Não preencha nomes de tradutores, revisões ou autoria com informação inventada para silenciar avisos. Mantenha tradução não inglesa fora da verificação de grafia inglesa conforme a política localizada existente, sem excluir código do lint.

## 14. Pacote real: build limpo, instalação, atualização e reversão

### 14.1. Confira a receita que será usada pela distribuição

Na referência, `packaging/arch/PKGBUILD` preserva o nome `biglinux-noise-reduction-pipewire`, substitui o pacote legado `biglinux-microphone`, usa versionamento de pacote baseado em data/hora e depende de `jemalloc-gtk-fixed`. Seus requisitos não são idênticos aos de uma imagem Arch genérica. Confira os valores atuais e sua disponibilidade no canal BigLinux alvo; não remova dependências para forçar instalação [R6].

A revisão do Cargo e o `pkgver/pkgrel` da distribuição têm significados diferentes. Não substitua o esquema rolling-release por SemVer arbitrariamente. Verifique com `vercmp` que uma atualização real é reconhecida corretamente.

**Cuidado com a fonte:** a receita diferencia checkout local e obtenção remota por Git. Não copie apenas o PKGBUILD para outra pasta e presuma que ele construirá o PR; o fallback pode buscar a branch padrão. Registre o SHA da fonte efetivamente compilada. Testes de obtenção remota devem fixar deliberadamente a revisão candidata no ambiente de build, sem deixar um pin temporário incorreto na receita de produção.

### 14.2. Construa uma vez o artefato candidato

Use um checkout/worktree limpo do SHA selecionado e dependências disponíveis no ambiente autorizado. Preserve o cache de downloads, mas não aceite binários locais obsoletos como evidência de build limpo.

Escolha o caminho de validação sem reconstruções redundantes:

- `scripts/test-package.sh` já executa preparação de fonte, build, instalação em diretório temporário e verificações; ele **não instala no sistema nem cria prova de atualização real** [R5].
- Para esta homologação, é necessário também um arquivo `.pkg.tar.*` instalável. Prefira construir o candidato com makepkg e reutilizar `scripts/verify-package.py` e as demais verificações sobre seu conteúdo, em vez de exigir outro build idêntico só para repetir a etapa temporária.

Quando a suíte funcional canônica já estiver aprovada para a revisão e as verificações pertinentes estiverem contabilizadas, `makepkg --nocheck` pode ser usado **nessa construção controlada** para não executar a suíte novamente pelo `check()`. Registre que o `check()` foi omitido e identifique a evidência que cobre os testes. Não transforme isso em remoção permanente dos testes do PKGBUILD nem use `--nocheck` para contornar testes ainda reprovados. Faça a validação dos metadados que também estivesse no `check()` [D7].

Não use `--nodeps`, `--skipinteg`, sobrescrita global de arquivos ou instalação automática na sessão principal. `--cleanbuild` remove a árvore de fontes do makepkg: use-o apenas no diretório descartável preparado para isso [D7].

Capture o caminho **real** do pacote produzido. Com `pkgver/pkgrel` calculados por data/hora, uma nova avaliação da receita pode gerar outro nome. Leia a versão do artefato existente; não reconstrua seu nome por suposição. Registre `.PKGINFO`, `.BUILDINFO`, receita, toolchain, flags, SHA da fonte e SHA-256 do arquivo.

### 14.3. Verifique conteúdo e dependências

Extraia o pacote em diretório vazio, como usuário comum, e confira:

- Os quatro executáveis, permissões, dependências resolvidas e ausência de referência à árvore de desenvolvimento.
- Unidades systemd, políticas/scripts WirePlumber, arquivos desktop/AppStream, ícones, ilustrações e applet.
- Catálogos atuais no domínio correto, guia brasileiro e licenças; nenhum catálogo removido sobrevivendo como resíduo de build.
- Ausência de caches, logs privados, caminhos de teste, gravações e dependências não declaradas no payload.

Use verificações ELF e metadados; `ldd` somente em binários confiáveis que você acabou de construir. Compare hashes de binários extraídos, instalados e efetivamente executados. Um checksum identifica o artefato; não substitui a política de assinatura do mantenedor.

### 14.4. Faça a instalação e a atualização de verdade

Na VM/conta e no sistema de teste autorizados, valide instalação limpa e atualização **a partir de um pacote stable anterior identificado**, com configurações representativas. Não chame um teste de importação de JSON de atualização do pacote completo.

Confira ownership com pacman, conflitos/provides/replaces, hooks, recarga de unidades, entrada de menu, autostart e carregamento das políticas. Os hooks não devem percorrer ou modificar homes de outros usuários nem substituir arquivos pertencentes ao pacote WirePlumber.

Teste iniciar GUI e CLI fora do checkout, sem variáveis artificiais que escondam arquivos não instalados. Repita os cenários de integração essenciais com o pacote candidato. Simule desinstalação e reversão no snapshot: preservar preferências deve ser um comportamento deliberado, não deixar rotas quebradas sem explicação.

Se o pacote anterior ou a infraestrutura de instalação não estiverem disponíveis, registre o bloqueio de atualização; ainda conclua build e validação de conteúdo. Não declare stable sem tratar as lacunas obrigatórias do escopo.

## 15. Corrigir defeitos e fechar o CI sem criar outro ciclo interminável

Para cada defeito encontrado, registre reprodução mínima, resultado esperado/observado, gravidade, ambiente e fonte da expectativa. Corrija a causa, acrescente uma regressão na camada apropriada e execute o caso direcionado. Quando o defeito depender de desktop/hardware, preserve também um procedimento reproduzível de aceitação.

Prioridade: perda de configuração, falha de áudio/recuperação, travamento, erro de segurança, regressão de uso essencial e inacessibilidade dos controles principais. Melhorias cosméticas secundárias não devem adiar correções funcionais nem motivar redesenho completo.

Antes do push, execute formatação, confira `git diff --check`, revise o diff e remova somente resíduos criados pelo próprio trabalho. Preserve o fork e a feature de produção do alocador. Agrupe mudanças relacionadas em commits coerentes na mesma branch.

Verifique que `.github` continua selecionando os checks corretos, que o resultado agregado não aceita job obrigatório ignorado/cancelado e que pushes/PRs não geram a duplicação já eliminada. Não altere proteção da branch nem crie workflows temporários para contornar checks.

Faça uma rodada final pertinente ao SHA entregue e confirme o resultado remoto correspondente. Não acrescente commits de código depois e reutilize a aprovação anterior. Se houver apenas um commit posterior de documentação, registre os dois SHAs e prove essa diferença; não atribua ao novo SHA uma execução que ocorreu no anterior.

Se um teste falhar apenas de forma intermitente, capture a corrida/condição e resolva ou documente o bloqueio. Reexecutar até obter verde sem investigar não é homologação.

## 16. Documentação e entregáveis obrigatórios

Atualize a documentação existente em vez de manter instruções incompatíveis. Corrija em especial a sequência duplicada em `REVIEW.md`, descrições de CLI que não reflitam o código, e a distinção entre critérios de integração do PR e critérios para publicação stable.

Entregue:

| Entregável | Conteúdo mínimo |
| --- | --- |
| Código e testes | Correções necessárias, regressões e commits na mesma branch; sem merge |
| Pacote candidato | Caminho existente, versão real, SHA-256, proveniência e dependências do build |
| Relatório de prontidão | Escopo, SHAs, ambiente, método, resultados, riscos e parecer |
| Matriz de aceitação | Um registro por caso: APROVADO, REPROVADO, BLOQUEADO ou FORA DO ESCOPO, com justificativa e evidência |
| Evidências privadas | Logs, snapshots, medições, hashes e capturas reais, com dados sensíveis protegidos |
| Guia e notas de release | Mudanças visíveis, migração, recuperação, compatibilidade e limitações conhecidas |
| Estado para continuação | Próxima ação concreta e somente fatos persistidos e confirmados |

Sugestão de relatório versionado: `docs/release-readiness.pt_BR.md`. Não inclua gravações pessoais nem enormes logs binários no Git. Use o diretório privado de evidências e referências saneadas; preserve os artefatos remotos úteis antes de expirarem.

Modelo compacto de registro por caso:

```text
Caso: INT-07
Estado: APROVADO | REPROVADO | BLOQUEADO | FORA DO ESCOPO
SHA da fonte:
SHA do checkout de integração, quando aplicável:
Pacote / SHA-256:
Ambiente e versões:
Pré-condições:
Comandos/ações realmente executados:
Resultado esperado:
Resultado observado:
Medições e tolerâncias definidas:
Evidências existentes:
Correção e teste de regressão, se necessários:
Restauração confirmada:
Limitação residual / responsável pela próxima ação:
```

Marque FORA DO ESCOPO somente para uma funcionalidade/plataforma realmente excluída do release anunciado, com justificativa explícita. Hardware ausente para um requisito anunciado é BLOQUEADO, não FORA DO ESCOPO. Uma afirmação anterior do agente não é evidência suficiente sem logs, código, artefato ou verificação reproduzível.

## 17. Critérios de saída

Use a lista abaixo como gate. Nenhum item começa aprovado apenas pela existência deste documento.

- [ ] Revisão candidata identificada; alterações concorrentes preservadas; código relevante revisado e regressões novas verificadas.
- [ ] Configurações e backups preservados; conflitos, falhas parciais e migração exercitados.
- [ ] Suítes e verificações pertinentes aprovadas com evidência identificada, sem matriz de alocadores nem jobs duplicados.
- [ ] Build nativo limpo e pacote instalável gerados com identidade e dependências conferidas.
- [ ] Instalação e atualização reais testadas, incluindo hooks, menu e autostart.
- [ ] GUI, CLI, systemd, PipeWire e WirePlumber testados em uma sessão BigLinux real descartável.
- [ ] Cadeias independentes, pausa, alterações ao vivo, topologia e recuperação confirmadas.
- [ ] Modelos/runtimes anunciados exercitados com demonstração de trabalho neural concluído.
- [ ] Restauração da prévia e proteção de alterações externas comprovadas, com limites de crash explicitados.
- [ ] Estéreo/mono, ganho, distorção, latência e XRUNs avaliados nas condições anunciadas.
- [ ] Interface instalada e applet Plasma realmente operados, incluindo falhas e sincronização.
- [ ] Teclado, texto ampliado, contraste, AT-SPI e ensaios necessários com Orca concluídos ou identificados como bloqueio.
- [ ] Tradução brasileira instalada na GUI e no applet validada; pseudolocalização e placeholders conferidos.
- [ ] Nenhum defeito bloqueante conhecido nem requisito obrigatório sem evidência no escopo declarado.
- [ ] Logs/artefatos existem, relatório e guia refletem o código, e a sessão de teste foi restaurada.
- [ ] Nenhuma tag, merge ou publicação stable realizada pelo agente.

Conclua com exatamente um parecer principal:

**PRONTO PARA RELEASE STABLE NO ESCOPO DECLARADO:** todos os gates obrigatórios atendidos, pacote identificado, evidência suficiente, sem bloqueadores conhecidos; resta apenas a decisão/ação de publicação do mantenedor.

**CANDIDATO CORRIGIDO, HOMOLOGAÇÃO PENDENTE:** código e checks podem estar aprovados, mas falta uma validação obrigatória identificada. Liste somente as lacunas concretas e como concluí-las.

**BLOQUEADO POR DEFEITO:** há falha funcional/de segurança/de dados/de acessibilidade essencial ainda não resolvida. Dê reprodução, gravidade, evidência e próxima correção.

Não finalize dizendo apenas que o CI está verde. Não invente tarefas extras para prolongar o trabalho depois de satisfazer os critérios. Execute o que for possível nesta sessão e deixe um checkpoint verificável para qualquer continuidade necessária.

## 18. Referências e como usá-las

As referências R descrevem o estado que originou este handoff. As referências D são documentação primária para confirmar contratos e sintaxe. Consulte as versões compatíveis com o ambiente local; links de documentação podem evoluir. As durações, ciclos e organização dos ensaios acima são uma proposta de protocolo de engenharia deste documento, não requisitos extraídos de uma norma externa.

### Repositório e evidências de origem

- [R1 — PR #30 e registro de homologação restante](https://github.com/biglinux/biglinux-noise-reduction-pipewire/pull/30)
- [R2 — CI aprovado da revisão e4703e0](https://github.com/biglinux/biglinux-noise-reduction-pipewire/actions/runs/34269733813)
- [R3 — REVIEW.md na revisão de referência](https://github.com/biglinux/biglinux-noise-reduction-pipewire/blob/e4703e0164b4839c5537e60ad1c21187a9a4fdad/REVIEW.md)
- [R4 — Política de testes nativos e limites da evidência](https://github.com/biglinux/biglinux-noise-reduction-pipewire/blob/e4703e0164b4839c5537e60ad1c21187a9a4fdad/docs/testing-native.md)
- [R5 — O que o teste de pacote efetivamente executa](https://github.com/biglinux/biglinux-noise-reduction-pipewire/blob/e4703e0164b4839c5537e60ad1c21187a9a4fdad/scripts/test-package.sh)
- [R6 — Receita nativa de referência](https://github.com/biglinux/biglinux-noise-reduction-pipewire/blob/e4703e0164b4839c5537e60ad1c21187a9a4fdad/packaging/arch/PKGBUILD)
- [R7 — Despacho e comandos do CLI](https://github.com/biglinux/biglinux-noise-reduction-pipewire/blob/e4703e0164b4839c5537e60ad1c21187a9a4fdad/src/bin/cli.rs)
- [R8 — Implementação da proteção de ganho](https://github.com/biglinux/biglinux-noise-reduction-pipewire/blob/e4703e0164b4839c5537e60ad1c21187a9a4fdad/src/pipeline/gain_safety.rs)

### Documentação primária

- [D1 — Claude Code: permissões](https://code.claude.com/docs/en/permissions) e [sandbox](https://code.claude.com/docs/en/sandboxing)
- [D2 — Cargo: execução e seleção de testes](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- [D3 — PipeWire: filter-chain](https://docs.pipewire.org/page_module_filter_chain.html) e [WirePlumber: locais e precedência de configuração](https://pipewire.pages.freedesktop.org/wireplumber/daemon/locations.html)
- [D4 — PipeWire: pw-loopback](https://docs.pipewire.org/page_man_pw-loopback_1.html)
- [D5 — PipeWire: pw-top e significado dos contadores](https://docs.pipewire.org/page_man_pw-top_1.html)
- [D6 — GTK: acessibilidade, propriedades e comportamento](https://docs.gtk.org/gtk4/section-accessibility.html)
- [D7 — makepkg: opções, construção e metadados](https://man.archlinux.org/man/makepkg.8.en)

Para systemd, consulte também os manuais instalados `systemctl(1)`, `systemd.exec(5)` e `pam_systemd(8)`, especialmente o alcance do gerenciador de usuário e do ambiente de execução.
