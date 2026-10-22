"""Check the locale documents of Git-Terminal.

A language is a document, so the language cannot be checked by reading the code.
What can be checked is the document: that it is well formed, that it still refers
to messages that exist, that its placeholders match the message it translates, and
that its whitespace matches the message it translates. Those are all defects that
would otherwise show up as wrong output in one language only.

The checker is deliberately not a translation review. The wording of a document is
accepted once, when the document is added; the wording of one language is not
re-verified on every change, because a wrong or awkward string is a defect of that
document alone and is fixed by editing the JSON.

Usage:
    python tools/check_locales.py             # check, report coverage, fail on error
    python tools/check_locales.py --coverage  # also list what each language misses
    python tools/check_locales.py --fix       # sort the keys, then check
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCALES = ROOT / 'locales'

# A placeholder is the only part of a message that may not be reworded freely:
# the substitution is applied by name, so a renamed placeholder silently prints
# the literal text in the middle of a sentence.
PLACEHOLDER = re.compile(r'\{([a-z_][a-z0-9_]*)\}')


def rust_literal(text: str) -> str:
    """Render a message the way it appears inside a Rust string literal."""
    return (
        text.replace('\\', '\\\\')
        .replace('"', '\\"')
        .replace('\n', '\\n')
        .replace('\t', '\\t')
        .replace('\r', '\\r')
    )


def source_corpus() -> str:
    """Every Rust source file, concatenated, for literal lookups."""
    parts = []
    for path in sorted((ROOT / 'src').rglob('*.rs')):
        parts.append(path.read_text(encoding='utf-8', errors='replace'))
    return '\n'.join(parts)


def present(key: str, corpus: str) -> bool:
    """Whether a message is still produced by the interface."""
    literal = rust_literal(key)
    if f'"{literal}"' in corpus or f'b"{literal}"' in corpus:
        return True
    return key in corpus


def placeholders(message: str) -> set[str]:
    """The substitution names of a message.

    A single-letter placeholder is a plural marker, which a language may drop or
    repeat freely. Only named substitutions must survive, and they may move,
    because word order differs between languages.
    """
    return {name for name in PLACEHOLDER.findall(message) if len(name) > 1}


def load(path: Path, problems: list[str]) -> dict | None:
    """Read one locale document, reporting every structural defect it has."""
    raw = path.read_bytes()
    if raw.startswith(b'\xef\xbb\xbf'):
        problems.append(f'{path.name}: starts with a byte order mark')
    if b'\r\n' in raw:
        problems.append(f'{path.name}: uses CRLF line endings')
    if not raw.endswith(b'\n'):
        problems.append(f'{path.name}: does not end with a newline')
    try:
        text = raw.decode('utf-8')
    except UnicodeDecodeError as error:
        problems.append(f'{path.name}: is not valid UTF-8 ({error})')
        return None

    duplicates: list[str] = []

    def pairs(items):
        seen = set()
        for key, value in items:
            if key in seen:
                duplicates.append(key)
            seen.add(key)
        return dict(items)

    try:
        document = json.loads(text, object_pairs_hook=pairs)
    except json.JSONDecodeError as error:
        problems.append(f'{path.name}: is not valid JSON ({error})')
        return None
    for key in duplicates:
        # A duplicated key is silently collapsed by every JSON reader, so the
        # first translation would simply disappear without a trace.
        problems.append(f'{path.name}: defines {key!r} more than once')
    return document


def check_document(path: Path, document: dict, corpus: str) -> list[str]:
    """Check one parsed locale document against the source of the program."""
    problems: list[str] = []
    code = path.stem

    if document.get('code') != code:
        problems.append(
            f'{path.name}: code is {document.get("code")!r}, but the file is named {code!r}'
        )
    for field in ('name', 'english_name'):
        if not document.get(field):
            problems.append(f'{path.name}: {field} is missing or empty')

    messages = document.get('messages', {})
    if not isinstance(messages, dict):
        problems.append(f'{path.name}: messages is not an object')
        return problems

    for key, value in messages.items():
        if not isinstance(value, str):
            problems.append(f'{path.name}: message {key!r} is not a string')
            continue
        if not value.strip():
            problems.append(f'{path.name}: message {key!r} is empty')
        # The message has to exist in the source. A stale key means the interface
        # was reworded and this translation is now dead text.
        if not present(key, corpus):
            problems.append(
                f'{path.name}: message {key!r} does not appear in the source, so the '
                'interface no longer produces it'
            )
        if placeholders(key) != placeholders(value):
            problems.append(
                f'{path.name}: message {key!r} has placeholders '
                f'{sorted(placeholders(key))} but the translation has '
                f'{sorted(placeholders(value))}'
            )
        # Message whitespace is part of the output: a trailing newline decides
        # whether the next line starts on a new line.
        if key.endswith('\n') != value.endswith('\n'):
            problems.append(
                f'{path.name}: message {key!r} and its translation disagree on a trailing newline'
            )

    keys = list(messages)
    if keys != sorted(keys):
        problems.append(
            f'{path.name}: messages is not sorted; run --fix so the file stays reviewable'
        )
    return problems


def coverage(documents: dict[str, dict]) -> str:
    """Report how much of the interface each language translates."""
    reference: set[str] = set()
    for document in documents.values():
        reference.update(document.get('messages', {}))
    if not reference:
        return 'no messages to measure'
    lines = [f'{len(reference)} messages known to the shipped languages']
    for code, document in sorted(documents.items()):
        messages = set(document.get('messages', {}))
        lines.append(
            f'{code:<10} messages {len(messages):>4}/{len(reference):<4} '
            f'({100 * len(messages) // len(reference):>3}%)  '
            f'falling back to English: {len(reference - messages)}'
        )
    return '\n'.join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument('--coverage', action='store_true', help='report per-language coverage')
    parser.add_argument('--fix', action='store_true', help='sort the keys of every document')
    arguments = parser.parse_args()

    if not LOCALES.is_dir():
        print(f'no locale directory at {LOCALES}', file=sys.stderr)
        return 1
    files = sorted(LOCALES.glob('*.json'))
    if not files:
        print(f'no locale documents in {LOCALES}', file=sys.stderr)
        return 1

    if arguments.fix:
        for path in files:
            document = json.loads(path.read_text(encoding='utf-8'))
            if isinstance(document.get('messages'), dict):
                document['messages'] = dict(sorted(document['messages'].items()))
            path.write_text(
                json.dumps(document, ensure_ascii=False, indent=2) + '\n',
                encoding='utf-8',
                newline='\n',
            )
        print(f'sorted the keys of {len(files)} documents')

    corpus = source_corpus()
    problems: list[str] = []
    documents: dict[str, dict] = {}
    for path in files:
        document = load(path, problems)
        if document is None:
            continue
        documents[path.stem] = document
        problems += check_document(path, document, corpus)

    if arguments.coverage:
        print(coverage(documents))
        print()

    if problems:
        for problem in problems:
            print(f'error: {problem}', file=sys.stderr)
        print(f'{len(problems)} problem(s) in {len(files)} documents', file=sys.stderr)
        return 1
    print(f'{len(files)} locale documents checked: structure, stale keys, placeholders, whitespace, order')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
