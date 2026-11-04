#!/usr/bin/env python3
"""Make the application icon from the compass illustration (assets/visual/fog-compass-v1.png):
packaging/aurelia.png (512 px, Linux/macOS) and packaging/aurelia.ico (Windows)."""
from PIL import Image, ImageDraw
import os
here = os.path.dirname(os.path.abspath(__file__))
compass = Image.open(os.path.join(here, "../assets/visual/fog-compass-v1.png")).convert("RGBA")
size = 512
bg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
ImageDraw.Draw(bg).rounded_rectangle((0, 0, size - 1, size - 1), radius=96, fill=(232, 216, 176, 255))
box = compass.getbbox()
compass = compass.crop(box)
scale = (size * 0.78) / max(compass.size)
compass = compass.resize((round(compass.width * scale), round(compass.height * scale)), Image.LANCZOS)
bg.alpha_composite(compass, ((size - compass.width) // 2, (size - compass.height) // 2))
bg.save(os.path.join(here, "aurelia.png"), optimize=True)
bg.save(os.path.join(here, "aurelia.ico"), sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
