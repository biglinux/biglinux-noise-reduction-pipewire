from pathlib import Path
import subprocess
from _common import done, commit

TITLE = 'chore(i18n): synchronize catalogs with all current interface messages'
if not done(TITLE):
    subprocess.run(['sudo', 'apt-get', 'update', '-qq'], check=True)
    subprocess.run(['sudo', 'apt-get', 'install', '-y', '-qq', 'gettext', 'python3-polib'], check=True)
    subprocess.run(['cargo', '+stable', 'install', 'xtr', '--version', '0.1.11', '--locked'], check=True)
    subprocess.run(['bash', 'scripts/refresh-pot.sh'], check=True)
    subprocess.run(['bash', 'scripts/refresh-pot.sh', '--check'], check=True)
    for catalog in sorted(Path('po').glob('*.po')):
        subprocess.run(['msgfmt', '--check', '--check-header', '-o', '/dev/null', str(catalog)], check=True)
    subprocess.run(['/usr/bin/python3', '-c', '''import json, polib
catalog = polib.pofile('po/pt_BR.po')
missing = [entry.msgid for entry in catalog if not entry.obsolete and (not entry.translated())]
print('PT_BR_MESSAGES_TO_TRANSLATE=' + json.dumps(missing, ensure_ascii=False))
print('CATALOG_ACTIVE=' + str(len([e for e in catalog if not e.obsolete])))
'''], check=True)
    commit(TITLE, ['po'])
