#!/usr/bin/env python3
"""
Generate Windows-compliant multi-resolution ICO and PNG icons from cymesh-logo.jpg.
Uses Pillow to produce standard MS Windows Icon Resource format with:
- 256x256, 128x128, 64x64, 48x48, 32x32, 16x16
"""
from pathlib import Path
from PIL import Image, ImageDraw

ROOT_DIR = Path(__file__).resolve().parent.parent
JPG_PATH = ROOT_DIR / "cymesh-logo.jpg"
OUT_ICO = ROOT_DIR / "src-tauri" / "icons" / "icon.ico"
OUT_PNG_DIR = ROOT_DIR / "src-tauri" / "icons"

def create_squircle_image(size: int) -> Image.Image:
    jpg = Image.open(JPG_PATH).convert("RGBA")
    # Center on the cat geometry (center ~ 995, 1025) and crop tightly
    cx, cy, half = 995, 1025, 750
    cropped = jpg.crop((cx - half, cy - half, cx + half, cy + half))
    resized = cropped.resize((size, size), Image.Resampling.LANCZOS)
    radius = int(size * 0.22)
    mask = Image.new("L", (size, size), 0)
    draw = ImageDraw.Draw(mask)
    draw.rounded_rectangle([(0, 0), (size - 1, size - 1)], radius=radius, fill=255)
    squircle = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    squircle.paste(resized, (0, 0), mask)
    return squircle

def main():
    OUT_PNG_DIR.mkdir(parents=True, exist_ok=True)

    img256 = create_squircle_image(256)
    img256.save(
        OUT_ICO,
        format="ICO",
        sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (16, 16)],
    )

    # Save standard PNGs for Tauri window and cross-platform display
    img256.save(OUT_PNG_DIR / "icon.png")
    img256.save(OUT_PNG_DIR / "128x128@2x.png")
    create_squircle_image(128).save(OUT_PNG_DIR / "128x128.png")
    create_squircle_image(32).save(OUT_PNG_DIR / "32x32.png")

    print(f"Generated compliant Windows ICO at {OUT_ICO} ({OUT_ICO.stat().st_size} bytes)")

if __name__ == "__main__":
    main()
