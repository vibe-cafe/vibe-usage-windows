from __future__ import annotations

import re
from pathlib import Path

from PIL import Image, ImageDraw


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "icon-source(1).svg"
VIEWBOX_SIZE = 600

PNG_OUTPUTS = {
    "src-tauri/icons/icon.png": (512, "black"),
    "src-tauri/icons/32x32.png": (32, "black"),
    "src-tauri/icons/64x64.png": (64, "black"),
    "src-tauri/icons/128x128.png": (128, "black"),
    "src-tauri/icons/128x128@2x.png": (256, "black"),
    "src-tauri/icons/StoreLogo.png": (50, "black"),
    "src-tauri/icons/Square30x30Logo.png": (30, "black"),
    "src-tauri/icons/Square44x44Logo.png": (44, "black"),
    "src-tauri/icons/Square71x71Logo.png": (71, "black"),
    "src-tauri/icons/Square89x89Logo.png": (89, "black"),
    "src-tauri/icons/Square107x107Logo.png": (107, "black"),
    "src-tauri/icons/Square142x142Logo.png": (142, "black"),
    "src-tauri/icons/Square150x150Logo.png": (150, "black"),
    "src-tauri/icons/Square284x284Logo.png": (284, "black"),
    "src-tauri/icons/Square310x310Logo.png": (310, "black"),
    "src-tauri/icons/tray-icon.png": (32, "white"),
    "src/assets/menubar-icon.png": (32, "white"),
}

ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
ICNS_SIZES = [(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512)]


def parse_rectangles(svg: str) -> list[tuple[float, float, float, float]]:
    rectangles: list[tuple[float, float, float, float]] = []
    for match in re.finditer(r"<rect\b([^>]*)/>", svg):
        attrs = dict(re.findall(r'(\w+)="([^"]+)"', match.group(1)))
        if attrs.get("fill") != "black":
            continue
        if "x" not in attrs or "y" not in attrs:
            x = float(attrs.get("x", 0))
            y = float(attrs.get("y", 0))
        else:
            x = float(attrs["x"])
            y = float(attrs["y"])
        rectangles.append((x, y, float(attrs["width"]), float(attrs["height"])))
    return rectangles


def parse_path_polygons(svg: str) -> list[list[tuple[float, float]]]:
    polygons: list[list[tuple[float, float]]] = []
    for path_match in re.finditer(r'<path\b[^>]*\bd="([^"]+)"', svg):
        tokens = re.findall(r"[A-Za-z]|[-+]?(?:\d*\.\d+|\d+)", path_match.group(1))
        index = 0
        current = (0.0, 0.0)
        polygon: list[tuple[float, float]] = []
        command = ""

        while index < len(tokens):
            token = tokens[index]
            if re.match(r"[A-Za-z]", token):
                command = token
                index += 1
            elif not command:
                raise ValueError(f"Path data starts without a command near {token!r}")

            if command == "M":
                x = float(tokens[index])
                y = float(tokens[index + 1])
                current = (x, y)
                polygon = [current]
                index += 2
            elif command == "H":
                x = float(tokens[index])
                current = (x, current[1])
                polygon.append(current)
                index += 1
            elif command == "V":
                y = float(tokens[index])
                current = (current[0], y)
                polygon.append(current)
                index += 1
            elif command in ("Z", "z"):
                if len(polygon) >= 3:
                    polygons.append(polygon)
                polygon = []
                command = ""
            else:
                raise ValueError(f"Unsupported SVG path command: {command}")

    return polygons


def render_logo(
    size: int,
    color: str,
    rectangles: list[tuple[float, float, float, float]],
    polygons: list[list[tuple[float, float]]],
) -> Image.Image:
    factor = 8 if size <= 128 else 4
    high_size = size * factor
    scale = high_size / VIEWBOX_SIZE
    rgba = (255, 255, 255, 255) if color == "white" else (0, 0, 0, 255)
    image = Image.new("RGBA", (high_size, high_size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    for x, y, width, height in rectangles:
        draw.rectangle(
            (
                round(x * scale),
                round(y * scale),
                round((x + width) * scale),
                round((y + height) * scale),
            ),
            fill=rgba,
        )

    for polygon in polygons:
        draw.polygon([(round(x * scale), round(y * scale)) for x, y in polygon], fill=rgba)

    return image.resize((size, size), Image.Resampling.LANCZOS)


def main() -> None:
    svg = SOURCE.read_text(encoding="utf-8")
    rectangles = parse_rectangles(svg)
    polygons = parse_path_polygons(svg)

    rendered: dict[tuple[int, str], Image.Image] = {}
    for output, (size, color) in PNG_OUTPUTS.items():
        image = rendered.setdefault((size, color), render_logo(size, color, rectangles, polygons))
        path = ROOT / output
        path.parent.mkdir(parents=True, exist_ok=True)
        image.save(path)

    base = render_logo(512, "black", rectangles, polygons)
    base.save(ROOT / "src-tauri/icons/icon.ico", sizes=ICO_SIZES)
    base.save(ROOT / "src-tauri/icons/icon.icns", sizes=ICNS_SIZES)


if __name__ == "__main__":
    main()
