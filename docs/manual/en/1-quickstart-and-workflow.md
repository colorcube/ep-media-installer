# 1. Quickstart & 1-Click Workflow

This chapter walks you through installing **EP Media Installer (`epmi`)**, connecting to your Epson SureColor printer, registering a Fine-Art paper in a printer NVRAM slot, and making it available to CUPS and **LumaPrint**.

---

## 1. Installation & Launch

### Build & Install from Source

```bash
cargo build --release --workspace
sudo install -m 0755 target/release/epmi target/release/epmi-gui /usr/local/bin/
./bin/install-desktop
```

Running `./bin/install-desktop` registers **EP Media Installer (epmi)** (`de.colorcube.epmi`) with its vector icon in your desktop application launcher (KDE Plasma, GNOME, Xfce).

### Launch the GUI

Start **EP Media Installer (epmi)** from your application menu or run:

```bash
epmi-gui
```

If your printer is not discovered automatically via CUPS, you can pass its IP address or hostname directly:

```bash
epmi-gui --host 192.168.178.50
```

---

## 2. Typical Workflow in 3 Steps

### Step 1: Select Your Printer & Read Slots

When `epmi-gui` starts, it automatically scans local CUPS queues (`lpstat -v`) and probes discovered network hosts via SNMP (`UDP 161`).

1. Select your printer in the **Epson printer** dropdown at the top left (or choose **⌨ Check manual IP / host name…** and click **Connect**).
2. Click **🔄 Refresh** to read all 30 NVRAM slots (`Slot 01`–`30`) from the printer.

### Step 2: Add or Edit a Fine-Art Paper

Depending on where your paper profile comes from, choose one of three paths:

- **Option A — Third-Party Fine-Art Paper with `.icc` Profile (Hahnemühle, Canson, Ilford):**
  1. Click a free slot (e.g. `Slot 01`) in the **🖨 Slots (1..30)** table.
  2. In the **★ Paper settings** dock on the right, enter the **Name on display** (e.g. `Hahnemuehle Photo Rag 308`), select the **Base paper type** recommended by the manufacturer's data sheet (e.g. `Velvet Fine Art Paper (Matte · MK)`), and set the **Paper thickness** (e.g. `0.48 mm`).
  3. Under **3. Color profile (ICC) & ink density**, click the folder icon (`📂`) to select the manufacturer's `.icc` / `.icm` file.
  4. Click **💾 Save slot 01 to printer** at the bottom right to write the mechanical settings into the printer's NVRAM.
- **Option B — Official Cloud Catalog Paper:**
  1. Switch to **☁ Epson cloud catalog** in the top toolbar and click **☁ Fetch catalog**.
  2. Pick a target slot (`✨ Free slot`) and click **📲 Install to slot** next to the desired paper.
- **Option C — Local `.emx` Media Package:**
  1. Switch to **📦 Local .emx file**, click **📂 Browse…** to load an `.emx` file, and click **📲 Install directly to slot**.

### Step 3: Click *"🖥 Install for this computer"*

Once your slots are configured:

1. Click **🖥 Install for this computer** in the **🖨 Slots (1..30)** view.
2. Enter your administrator password when prompted by Polkit (`pkexec`).

`epmi` installs all custom media definitions and ICC profiles into `/etc/epson-p900-escpr-cups/custom_media.json` and `/usr/local/share/color/icc/epson-p900-custom/` and updates your CUPS PPD. Both CUPS and **LumaPrint** immediately list your custom papers (complete with automatic ICC profile selection).

---

## 3. Command-Line Quickstart (`epmi`)

Every action available in `epmi-gui` can also be scripted via the `epmi` CLI:

```bash
# 1. Discover Epson printers via CUPS and local network SNMP
epmi discover

# 2. Read all 30 NVRAM slots from the printer
epmi list-slots --host 192.168.178.50

# 3. Import a local .emx package into Slot 6 and write it to the printer
epmi import-emx Paper.emx --slot 6 --write-printer --host 192.168.178.50

# 4. Install all slots & ICC profiles system-wide for CUPS and LumaPrint
#    (optionally assigning a third-party ICC profile to a slot)
epmi install-system --host 192.168.178.50 --icc 1=~/Downloads/HFA_EpsP900_MK_PhotoRag308.icc
```
