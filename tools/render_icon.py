"""Render the small original SVG subset to reproducible PNG and multi-size ICO assets."""
from pathlib import Path
import xml.etree.ElementTree as ET
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parents[1]
scale = 4
image = Image.new('RGBA', (256*scale, 256*scale))
draw = ImageDraw.Draw(image)
def number(node, key): return float(node.get(key, '0'))*scale
def circle(x, y, r, fill): draw.ellipse((x-r, y-r, x+r, y+r), fill=fill)
for node in ET.parse(root/'assets/git-terminal.svg').getroot():
    tag = node.tag.split('}')[-1]
    fill, stroke = node.get('fill'), node.get('stroke')
    width = round(number(node, 'stroke-width'))
    if tag == 'rect':
        x, y = number(node, 'x'), number(node, 'y')
        draw.rounded_rectangle((x, y, x+number(node, 'width'), y+number(node, 'height')),
                               number(node, 'rx'), fill=fill, outline=stroke, width=width)
    elif tag in ('line', 'polyline'):
        points = [(float(x)*scale, float(y)*scale) for x, y in (p.split(',') for p in node.get('points').split())] if tag == 'polyline' else [(number(node, 'x1'), number(node, 'y1')), (number(node, 'x2'), number(node, 'y2'))]
        draw.line(points, fill=stroke, width=width, joint='curve')
        for x, y in points: circle(x, y, width/2, stroke)
    elif tag == 'circle': circle(number(node, 'cx'), number(node, 'cy'), number(node, 'r'), fill)
    else: raise ValueError('Unsupported icon primitive')
image = image.resize((256, 256), Image.Resampling.LANCZOS)
image.save(root/'assets/git-terminal.png', optimize=True)
image.save(root/'assets/git-terminal.ico', sizes=[(size,size) for size in (16,20,24,32,40,48,64,128,256)])
print('Rendered original icon: 256px PNG and nine Windows ICO sizes')
