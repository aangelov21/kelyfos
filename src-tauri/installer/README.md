# Installer artwork

Windows installer graphics, generated from `../icons/icon.png` so they stay in
step with the app icon. Wired up in `tauri.conf.json` under `bundle.windows`.

| File | Size | Used by |
| --- | --- | --- |
| `header.bmp` | 150×57 | NSIS, top corner of each wizard page |
| `sidebar.bmp` | 164×314 | NSIS, welcome and finish pages |
| `wix-banner.bmp` | 493×58 | MSI, top banner |
| `wix-dialog.bmp` | 493×312 | MSI, welcome page |

These must be **24-bit Windows BMPs**. A PNG renamed to `.bmp`, or a BMP with an
alpha channel, is silently ignored or drawn as garbage — `file` should report
`PC bitmap, Windows 3.x format, … x 24`.

To regenerate after changing the icon (needs Pillow, and the macOS font paths in
the script):

```sh
python3 -m venv /tmp/venv && /tmp/venv/bin/pip install pillow
/tmp/venv/bin/python src-tauri/installer/generate.py   # run from the repo root
```
