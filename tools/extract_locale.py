"""Rewrite the two-language label calls of the interface into locale lookups.

The interface used to carry both languages inline: `label(zh, "连接", "Connect")`.
That form cannot express a third language, and the Chinese text was spread over
the whole code base. The call sites are rewritten to `tr(&language, "Connect")`,
where the English text is the key of the message, and the Chinese text moves into
the locale document.

The rewrite is mechanical and reversible: the script only touches `label(zh, ...)`
calls, and it reports every message it moved so the locale document can be
regenerated from the result.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'src' / 'remote_ui.rs'
LOCALE = ROOT / 'locales' / 'zh-Hans.json'

# `label(zh, "中文", "English")` and its multi-line form.
CALL = re.compile(
    r'label\(\s*zh,\s*"(?P<chinese>(?:[^"\\]|\\.)*)",\s*"(?P<english>(?:[^"\\]|\\.)*)",?\s*\)',
    re.DOTALL,
)


def main() -> int:
    source = SOURCE.read_text(encoding='utf-8')
    moved: dict[str, str] = {}

    def replace(match: re.Match[str]) -> str:
        chinese = match.group('chinese')
        english = match.group('english')
        moved[english] = chinese
        return f'tr(&language, "{english}")'

    rewritten = CALL.sub(replace, source)
    if not moved:
        print('no label calls were rewritten', file=sys.stderr)
        return 1

    # The helper becomes a thin wrapper over the shared translation function.
    rewritten = rewritten.replace(
        'fn label(zh: bool, chinese: &\'static str, english: &\'static str) -> &\'static str {\n'
        '    if zh { chinese } else { english }\n'
        '}',
        '/// Translate an interface message with the active language.\n'
        'fn tr(language: &crate::i18n::Language, english: &\'static str) -> &\'static str {\n'
        '    crate::i18n::translate(language, english)\n'
        '}',
    )
    SOURCE.write_text(rewritten, encoding='utf-8')

    document = json.loads(LOCALE.read_text(encoding='utf-8'))
    messages = document.setdefault('messages', {})
    for english, chinese in moved.items():
        messages.setdefault(english, chinese)
    document['messages'] = dict(sorted(messages.items()))
    LOCALE.write_text(
        json.dumps(document, ensure_ascii=False, indent=2) + '\n', encoding='utf-8'
    )
    print(f'rewrote {len(moved)} label calls; the locale now holds {len(document["messages"])} messages')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
