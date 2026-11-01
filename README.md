# `ep-media-installer` (`epmi`)

Native Rust Custom Media & `.emx` Management Tool for **Epson SureColor** Fine Art & Large Format Printers (SC-P900, SC-P700, SC-P5300, SC-P7500, SC-P9500, SC-T Series, and more).

> **Disclaimer:** `ep-media-installer` (`epmi`) is an independent, clean-room open-source utility and is not affiliated with, endorsed by, or sponsored by Seiko Epson Corporation. Product names are used solely for compatibility reference.

> [!WARNING]
> **Beta.** Tested on Fedora with an Epson SureColor **SC-P900**: discovering the printer, reading the
> 30 custom media slots, the cloud catalog, `.emx` import and „Für das System installieren“ together
> with [epson-p900-escpr-cups](https://github.com/colorcube/epson-p900-escpr-cups). Writing and clearing
> slots changes the printer's memory – the other SureColor models are listed from Epson's cloud data but
> have not been tested. The user interface is currently German only (English is planned).

---

## Workspace Architecture

```text
ep-media-installer/
├── crates/
│   └── epmi-core/   # 100% GUI-free Rust library (CUPS + SNMP printer discovery, NVRAM slots 1..30, .emx parser, 27-model Cloud client, custom_media.json sync)
└── apps/
    ├── epmi/        # Fast CLI binary (`epmi discover`, `epmi list-slots`, `epmi sync`, `epmi cloud-models`, `epmi cloud-list`, ...)
    └── epmi-gui/    # Native egui/eframe desktop GUI (`epmi-gui`)
```

### Integration with [`epson-p900-escpr-cups`](https://github.com/colorcube/epson-p900-escpr-cups) and [`LumaPrint`](https://github.com/colorcube/lumaprint)

Like Epson's own software on Windows/macOS, custom media live in two places: the **printer**
stores the mechanical settings in its 30 NVRAM slots, every **computer** that prints needs the
paper list and the ICC profiles locally. Epson's FAQ: the Media Installer must run on every
computer.

| What | Where | Written by |
|---|---|---|
| Mechanical settings, media ID | printer NVRAM slot `1..30` | `epmi` (SNMP) |
| Working copy (per user) | `~/.config/epson-p900-escpr/custom_media.json`, profiles in `~/.local/share/color/icc/epson-scp900/` | `epmi` |
| System paper list (CUPS filter, PPD, LumaPrint) | `/etc/epson-p900-escpr-cups/custom_media.json` | driver helper, via **„Für das System installieren“** |
| System colour profiles | `/usr/local/share/color/icc/epson-p900-custom/` | driver helper |

**„Für das System installieren“** (GUI button, CLI `epmi install-system`) reads the printer's slots,
fetches the profiles of Epson cloud media automatically (the slot's media ID equals the cloud
`.emx` ID), and hands the list to the privileged helper of the driver
(`pkexec /usr/libexec/epson-p900-escpr-cups/epson-p900-media-install`, administrator password).
The helper installs list and profiles and refreshes the PPDs of all CUPS queues of the driver.
Requires [`epson-p900-escpr-cups`](https://github.com/colorcube/epson-p900-escpr-cups) to be installed.

Papers that are not in the Epson cloud (e.g. third-party papers) get their profile assigned once
(GUI: 📂 next to *Farbprofil*, CLI: `--icc <SLOT>=<FILE.icc>`). To set up a second computer,
export them (`.epmi` archive with profiles) and import them there.

---

## CLI Usage (`epmi`)

### 1. Automatically discover Epson printers (CUPS + local network SNMP)

```bash
cargo run -p epmi -- discover
```

### 2. List custom media slots (`1..30`) from the printer

```bash
# Automatically discovers the Epson printer on the network if --host is omitted:
cargo run -p epmi -- list-slots
```

### 3. Install the printer's papers for this computer

```bash
# State of each slot on this computer and its colour profile:
cargo run -p epmi -- status

# Install paper list + profiles for the CUPS driver and LumaPrint (asks for the admin password);
# assign an own profile to slot 2 on the way:
cargo run -p epmi -- install-system --icc 2=Hahnemuehle_PhotoRag308.icc

# Move papers with their profiles to another computer:
cargo run -p epmi -- export -o my-papers.epmi
cargo run -p epmi -- import my-papers.epmi      # on the other computer, then install-system
```

### 4. Inspect or import a local `.emx` package

```bash
# Inspect XML, CPB, and ICC metadata inside an .emx archive:
cargo run -p epmi -- inspect-emx Paper.emx

# Import .emx package (extracts .icc, updates the working copy, and writes slot to printer):
cargo run -p epmi -- import-emx Paper.emx --slot 6 --write-printer
cargo run -p epmi -- install-system
```

### 5. Browse and download `.emx` media packages from the Cloud Catalog (27 SureColor series)

```bash
# List all 27 supported SureColor P-Series & T-Series models:
cargo run -p epmi -- cloud-models

# List media packages for SC-P900 (scp9xx), SC-P700 (scp7xx), SC-P5300 (scp53xx), etc.:
cargo run -p epmi -- cloud-list --printer scp9xx --emx-only
cargo run -p epmi -- cloud-download 230009156 --import --slot 6 --write-printer
```

---

## Desktop GUI (`epmi-gui`)

```bash
cargo run -p epmi-gui
```

- **Automatic Epson Printer Discovery:** Finds Epson printers automatically via installed CUPS queues (`lpstat -v` / PPD) and local network SNMPv1 discovery (`hrDeviceDescr` / IEEE-1284 Device ID), strictly filtering out non-Epson devices. Automatically selects the matching model in the Cloud Catalog and reads all 30 memory slots on startup.
- **1-Click Installation Workflow:** Choose a target slot (defaults to the next free slot, e.g. `Platz 06`) and click **`📲 Auf Platz XX installieren`** in the Cloud Catalog or Local `.emx` view to extract the `.icc` profile, write the paper into the printer's memory slot, and update the working copy; then **`🖥 Für das System installieren`** makes it available to the CUPS driver and LumaPrint.
- **All 27 SureColor Models in Cloud Catalog:** Supports the full SureColor SC-P (`SC-P700` to `SC-P20500`) and SC-T (`SC-T3700` to `SC-T7700`) product lines.
- **LumaPrint / PhotoCraft Desktop UI:**
  - **KDE System Font & Point-Size Detection:** Reads `~/.config/kdeglobals` and loads the system font via `fc-match`.
  - **Native Wayland Scaling:** Converts typographic points (`72 DPI`) to logical pixels (`96 DPI`, `pt * (96.0 / 72.0)`) with a `90% / 100% / 110% / 125%` UI scale selector in the `Einstellungen` menu (`Ctrl` + `+`/`-` keyboard zooming disabled).
  - **12 PhotoCraft & KDE Themes:** `KDE Breeze Dark` (default), `KDE Breeze Light`, `Pro Dunkel`, `Pro Mittelgrau`, `Pro Hellgrau`, `Pro Hell`, `Studio Dunkel`, `Studio Hell`, `GNOME Adwaita Dunkel`, `GNOME Adwaita Hell`, `Solarized Dunkel`, `Klassisch Dunkel`.
  - **User-Friendly Inspector Dock:** Clear German photography terms with a collapsible `5. Technische Details (Expertenansicht)` section for low-level IDs.

## Privacy

`epmi` talks to your printer over SNMP (UDP 161) in the local network and, for the cloud catalog and
automatic profiles, to Epson's media server `https://support.epson.biz/lfp/cms/` (printer series,
country and language are sent, as Epson's own Media Installer does). Downloaded `.emx` packages are
cached in `~/.cache/epmi/emx/`. There is no telemetry.

## About the development

ep-media-installer was developed with extensive help from AI. The code, the tests and the documentation were written by Google's Gemini and Anthropic's Claude under my direction. Requirements, design decisions, review and all tests with real hardware are my own.

## License

GNU General Public License v3.0 or later, see [LICENSE](LICENSE).
