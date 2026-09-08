import json
import subprocess
from _common import done, commit

TITLE = 'feat(i18n): complete Brazilian Portuguese for guided settings and recovery'
if not done(TITLE):
    translations = {
        'Above 100% may distort your voice. Lower the volume if the level meter reaches the top.': 'Acima de 100%, sua voz pode ficar distorcida. Reduza o volume se o medidor atingir o máximo.',
        'Another application may be changing the settings, or the settings file is not writable. Try again, keep this window open, or close without saving your latest changes.': 'Outro aplicativo pode estar alterando as configurações, ou não foi possível gravar o arquivo. Tente novamente, mantenha a janela aberta ou feche sem salvar as últimas alterações.',
        'Applies your selected effects to the sound you hear.': 'Aplica os efeitos escolhidos ao som que você escuta.',
        'Applying audio settings…': 'Aplicando as configurações de áudio…',
        'Audio needs attention. Open the controls for details.': 'O áudio precisa de atenção. Abra os controles para ver os detalhes.',
        'Changes are not applied yet. Apply them when you are ready.': 'As alterações ainda não foram aplicadas. Aplique quando terminar de escolher.',
        'Changes have not been saved. You can keep editing or close without saving.': 'As alterações não foram salvas. Você pode continuar editando ou fechar sem salvar.',
        'Cleans your voice for calls and recordings': 'Reduz o ruído da sua voz em chamadas e gravações',
        'Close without saving': 'Fechar sem salvar',
        'Combine to mono (lower CPU use)': 'Combinar em mono (menor uso de CPU)',
        'Could not read the audio status. Open settings to check the connection.': 'Não foi possível verificar o estado do áudio. Abra as configurações para verificar a conexão.',
        'Could not update the microphone. Check the audio connection and try again.': 'Não foi possível atualizar o microfone. Verifique a conexão de áudio e tente novamente.',
        'Input level in decibels. Reduce microphone volume if it reaches zero.': 'Nível de entrada em decibéis. Reduza o volume do microfone se atingir zero.',
        'Keep loaded audio data in memory': 'Manter os dados de áudio carregados na memória',
        'Keep stereo (recommended)': 'Manter estéreo (recomendado)',
        'Keep window open': 'Manter a janela aberta',
        'LEVEL / PEAK': 'NÍVEL / PICO',
        'Leave these off unless audio still stutters. Changing them briefly restarts the filters.': 'Mantenha estas opções desligadas, a menos que o áudio ainda apresente cortes. Alterá-las reinicia os filtros por um instante.',
        'Level: {level} dB · Peak: {peak} dB': 'Nível: {level} dB · Pico: {peak} dB',
        'May help on some computers, but can use more power. The automatic system choice is usually best.': 'Pode ajudar em alguns computadores, mas consumir mais energia. Em geral, é melhor deixar o sistema escolher.',
        'May reduce pauses under memory pressure, but leaves less RAM for other applications. This is optional and limited by the system.': 'Pode reduzir pausas quando a memória está muito ocupada, mas deixa menos memória disponível para outros aplicativos. É opcional e respeita o limite do sistema.',
        'Microphone and system sound filters': 'Filtros do microfone e do som do sistema',
        'Microphone filter': 'Filtro do microfone',
        'Open audio filter controls': 'Abrir os controles dos filtros de áudio',
        'Open settings…': 'Abrir configurações…',
        'Performance options': 'Opções de desempenho',
        'Prefer faster processor cores': 'Preferir os núcleos mais rápidos do processador',
        'Reduces background noise in your voice. Turning this off pauses microphone effects without forgetting your choices.': 'Reduz o ruído de fundo da sua voz. Desligar pausa os efeitos do microfone sem apagar suas escolhas.',
        'Saving settings…': 'Salvando as configurações…',
        'Settings could not be saved': 'Não foi possível salvar as configurações',
        'Some audio filters are not ready. Open settings to check them.': 'Alguns filtros de áudio não estão prontos. Abra as configurações para verificá-los.',
        'Speak normally to check your microphone level.': 'Fale normalmente para verificar o nível do microfone.',
        'Stereo keeps left and right separate for music and video. Mono uses fewer resources but combines both sides; use it mainly for speech.': 'O estéreo mantém os lados esquerdo e direito separados em músicas e vídeos. O mono usa menos recursos, mas combina os dois lados; use principalmente para voz.',
        'Stereo or lower CPU use': 'Estéreo ou menor uso de CPU',
        'Stop preview': 'Parar a prévia',
        'System sound filter': 'Filtro do som do sistema',
        'Temporarily changes the audio buffer for all applications. Stop the preview to return to the previous value.': 'Altera temporariamente a reserva de áudio de todos os aplicativos. Pare a prévia para voltar ao valor anterior.',
        'The audio preview could not be started or restored. Check the audio connection and try again.': 'Não foi possível iniciar a prévia ou restaurar o áudio. Verifique a conexão de áudio e tente novamente.',
        'The change could not be applied. Your previous choices have been kept where possible. Open settings for details.': 'Não foi possível aplicar a alteração. Suas escolhas anteriores foram mantidas quando possível. Abra as configurações para ver os detalhes.',
        'Try for 15 seconds': 'Testar por 15 segundos',
        'Try preview again': 'Tentar a prévia novamente',
        'Turning this off pauses the effects without erasing your preferences.': 'Desligar pausa os efeitos sem apagar suas preferências.',
        'Your audio settings are active.': 'Suas configurações de áudio estão ativas.',
    }
    subprocess.run(['sudo','apt-get','update','-qq'], check=True)
    subprocess.run(['sudo','apt-get','install','-y','-qq','gettext','python3-polib'], check=True)
    subprocess.run(['/usr/bin/python3', '-c', '''import datetime, json, re, sys, polib
translations = json.load(sys.stdin)
catalog = polib.pofile('po/pt_BR.po')
for message, translation in translations.items():
    entry = catalog.find(message)
    assert entry is not None and not entry.obsolete, message
    assert set(re.findall(r'\{[a-z_]+\}', message)) == set(re.findall(r'\{[a-z_]+\}', translation)), message
    entry.msgstr = translation
    entry.flags = [flag for flag in entry.flags if flag != 'fuzzy']
catalog.metadata['PO-Revision-Date'] = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%d %H:%M%z')
catalog.metadata['Last-Translator'] = 'OpenAI-assisted review (pull request 30)'
catalog.save()
missing = [entry.msgid for entry in catalog if not entry.obsolete and not entry.translated()]
print('PT_BR_REMAINING=' + json.dumps(missing, ensure_ascii=False))
assert not missing, 'Untranslated current Brazilian Portuguese messages remain'
print('PT_BR_COMPLETE=' + str(len([entry for entry in catalog if not entry.obsolete])))
'''], input=json.dumps(translations, ensure_ascii=False), text=True, check=True)
    subprocess.run(['msgfmt','--check','--check-header','-o','/dev/null','po/pt_BR.po'], check=True)
    commit(TITLE, ['po/pt_BR.po'])
