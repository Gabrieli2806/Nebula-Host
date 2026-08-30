# Tauri App Icons

Place your app icons here with the following names and sizes:

## Required Icons

| Filename | Size | Platform |
|----------|------|----------|
| `32x32.png` | 32x32 px | Windows |
| `128x128.png` | 128x128 px | Linux/Windows |
| `128x128@2x.png` | 256x256 px | macOS Retina |
| `icon.ico` | Multi-size | Windows |
| `icon.icns` | Multi-size | macOS |
| `icon.png` | 512x512 px | Tray icon |

## How to Generate

You can use the Tauri icon generator:

```bash
npx tauri icon path/to/your/source-icon.png
```

This will generate all required sizes from a single 1024x1024+ source image.

## Recommended Source

Use a PNG image at least 1024x1024 pixels with transparency.
The icon should be your "G2806" or "Nebula" logo.
