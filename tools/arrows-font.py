# SPDX-License-Identifier: AGPL-3.0-or-later
"""Makes the Ficelle Arrows font: every Unicode arrow the Noto fonts draw,
cut out of Noto Sans Math and Noto Sans Symbols 2 (OFL-1.1). The app's own
fonts have no arrows, so this one stands right behind the chosen font and
each arrow looks the same on every device.

Writes apps/client/src/styles/arrows/*.woff2, styles/arrows.css (the faces,
with their unicode-range) and editor/arrows.json (the arrows the picker
shows). Run again only to change the fonts or the arrows:

    pip install fonttools brotli
    python tools/arrows-font.py
    npx prettier --write apps/client/src/styles/arrows.css apps/client/src/editor/arrows.json
"""

import json
import subprocess
import tarfile
import tempfile
import unicodedata
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parent.parent
CLIENT = ROOT / 'apps' / 'client' / 'src'
VERSION = '5.3.0'
# the blocks that hold arrows; only the characters named so are kept
BLOCKS = [
    (0x2190, 0x21FF),
    (0x2794, 0x27BF),
    (0x27F0, 0x27FF),
    (0x2900, 0x297F),
    (0x2B00, 0x2BFF),
    (0x1F800, 0x1F8FF),
]


def arrows() -> list[int]:
    found = []
    for first, last in BLOCKS:
        for code in range(first, last + 1):
            name = unicodedata.name(chr(code), '')
            if 'ARROW' in name or 'HARPOON' in name:
                found.append(code)
    return found


def fetch(package: str, into: Path) -> Path:
    """The package's files folder, from npm."""
    subprocess.run(
        ['npm', 'pack', f'{package}@{VERSION}', '--silent'],
        cwd=into,
        check=True,
        shell=True,
        stdout=subprocess.DEVNULL,
    )
    # npm names it after the scope too: fontsource-noto-sans-math-5.3.0.tgz
    tgz = next(into.glob('*' + package.split('/')[1] + '-*.tgz'))
    folder = into / tgz.stem
    with tarfile.open(tgz) as archive:
        archive.extractall(folder, filter='data')
    return folder / 'package' / 'files'


def ranges(codes: list[int]) -> str:
    """`U+2190-21FF, U+2794` style, for unicode-range."""
    out = []
    start = prev = codes[0]
    for code in codes[1:] + [None]:
        if code is not None and code == prev + 1:
            prev = code
            continue
        out.append(f'U+{start:X}' if start == prev else f'U+{start:X}-{prev:X}')
        if code is not None:
            start = prev = code
    return ', '.join(out)


def drawn(font: Path) -> set[int]:
    """The characters `font` has, closed again (Windows keeps open files)."""
    with TTFont(str(font)) as opened:
        return set(opened.getBestCmap())


def cut(source: Path, codes: list[int], target: Path) -> None:
    options = subset.Options()
    options.flavor = 'woff2'
    options.layout_features = []
    options.name_IDs = ['*']
    options.notdef_outline = True
    font = subset.load_font(str(source), options)
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=codes)
    subsetter.subset(font)
    subset.save_font(font, str(target), options)
    font.close()


def main() -> None:
    wanted = arrows()
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        math = fetch('@fontsource/noto-sans-math', work) / 'noto-sans-math-latin-400-normal.woff2'
        symbols = (
            fetch('@fontsource/noto-sans-symbols-2', work)
            / 'noto-sans-symbols-2-symbols-400-normal.woff2'
        )
        in_math = drawn(math)
        in_symbols = drawn(symbols)
        # Noto Sans Math first, it has the most; Symbols 2 for the rest
        parts = [
            ('math', math, sorted(c for c in wanted if c in in_math)),
            ('symbols', symbols, sorted(c for c in wanted if c not in in_math and c in in_symbols)),
        ]
        out = CLIENT / 'styles' / 'arrows'
        out.mkdir(exist_ok=True)
        faces = []
        for name, source, codes in parts:
            cut(source, codes, out / f'arrows-{name}.woff2')
            faces.append(
                "@font-face {\n"
                "  font-family: 'Ficelle Arrows';\n"
                "  font-style: normal;\n"
                "  font-display: swap;\n"
                "  font-weight: 100 900;\n"
                f"  src: url('./arrows/arrows-{name}.woff2') format('woff2');\n"
                f"  unicode-range: {ranges(codes)};\n"
                "}\n"
            )
    shown = sorted(c for _, _, codes in parts for c in codes)
    header = (
        '/* Made by tools/arrows-font.py: every arrow, from Noto Sans Math and\n'
        '   Noto Sans Symbols 2 (OFL-1.1), behind the chosen font. */\n'
    )
    (CLIENT / 'styles' / 'arrows.css').write_text(header + '\n'.join(faces), encoding='utf-8')
    (CLIENT / 'editor' / 'arrows.json').write_text(
        json.dumps(''.join(chr(c) for c in shown), ensure_ascii=False) + '\n', encoding='utf-8'
    )
    print(f'{len(shown)} arrows, {len(wanted) - len(shown)} not in the fonts')


if __name__ == '__main__':
    main()
