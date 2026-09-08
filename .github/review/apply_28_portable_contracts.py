from _common import done, replace, commit

TITLE = 'fix(lint): exclude localized prose from the English spelling dictionary'
if not done(TITLE):
    path = '_typos.toml'
    replace(path, 'extend-exclude = [', '''extend-exclude = [
    # Localized prose is not English; gettext catalogs have a separate gate.
    # Keep English docs, Rust code, protocol identifiers and fixtures checked.
    "docs/*.pt_BR.md",''')
    commit(TITLE, [path])
