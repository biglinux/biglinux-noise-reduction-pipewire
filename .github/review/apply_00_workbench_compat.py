"""Keep exact-match patch anchors aligned with the reviewed source layout."""
from pathlib import Path

path = Path(__file__).with_name('apply_11_stereo.py')
text = path.read_text()
text = text.replace("    content.append(&master);", "    content.append(super::simple::output_card(state, input).widget());")
text = text.replace("    path = 'src/ui/views/output.rs'\n    text = Path(path).read_text()", "    path = 'src/ui/views/output.rs'\n    text = Path(path).read_text().replace('use gtk::prelude::*;', 'use adw::prelude::*;')")
path.write_text(text)
