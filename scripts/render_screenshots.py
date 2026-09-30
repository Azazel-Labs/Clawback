"""Convert the opt-in demo renderer captures into portable README images (stdlib only)."""
from pathlib import Path
import binascii
import html
import re
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'docs/images'


def chunk(kind, payload):
    return struct.pack('!I', len(payload)) + kind + payload + struct.pack('!I', binascii.crc32(kind + payload) & 0xffffffff)


for name in ('desktop', 'explore'):
    magic, dimensions, maximum, pixels = (ROOT / f'target/demo-{name}.ppm').read_bytes().split(b'\n', 3)
    width, height = map(int, dimensions.split())
    assert magic == b'P6' and maximum == b'255' and len(pixels) == width * height * 3
    rows = b''.join(b'\0' + pixels[y * width * 3:(y + 1) * width * 3] for y in range(height))
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', width, height, 8, 2, 0, 0, 0))
    (OUT / f'{name}.png').write_bytes(png + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b''))

COLORS = {'Reset': '#d8d8d8', 'Black': '#000000', 'White': '#ffffff', 'Gray': '#aaaaaa', 'DarkGray': '#555555',
          'Red': '#aa0000', 'Green': '#00aa00', 'Yellow': '#aaaa00', 'Blue': '#0000aa', 'Magenta': '#aa00aa',
          'Cyan': '#00aaaa', 'LightRed': '#ff5555', 'LightGreen': '#55ff55', 'LightYellow': '#ffff55',
          'LightBlue': '#5555ff', 'LightMagenta': '#ff55ff', 'LightCyan': '#55ffff'}


def color(value):
    if value.startswith('Rgb('):
        return '#%02x%02x%02x' % tuple(map(int, re.findall(r'\d+', value)))
    return COLORS[value]


lines = (ROOT / 'target/demo-terminal.tsv').read_text(encoding='utf-8').splitlines()
width, height = map(int, lines[0].split())
assert len(lines) - 1 == width * height
svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width * 10}" height="{height * 20}" viewBox="0 0 {width * 10} {height * 20}">',
       '<title>Clawback terminal interface — fictional demo drive</title>',
       '<rect width="100%" height="100%" fill="#0f141e"/>']
text = []
for index, line in enumerate(lines[1:]):
    symbol, foreground, background = line.split('\t')
    x, y = (index % width) * 10, (index // width) * 20
    if background != 'Reset':
        svg.append(f'<rect x="{x}" y="{y}" width="10" height="20" fill="{color(background)}"/>')
    if symbol.strip():
        text.append(f'<text x="{x}" y="{y + 15}" fill="{color(foreground)}">{html.escape(symbol)}</text>')
svg.append('<g font-family="Consolas, DejaVu Sans Mono, monospace" font-size="16" xml:space="preserve">')
svg.extend(text)
svg.extend(['</g>', '</svg>'])
(OUT / 'terminal.svg').write_text('\n'.join(svg), encoding='utf-8')
print('Wrote desktop.png, explore.png, terminal.svg from fictional renderer output.')
