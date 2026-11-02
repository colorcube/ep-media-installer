# Changelog

## 0.1.0 – unreleased (beta)

First public version.

### Printer
- Discovery of Epson printers via CUPS queues and SNMP in the local network.
- Read, write and clear the printer's 30 custom media slots over SNMP (mechanical settings, media ID).

### Media
- `.emx` packages: inspect, import (ICC profile, mechanical settings, ink density) and write to a slot.
- Epson cloud catalog for 27 SureColor models: list and download `.emx` packages.

### This computer (“Install for this computer”)
- Installs paper list and ICC profiles for all users, the CUPS driver
  [epson-p900-escpr-cups](https://github.com/colorcube/epson-p900-escpr-cups) and
  [LumaPrint](https://github.com/colorcube/lumaprint), via the driver's privileged helper (pkexec).
- Profiles of Epson cloud media are fetched automatically (slot media ID = cloud `.emx` ID).
- Own profiles can be assigned per paper; export/import with profiles (`.epmi`) for other computers.
- Status per paper: installed / changed / not installed, own profile / base profile only / missing.

### Interfaces & desktop integration
- CLI `epmi` (`discover`, `list-slots`, `status`, `install-system`, `export`, `import`, `import-emx`,
  `write-slot`, `clear-slot`, `cloud-list`, `cloud-download`, …) and desktop GUI `epmi-gui`.
- Bilingual user interface in `epmi-gui` (English by default, German according to system language or switchable under Settings → Language).
- Application icon, Wayland/X11 desktop entry (`share/applications/de.colorcube.epmi.desktop`, *EP Media Installer (epmi)*), AppStream metainfo (`share/metainfo/de.colorcube.epmi.metainfo.xml`), and `./bin/install-desktop` (`--uninstall`) installer.
