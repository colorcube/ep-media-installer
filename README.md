# `ep-media-installer` (`epmi`)

Native Rust Custom Media & `.emx` Management Tool for **Epson SureColor SC-P900** and **SC-P700** printers.

> **Disclaimer:** `ep-media-installer` (`epmi`) is an independent, clean-room open-source utility and is not affiliated with, endorsed by, or sponsored by Seiko Epson Corporation. Product names are used solely for compatibility reference.

---

## Workspace Architecture

```text
ep-media-installer/
├── crates/
│   └── epmi-core/   # 100% GUI-free Rust library (SNMPv1 NVRAM 1..30, .emx ZIP/XML/CPB parser, Cloud client, custom_media.json sync)
└── apps/
    ├── epmi/        # Fast CLI binary (`epmi list-slots`, `epmi sync`, `epmi inspect-emx`, `epmi import-emx`, ...)
    └── epmi-gui/    # Native egui/eframe desktop GUI (`epmi-gui`)
```

### Integration with `epson-p900-escpr-cups` and `LumaPrint`

`epmi` communicates with the CUPS driver (`rastertop900escpr` / `generate_ppd.py`) and **LumaPrint** via a clean file interface (no monorepo required):
- **Custom Media Database:** `~/.config/epson-p900-escpr/custom_media.json` (and automatically mirrors to `../epson-p900-escpr-cups/custom_media.json` if present)
- **Extracted ICC Profiles:** `~/.local/share/color/icc/epson-scp900/*.icc` (and `../epson-p900-escpr-cups/icc/*.icc`)

---

## CLI Usage (`epmi`)

### 1. List custom media slots (`1..30`) from the printer

```bash
cargo run -p epmi -- list-slots --host 10.10.10.124
```

### 2. Synchronize printer NVRAM slots into `custom_media.json` (and regenerate CUPS PPDs)

```bash
cargo run -p epmi -- sync --host 10.10.10.124 --regenerate-ppd
```

### 3. Inspect or import a local `.emx` package

```bash
# Inspect XML, CPB, and ICC metadata inside an .emx archive:
cargo run -p epmi -- inspect-emx Paper.emx

# Import .emx package (extracts .icc, updates custom_media.json, and optionally writes slot to printer):
cargo run -p epmi -- import-emx Paper.emx --slot 6 --host 10.10.10.124 --write-printer --regenerate-ppd
```

### 4. Write or clear a custom media slot on the printer directly

```bash
cargo run -p epmi -- write-slot \
    --host 10.10.10.124 \
    --slot 10 \
    --name "Canson Baryta Photographique II" \
    --base-media Baryta \
    --platen-gap 15 \
    --thickness-mm 0.31 \
    --dry-time 0 \
    --sync --regenerate-ppd

cargo run -p epmi -- clear-slot --host 10.10.10.124 --slot 10 --sync --regenerate-ppd
```

### 5. Browse and download `.emx` media packages from the Cloud Catalog

```bash
cargo run -p epmi -- cloud-list --printer scp9xx --region 8 --emx-only
cargo run -p epmi -- cloud-download 230009156 --import --slot 6
```

---

## Desktop GUI (`epmi-gui`)

```bash
cargo run -p epmi-gui -- --host 10.10.10.124
```

Uses the **LumaPrint / PhotoCraft** desktop UI architecture:
- **KDE System Font & Point-Size Detection:** Automatically reads `~/.config/kdeglobals` (`[General] font=Family,PointSize,...`) and loads the system font via `fc-match` plus `NotoSansSymbols2` fallbacks.
- **Native Wayland Scaling:** Converts typographic points (`72 DPI`) to logical pixels (`96 DPI`, `pt * (96.0 / 72.0)`) so Wayland fractional scaling works cleanly without double-scaling, plus a `90% / 100% / 110% / 125%` UI scale selector in the `Einstellungen` menu (`Ctrl` + `+`/`-` keyboard zooming disabled).
- **12 PhotoCraft & KDE Themes:** `KDE Breeze Dark` (default), `KDE Breeze Light`, `Pro Dunkel`, `Pro Mittelgrau`, `Pro Hellgrau`, `Pro Hell`, `Studio Dunkel`, `Studio Hell`, `GNOME Adwaita Dunkel`, `GNOME Adwaita Hell`, `Solarized Dunkel`, `Klassisch Dunkel`.
- **Clean Dock Layout:**
  - **Top Menu Bar & Action Toolbar:** `Datei`, `Drucker`, `Einstellungen`, and `Hilfe` (`Über epmi…`) menus plus quick access to SNMP slot polling, `custom_media.json` + PPD sync, and workspace switcher.
  - **Central Workspace:**
    1. **🖨 Drucker-Slots (1..30):** Live SNMPv1 table of all 30 NVRAM slots on the SC-P900/P700 with PK/MK indicator, `MA_ID` (`ESC q setl`), `BaseMedia` (`ESC q setq`), thickness, and platen gap.
    2. **📦 Lokale `.emx`-Pakete:** Inspect `.emx` archives and import their parameters + `.icc` profile into any slot with 1 click.
    3. **☁ Epson Cloud-Katalog:** Browse and 1-click download/import `.emx` packages for `scp9xx` (SC-P900) and `scp7xx` (SC-P700).
  - **Resizable Right Inspector Dock (`SidePanel::right`):** Structured sections separated by horizontal lines (`1. Slot-Identität & Basismedium`, `2. Mechanische Parameter (NVRAM TLV)`, `3. CUPS / LumaPrint & ICC-Profil`, `4. Drucker-NVRAM Aktionen`).

---

## License

GNU General Public License v3.0 or later, see [LICENSE](LICENSE).
