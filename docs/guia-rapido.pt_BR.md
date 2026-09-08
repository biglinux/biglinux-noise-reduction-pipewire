# Filtro de ruído: escolha pelo que você precisa ouvir

## Para começar

Use a tela simples. Escolha o microfone, fale no seu volume habitual e observe
o medidor. Ligue o filtro do microfone. Comece com a qualidade automática e
ajuste a intensidade apenas se ainda houver ruído ou se a voz ficar artificial.
Você não precisa escolher um modelo de inteligência artificial para começar.

Faça uma mudança por vez. Compare uma frase curta antes e depois; o resultado
mais útil é uma voz compreensível, não necessariamente silêncio absoluto.

## Qualidade e consumo

**Automática** escolhe entre os modelos disponíveis segundo a política do
aplicativo e as medições que ele consegue obter. Isso não é uma garantia de
adaptação contínua a toda mudança de carga do computador.

**Menor uso de CPU** é a primeira alternativa quando o computador está ocupado
ou o som apresenta cortes. **Melhor qualidade** prioriza o modelo de maior
qualidade da política e pode consumir mais recursos. A lista manual fica nos
controles avançados; modelos sem a biblioteca necessária não são escolhas
funcionais, mesmo que um arquivo do plugin esteja presente.

A intensidade controla quanto o filtro atua. Aumentar demais pode mudar o
timbre ou prejudicar consoantes. Reduza um pouco e compare novamente.

## Eco e retorno da própria voz

O eco acontece quando o microfone capta o que sai dos alto-falantes. A opção
automática de cancelamento considera o dispositivo de saída. Fones de ouvido
normalmente evitam esse caminho acústico.

Use **Ouvir minha voz** somente com fones. Com alto-falantes, o retorno pode
produzir realimentação. Comece com volume baixo. O retorno pertence à janela e
é encerrado ao fechá-la; não é uma gravação permanente.

## Som do sistema

O filtro de saída modifica o áudio que você escuta. Para música, filmes e
jogos, preserve o estéreo. O modo mono é uma escolha explícita para situações
centradas em voz e pode usar menos processamento; ele combina os canais e perde
a separação espacial. Compare antes de deixá-lo ligado em todo o sistema.

Desligar o controle principal suspende o processamento correspondente sem
apagar as escolhas dos efeitos. Restaurar os padrões é uma ação diferente,
confirmada antes de substituir suas preferências.

## Quando abrir os controles avançados

O equalizador muda o timbre; o compressor reduz diferenças de volume; o gate
atenua pausas. Eles não precisam ficar todos ligados. Para começar, mantenha
os padrões e habilite apenas o efeito que resolve um problema que você ouviu.

A aba de ajustes do sistema é para cortes, atrasos ou particularidades de um
dispositivo. Ela distingue alterações pendentes das que já foram aplicadas.
Aplicar esses ajustes pode reiniciar o áudio e interromper uma chamada: faça
isso fora de uma conversa importante.

A prévia do tamanho do buffer é temporária e pode ser interrompida. Ela altera
a sessão de áudio, não apenas este aplicativo. Se o resultado piorar, interrompa
a prévia e retorne ao valor anterior; não é preciso salvar para experimentar.

**Preferir CPUs rápidas** e **reservar memória** são ajustes especializados.
Deixe-os desligados inicialmente. Podem ajudar em algumas máquinas, mas também
restringir o escalonamento ou aumentar a memória indisponível para outros
programas. Não existe uma combinação universalmente mais rápida.

## Quando aparecer um erro

Leia a causa e a próxima ação. Uma preferência salva não significa que o
filtro conseguiu iniciar. Use a ação de tentar novamente depois de corrigir
a causa. A janela deve poder ser fechada mesmo quando o áudio não responde.

Se dois programas alterarem o mesmo campo ao mesmo tempo, um conflito pode
ser informado em vez de apagar silenciosamente uma das escolhas. Confira o
valor atual e refaça a alteração pretendida.

O diagnóstico de terminal está disponível em `biglinux-microphone-cli doctor`.
Ao relatar um problema, inclua a versão, o dispositivo e a mensagem apresentada;
não publique gravações privadas, credenciais ou um log inteiro sem revisá-lo.
