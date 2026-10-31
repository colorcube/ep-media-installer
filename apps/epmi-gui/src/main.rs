//! `epmi-gui` — Native desktop GUI for Epson SureColor media management.
//!
//! Incorporates the LumaPrint / PhotoCraft UI architecture:
//! - Automatic KDE Plasma (`~/.config/kdeglobals`) font family & point-size adoption
//! - Wayland-safe fractional scaling (`pt * (96.0 / 72.0)` logical pixels + UI scale)
//! - 12 PhotoCraft themes (`BreezeDark`, `BreezeLight`, `Pro`, `Studio`, `Adwaita`, `SolarizedDark`, ...)
//! - Automatic Epson printer discovery (CUPS queues + SNMPv1 network scan)
//! - All 27 Epson SureColor models supported in the Cloud Catalog
//! - Intuitive 1-click installation workflow for `.emx` packages and Cloud profiles

mod theme;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use egui::{Context, CornerRadius, Margin, RichText, Stroke, Ui, Vec2};
use epmi_core::{
    BASE_MEDIA_SPECS, CLOUD_PRINTER_MODELS, CloudMediaRecord, CustomMediaEntry,
    DiscoveredEpsonPrinter, DiscoverySource, EmxPackage, PrinterMediaSlot, ProfileSource,
    SnmpMediaClient, SystemState, assign_profile, cloud_download_emx, cloud_list_media,
    default_icc_output_dirs, default_user_custom_media_json, discover_epson_printers,
    discover_sync_targets, entries_for_slots, export_media, fetch_cloud_profiles,
    find_base_media_by_sn_id, import_emx_package, import_media, install_system_media,
    load_custom_media_json, load_system_media, match_printer_to_cloud_series, media_category_label,
    merge_imported, merge_slots_with_existing, parse_emx_file, platen_gap_label,
    probe_epson_printer_snmp, resolve_profile, sanitize_ppd_media_key, save_custom_media_json,
    system_state,
};
use theme::{
    KdeTypography, ThemeKind, Tokens, apply_theme_and_typography, install_kde_system_fonts,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    PrinterSlots,
    CloudCatalog,
    EmxPackages,
}

/// Native Linux/KDE file dialog (`kdialog`, fallback `zenity`) for opening or saving a file.
/// `patterns` is a space-separated glob list such as `"*.icc *.icm"`.
fn native_file_dialog(title: &str, label: &str, patterns: &str, save: bool) -> Option<String> {
    let kde_filter = format!("{label} ({patterns})|Alle Dateien (*)");
    let kde_mode = if save {
        "--getsavefilename"
    } else {
        "--getopenfilename"
    };
    if let Ok(out) = Command::new("kdialog")
        .args(["--title", title, kde_mode, ".", kde_filter.as_str()])
        .output()
        && out.status.success()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(p);
        }
    }

    let mut zenity_args = vec![
        String::from("--file-selection"),
        format!("--title={title}"),
        format!("--file-filter={label} | {patterns}"),
    ];
    if save {
        zenity_args.push(String::from("--save"));
        zenity_args.push(String::from("--confirm-overwrite"));
    }
    if let Ok(out) = Command::new("zenity").args(&zenity_args).output()
        && out.status.success()
    {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() {
            return Some(p);
        }
    }

    None
}

/// Open a native file dialog to select a `.emx` file.
fn open_native_emx_file_dialog() -> Option<String> {
    native_file_dialog(
        "Epson Medienpaket (.emx) auswählen",
        "Epson Media Package",
        "*.emx *.emy",
        false,
    )
}

pub struct EpmiGuiApp {
    /// Selected PhotoCraft / KDE theme.
    theme_kind: ThemeKind,
    /// Active theme color and radius tokens.
    tokens: Tokens,
    /// Detected KDE font family & point size from `~/.config/kdeglobals`.
    kde_typography: KdeTypography,
    /// Additional user UI scale multiplier (`0.9` .. `1.25`).
    ui_scale: f32,
    /// Show/hide the right-hand paper settings dock.
    show_right_dock: bool,

    /// Active central workspace view.
    active_view: WorkspaceView,
    /// Show all 30 slots or only occupied slots in the central view.
    show_empty_slots: bool,

    /// Discovered Epson printers from CUPS and local network SNMP.
    discovered_printers: Vec<DiscoveredEpsonPrinter>,
    /// Index of the selected printer in `discovered_printers` (or `None` when manual mode is active).
    selected_printer_idx: Option<usize>,
    /// Whether the user selected manual host entry.
    manual_host_mode: bool,
    /// Manual host input field (only accepted if verified as an Epson printer).
    manual_host_input: String,
    /// Active printer IPv4 address / hostname.
    printer_host: String,
    /// Active printer model name.
    printer_model_name: String,

    /// All 30 NVRAM user slots (`1..=30`).
    slots: Vec<PrinterMediaSlot>,
    /// Currently selected slot (`1..=30`) in the Right Dock editor.
    selected_slot: u8,
    /// Target slot (`1..=30`) for 1-click installation in Cloud & `.emx` views.
    install_target_slot: u8,
    /// Editable working copy of the selected slot in the Right Dock.
    editor_slot: PrinterMediaSlot,
    /// Editable print duty (`-50..=50%`) for `custom_media.json` / driver.
    editor_print_duty: i8,
    /// Editable ICC profile filename for `custom_media.json` / `*cupsICCProfile`.
    editor_icc_profile: String,

    /// Local `.emx` file path input.
    emx_input_path: String,
    /// Currently loaded `.emx` package.
    loaded_emx: Option<EmxPackage>,

    /// Selected Cloud Catalog printer series code (from `CLOUD_PRINTER_MODELS`, e.g. `"scp9xx"`).
    cloud_model_code: String,
    /// Fetched cloud catalog items.
    cloud_items: Vec<CloudMediaRecord>,
    /// Filter query for cloud catalog / slots.
    search_filter: String,
    /// Show only downloadable custom/add-on `.emx` packages in Cloud Catalog.
    cloud_only_custom_emx: bool,

    /// Primary `custom_media.json` path.
    custom_media_json_path: PathBuf,

    /// Show/hide the "Über epmi" modal window.
    show_about_modal: bool,

    /// Status bar message & state.
    status_msg: String,
    status_is_error: bool,
}

impl EpmiGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_host: Option<String>) -> Self {
        cc.egui_ctx.options_mut(|opt| {
            opt.zoom_with_keyboard = false;
        });
        let kde_typography = install_kde_system_fonts(&cc.egui_ctx);
        let theme_kind = ThemeKind::BreezeDark;
        let tokens = theme_kind.tokens();
        let ui_scale = 1.0_f32;
        apply_theme_and_typography(&cc.egui_ctx, theme_kind, &kde_typography, ui_scale);

        let slots: Vec<PrinterMediaSlot> = (1..=30).map(PrinterMediaSlot::empty).collect();
        let editor_slot = slots
            .first()
            .cloned()
            .unwrap_or_else(|| PrinterMediaSlot::empty(1));

        let custom_media_json_path = default_user_custom_media_json();

        let mut app = Self {
            theme_kind,
            tokens,
            kde_typography,
            ui_scale,
            show_right_dock: true,
            active_view: WorkspaceView::PrinterSlots,
            show_empty_slots: true,
            discovered_printers: Vec::new(),
            selected_printer_idx: None,
            manual_host_mode: false,
            manual_host_input: initial_host.clone().unwrap_or_default(),
            printer_host: initial_host.clone().unwrap_or_default(),
            printer_model_name: String::from("Kein Epson-Drucker verbunden"),
            slots,
            selected_slot: 1,
            install_target_slot: 1,
            editor_slot,
            editor_print_duty: 0,
            editor_icc_profile: String::from("Epson_SC-P900_700_VelvetFineArtPaper.icc"),
            emx_input_path: String::new(),
            loaded_emx: None,
            cloud_model_code: String::from("scp9xx"),
            cloud_items: Vec::new(),
            search_filter: String::new(),
            cloud_only_custom_emx: false,
            custom_media_json_path,
            show_about_modal: false,
            status_msg: String::from("Suche nach Epson-Druckern in CUPS und im Netzwerk…"),
            status_is_error: false,
        };

        // Pre-populate slots from existing custom_media.json if available
        let existing = load_custom_media_json(&app.custom_media_json_path);
        if !existing.is_empty() {
            for entry in &existing {
                if (1..=30).contains(&entry.slot)
                    && let Some(slot_ref) = app.slots.get_mut(usize::from(entry.slot - 1))
                {
                    slot_ref.registered = true;
                    slot_ref.user_media_id = entry.user_media_id;
                    slot_ref.media_name = if entry.media_name.is_empty() {
                        entry.name.clone()
                    } else {
                        entry.media_name.clone()
                    };
                    slot_ref.base_sn_id = entry.base_sn_id;
                    slot_ref.media_category = entry.media_category;
                    slot_ref.platen_gap = entry.platen_gap;
                    slot_ref.thickness_01mm = entry.thickness;
                    slot_ref.cut_sheet_feed_offset = entry.cut_sheet_feed_offset;
                    slot_ref.roll_paper_feed_offset = entry.roll_paper_feed_offset;
                    slot_ref.drying_time = entry.drying_time;
                    slot_ref.paper_width_check = entry.paper_width_check;
                }
            }
            if let Some(first) = existing.first() {
                app.select_slot(first.slot.max(1));
                app.editor_print_duty = first.print_duty;
                app.editor_icc_profile = first.icc_profile.clone();
            }
            app.install_target_slot = app.next_free_slot();
        }

        // Run automatic Epson printer discovery on startup
        let extra: Vec<&str> = initial_host.as_deref().into_iter().collect();
        app.refresh_printer_discovery(&extra);

        app
    }

    fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_msg = msg.into();
        self.status_is_error = is_error;
    }

    fn next_free_slot(&self) -> u8 {
        self.slots
            .iter()
            .find(|s| !s.registered)
            .map(|s| s.slot)
            .unwrap_or(1)
    }

    fn refresh_printer_discovery(&mut self, extra_hosts: &[&str]) {
        let found = discover_epson_printers(extra_hosts);
        self.discovered_printers = found;
        if let Some(first) = self.discovered_printers.first().cloned() {
            self.selected_printer_idx = Some(0);
            self.manual_host_mode = false;
            self.apply_selected_printer(&first);
        } else {
            self.selected_printer_idx = None;
            self.set_status(
                "Kein Epson-Drucker automatisch gefunden. Klicke auf '🔍 Drucker suchen' oder wähle 'Manuelle Adresse…'.",
                false,
            );
        }
    }

    fn apply_selected_printer(&mut self, printer: &DiscoveredEpsonPrinter) {
        self.printer_host = printer.host.clone();
        self.printer_model_name = printer.model_name.clone();
        if let Some(code) = &printer.cloud_series_code {
            self.cloud_model_code = code.clone();
        } else if let Some(matched) = match_printer_to_cloud_series(&printer.model_name) {
            self.cloud_model_code = matched.code.to_string();
        }
        if printer.online {
            self.poll_printer_slots();
        } else {
            self.set_status(
                format!(
                    "Epson-Drucker '{}' ({}) ausgewählt. Klicke auf '🔄 Aktualisieren', sobald der Drucker eingeschaltet ist.",
                    printer.model_name, printer.host
                ),
                false,
            );
        }
    }

    fn verify_and_connect_manual_host(&mut self) {
        let candidate = self.manual_host_input.trim().to_string();
        if candidate.is_empty() {
            self.set_status("Bitte eine IP-Adresse oder einen Hostnamen eingeben.", true);
            return;
        }
        match probe_epson_printer_snmp(&candidate, Duration::from_millis(1200)) {
            Some(model_name) => {
                let cloud_code =
                    match_printer_to_cloud_series(&model_name).map(|m| m.code.to_string());
                let entry = DiscoveredEpsonPrinter {
                    host: candidate.clone(),
                    model_name,
                    source: DiscoverySource::Manual,
                    cloud_series_code: cloud_code,
                    online: true,
                };
                if let Some(pos) = self
                    .discovered_printers
                    .iter()
                    .position(|p| p.host == candidate)
                {
                    self.discovered_printers[pos] = entry.clone();
                    self.selected_printer_idx = Some(pos);
                } else {
                    self.discovered_printers.push(entry.clone());
                    self.selected_printer_idx = Some(self.discovered_printers.len() - 1);
                }
                self.manual_host_mode = false;
                self.apply_selected_printer(&entry);
            }
            None => {
                self.set_status(
                    format!(
                        "Unter '{candidate}' wurde kein unterstützter Epson-Drucker per SNMP gefunden (nur Epson-Drucker sind zulässig)."
                    ),
                    true,
                );
            }
        }
    }

    fn select_slot(&mut self, slot_num: u8) {
        let clamped = slot_num.clamp(1, 30);
        self.selected_slot = clamped;
        self.install_target_slot = clamped;
        if let Some(s) = self.slots.get(usize::from(clamped - 1)).cloned() {
            self.editor_slot = s;
        }
        let existing = load_custom_media_json(&self.custom_media_json_path);
        if let Some(found) = existing.iter().find(|e| e.slot == clamped) {
            self.editor_print_duty = found.print_duty;
            self.editor_icc_profile = found.icc_profile.clone();
        } else {
            self.editor_print_duty = 0;
            self.editor_icc_profile = self.editor_slot.base_media_spec().icc_profile.to_string();
        }
    }

    fn poll_printer_slots(&mut self) {
        if self.printer_host.trim().is_empty() {
            self.set_status("Kein Epson-Drucker ausgewählt.", true);
            return;
        }
        let client = SnmpMediaClient::new(self.printer_host.trim());
        match client.read_all_slots(false) {
            Ok(read_slots) => {
                let registered = read_slots.iter().filter(|s| s.registered).count();
                self.slots = read_slots;
                self.select_slot(self.selected_slot);
                self.install_target_slot = self.next_free_slot();
                self.set_status(
                    format!(
                        "Verbunden mit {} ({}): {registered} von 30 Speicherplätzen im Drucker belegt.",
                        self.printer_model_name, self.printer_host
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    format!(
                        "Drucker {} ({}) antwortet nicht: {e}",
                        self.printer_model_name, self.printer_host
                    ),
                    true,
                );
            }
        }
    }

    fn sync_slots_to_driver(&mut self) {
        let targets = discover_sync_targets(Some(&self.custom_media_json_path));
        let mut existing = Vec::new();
        for t in &targets {
            let loaded = load_custom_media_json(t);
            if !loaded.is_empty() {
                existing = loaded;
                break;
            }
        }

        let updated = merge_slots_with_existing(&self.slots, &existing);
        let mut written = Vec::new();
        for t in &targets {
            if save_custom_media_json(t, &updated).is_ok() {
                written.push(t.display().to_string());
            }
        }
        self.set_status(
            format!(
                "{} Papiere in die Arbeitskopie übernommen ({}). Für Druckertreiber & LumaPrint: „Für das System installieren“.",
                updated.len(),
                written.join(", ")
            ),
            false,
        );
    }

    fn write_selected_slot_to_printer(&mut self) {
        if self.printer_host.trim().is_empty() {
            self.set_status("Bitte zuerst oben einen Epson-Drucker auswählen.", true);
            return;
        }
        if self.editor_slot.media_name.trim().is_empty() {
            self.set_status("Bitte zuerst einen Papiernamen eingeben.", true);
            return;
        }
        if self.editor_slot.user_media_id == 0 {
            self.editor_slot.user_media_id = 910_000_000 + u32::from(self.selected_slot);
        }
        self.editor_slot.slot = self.selected_slot;
        self.editor_slot.registered = true;

        let client = SnmpMediaClient::new(self.printer_host.trim());
        match client.write_slot(&self.editor_slot) {
            Ok(verified) => {
                if let Some(slot_ref) = self.slots.get_mut(usize::from(self.selected_slot - 1)) {
                    *slot_ref = verified.clone();
                }
                self.editor_slot = verified.clone();

                // Update custom_media.json with custom ICC & print_duty
                let targets = discover_sync_targets(Some(&self.custom_media_json_path));
                let existing = load_custom_media_json(&self.custom_media_json_path);
                let mut entry = verified
                    .to_custom_media_entry(existing.iter().find(|e| e.slot == self.selected_slot));
                entry.print_duty = self.editor_print_duty;
                if !self.editor_icc_profile.trim().is_empty() {
                    entry.icc_profile = self.editor_icc_profile.trim().to_string();
                }

                let mut merged = merge_slots_with_existing(&self.slots, &existing);
                if let Some(pos) = merged.iter_mut().find(|e| e.slot == self.selected_slot) {
                    *pos = entry;
                } else {
                    merged.push(entry);
                }
                for t in &targets {
                    let _ = save_custom_media_json(t, &merged);
                }
                self.install_target_slot = self.next_free_slot();

                self.set_status(
                    format!(
                        "Speicherplatz {:02} ('{}') im Drucker gespeichert. Für Druckertreiber & LumaPrint: „Für das System installieren“.",
                        self.selected_slot, verified.media_name
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    format!(
                        "Fehler beim Speichern auf Speicherplatz {:02}: {e}",
                        self.selected_slot
                    ),
                    true,
                );
            }
        }
    }

    fn clear_selected_slot_on_printer(&mut self) {
        if self.printer_host.trim().is_empty() {
            self.set_status("Bitte zuerst oben einen Epson-Drucker auswählen.", true);
            return;
        }
        let client = SnmpMediaClient::new(self.printer_host.trim());
        match client.clear_slot(self.selected_slot) {
            Ok(cleared) => {
                if let Some(slot_ref) = self.slots.get_mut(usize::from(self.selected_slot - 1)) {
                    *slot_ref = cleared.clone();
                }
                self.editor_slot = cleared;
                self.sync_slots_to_driver();
                self.install_target_slot = self.next_free_slot();
                self.set_status(
                    format!(
                        "Speicherplatz {:02} im Drucker geleert und Treiberliste aktualisiert.",
                        self.selected_slot
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    format!(
                        "Fehler beim Leeren von Speicherplatz {:02}: {e}",
                        self.selected_slot
                    ),
                    true,
                );
            }
        }
    }

    /// Load an `.emx` package into the Right Dock editor for manual inspection/adjustment.
    fn apply_emx_to_editor(&mut self, pkg: &EmxPackage, target_slot: u8) {
        if !pkg.icc_profile_name.is_empty() && !pkg.icc_profile_bytes.is_empty() {
            for dir in default_icc_output_dirs() {
                if std::fs::create_dir_all(&dir).is_ok() {
                    let _ = std::fs::write(dir.join(&pkg.icc_profile_name), &pkg.icc_profile_bytes);
                }
            }
        }

        let clamped = target_slot.clamp(1, 30);
        self.selected_slot = clamped;
        self.editor_slot = pkg.to_printer_slot(clamped);
        self.editor_print_duty = pkg.print_duty();
        self.editor_icc_profile = if pkg.icc_profile_name.is_empty() {
            self.editor_slot.base_media_spec().icc_profile.to_string()
        } else {
            pkg.icc_profile_name.clone()
        };

        self.set_status(
            format!(
                "'{}' in den Editor für Speicherplatz {:02} geladen. Klicke rechts auf '💾 Auf Drucker & System speichern', um es zu übertragen.",
                pkg.media_name, clamped
            ),
            false,
        );
    }

    /// 1-Click workflow: Install an `.emx` file directly to the printer slot + ICC folder + `custom_media.json` + PPD.
    fn install_emx_one_click(&mut self, emx_path: &std::path::Path, target_slot: u8) {
        let clamped = target_slot.clamp(1, 30);
        let client_opt = if !self.printer_host.trim().is_empty() {
            Some(SnmpMediaClient::new(self.printer_host.trim()))
        } else {
            None
        };

        match import_emx_package(
            emx_path,
            clamped,
            Some(&self.custom_media_json_path),
            client_opt.as_ref(),
        ) {
            Ok((pkg, entry, saved_iccs)) => {
                if let Some(slot_ref) = self.slots.get_mut(usize::from(clamped - 1)) {
                    *slot_ref = pkg.to_printer_slot(clamped);
                }
                self.selected_slot = clamped;
                self.editor_slot = pkg.to_printer_slot(clamped);
                self.editor_print_duty = entry.print_duty;
                self.editor_icc_profile = entry.icc_profile.clone();
                self.loaded_emx = Some(pkg.clone());
                self.install_target_slot = self.next_free_slot();

                let icc_note = if saved_iccs.is_empty() {
                    String::from("Standard-ICC")
                } else {
                    format!("ICC '{}' installiert", entry.icc_profile)
                };
                let printer_note = if client_opt.is_some() {
                    format!("in Drucker-Speicherplatz {:02} übertragen", clamped)
                } else {
                    format!("für Speicherplatz {:02} lokal gespeichert", clamped)
                };
                self.set_status(
                    format!(
                        "✅ '{}' erfolgreich {printer_note} ({icc_note}). Für Druckertreiber & LumaPrint: „Für das System installieren“.",
                        pkg.media_name
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(format!("Installation fehlgeschlagen: {e}"), true);
            }
        }
    }

    /// Working copy (`custom_media.json` of the user), seeded from the system installation.
    fn working_copy(&self) -> Vec<CustomMediaEntry> {
        let working = load_custom_media_json(&self.custom_media_json_path);
        if working.is_empty() {
            load_system_media()
        } else {
            working
        }
    }

    /// „Für das System installieren“: registered printer slots + working copy settings, profiles of
    /// Epson cloud media fetched automatically, installed for all users and the CUPS driver
    /// (administrator password via pkexec).
    fn install_for_system(&mut self) {
        let mut entries = entries_for_slots(&self.slots, &self.working_copy());
        if entries.is_empty() {
            self.set_status(
                "Keine belegten Speicherplätze – bitte zuerst den Drucker auslesen.",
                true,
            );
            return;
        }
        let code = match_printer_to_cloud_series(&self.printer_model_name)
            .map(|m| m.code.to_string())
            .unwrap_or_else(|| self.cloud_model_code.clone());
        let cloud_note = match cloud_list_media(&code, "DE", 8, "de")
            .and_then(|list| fetch_cloud_profiles(&mut entries, &list))
        {
            Ok(done) if !done.is_empty() => {
                format!(" Profile aus der Epson-Cloud: {}.", done.join(", "))
            }
            Ok(_) => String::new(),
            Err(e) => format!(" (Epson-Cloud nicht erreichbar: {e})"),
        };
        let _ = save_custom_media_json(&self.custom_media_json_path, &entries);
        match install_system_media(&entries) {
            Ok(_) => self.set_status(
                format!(
                    "✅ {} Papier(e) für dieses System installiert – Druckertreiber und LumaPrint sind aktualisiert.{cloud_note}",
                    entries.len()
                ),
                false,
            ),
            Err(e) => self.set_status(format!("Installation fehlgeschlagen: {e}{cloud_note}"), true),
        }
    }

    /// Assigns an ICC profile file to the selected slot in the working copy.
    fn choose_icc_for_selected_slot(&mut self) {
        let Some(file) = native_file_dialog(
            "Farbprofil (.icc) für das Papier auswählen",
            "ICC-Profil",
            "*.icc *.icm *.ICC *.ICM",
            false,
        ) else {
            return;
        };
        let mut working = self.working_copy();
        let slot = self.selected_slot;
        let idx = if let Some(i) = working.iter().position(|e| e.slot == slot) {
            i
        } else {
            let Some(s) = self
                .slots
                .get(usize::from(slot.saturating_sub(1)))
                .filter(|s| s.registered)
            else {
                self.set_status(
                    "Der Speicherplatz ist leer – bitte zuerst ein Papier speichern.",
                    true,
                );
                return;
            };
            working.push(s.to_custom_media_entry(None));
            working.len() - 1
        };
        let Some(entry) = working.get_mut(idx) else {
            return;
        };
        match assign_profile(entry, std::path::Path::new(&file)) {
            Ok(()) => {
                self.editor_icc_profile = entry.icc_profile.clone();
                let _ = save_custom_media_json(&self.custom_media_json_path, &working);
                self.set_status(
                    format!(
                        "Farbprofil '{}' Platz {slot:02} zugewiesen. Jetzt „Für das System installieren“.",
                        self.editor_icc_profile
                    ),
                    false,
                );
            }
            Err(e) => self.set_status(format!("Farbprofil nicht übernommen: {e}"), true),
        }
    }

    fn export_media_dialog(&mut self) {
        let Some(file) =
            native_file_dialog("Papiere exportieren", "epmi-Papierarchiv", "*.epmi", true)
        else {
            return;
        };
        let path = if file.ends_with(".epmi") {
            file
        } else {
            format!("{file}.epmi")
        };
        let entries = entries_for_slots(&self.slots, &self.working_copy());
        match export_media(&entries, std::path::Path::new(&path)) {
            Ok(()) => self.set_status(
                format!(
                    "{} Papier(e) mit Farbprofilen exportiert: {path}",
                    entries.len()
                ),
                false,
            ),
            Err(e) => self.set_status(format!("Export fehlgeschlagen: {e}"), true),
        }
    }

    fn import_media_dialog(&mut self) {
        let Some(file) =
            native_file_dialog("Papiere importieren", "epmi-Papierarchiv", "*.epmi", false)
        else {
            return;
        };
        match import_media(std::path::Path::new(&file)) {
            Ok(imported) => {
                let count = imported.len();
                let mut working = self.working_copy();
                merge_imported(&mut working, imported);
                let _ = save_custom_media_json(&self.custom_media_json_path, &working);
                self.select_slot(self.selected_slot);
                self.set_status(
                    format!("{count} Papier(e) importiert. Jetzt „Für das System installieren“."),
                    false,
                );
            }
            Err(e) => self.set_status(format!("Import fehlgeschlagen: {e}"), true),
        }
    }

    /// Helper widget to select the target slot (`1..30`) for 1-click installations.
    fn draw_target_slot_selector(&mut self, ui: &mut Ui, id_salt: &str) {
        let t = self.tokens;
        ui.label(RichText::new("Ziel-Speicherplatz:").color(t.text_dim));
        let cur = self.install_target_slot.clamp(1, 30);
        let cur_status = self
            .slots
            .get(usize::from(cur - 1))
            .filter(|s| s.registered)
            .map(|s| format!("Platz {:02} (Belegt: {})", cur, s.media_name))
            .unwrap_or_else(|| format!("Platz {:02} (Frei)", cur));

        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(cur_status)
            .width(230.0)
            .show_ui(ui, |ui| {
                for idx in 1..=30_u8 {
                    let s_lbl = self
                        .slots
                        .get(usize::from(idx - 1))
                        .filter(|s| s.registered)
                        .map(|s| format!("Platz {:02}: ● {}", idx, s.media_name))
                        .unwrap_or_else(|| format!("Platz {:02}: ○ — Frei —", idx));
                    if ui
                        .selectable_value(&mut self.install_target_slot, idx, s_lbl)
                        .clicked()
                    {
                        self.selected_slot = idx;
                    }
                }
            });

        let next_free = self.next_free_slot();
        if self
            .slots
            .get(usize::from(cur - 1))
            .is_some_and(|s| s.registered)
            && ui
                .button(format!("✨ Freier Platz {:02}", next_free))
                .on_hover_text("Wählt automatisch den nächsten freien Speicherplatz im Drucker")
                .clicked()
        {
            self.install_target_slot = next_free;
            self.selected_slot = next_free;
        }
    }

    // -----------------------------------------------------------------------
    // UI Panels (LumaPrint / PhotoCraft Layout)
    // -----------------------------------------------------------------------

    fn draw_top_bars(&mut self, ctx: &Context) {
        let t = self.tokens;
        let top_frame = egui::Frame::new()
            .fill(t.chrome)
            .inner_margin(Margin::symmetric(14, 6))
            .stroke(Stroke::new(1.0_f32, t.separator));

        egui::TopBottomPanel::top("top_chrome_bar")
            .frame(top_frame)
            .show(ctx, |ui| {
                // Row 1: Classic Menu Bar
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("Datei", |ui| {
                        if ui.button("📂 Lokale .emx-Datei öffnen…").clicked() {
                            self.active_view = WorkspaceView::EmxPackages;
                            if let Some(picked) = open_native_emx_file_dialog() {
                                self.emx_input_path = picked.clone();
                                if let Ok(pkg) = parse_emx_file(&picked) {
                                    self.loaded_emx = Some(pkg);
                                }
                            }
                            ui.close_menu();
                        }
                        if ui
                            .button("💾 Mit Druckertreiber & LumaPrint synchronisieren")
                            .clicked()
                        {
                            self.sync_slots_to_driver();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Beenden").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    ui.menu_button("Drucker", |ui| {
                        if ui
                            .button("🔍 Epson-Drucker in CUPS & Netzwerk suchen")
                            .clicked()
                        {
                            self.refresh_printer_discovery(&[]);
                            ui.close_menu();
                        }
                        if ui
                            .button("🔄 Alle 30 Speicherplätze vom Drucker neu laden")
                            .clicked()
                        {
                            self.poll_printer_slots();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(format!(
                                "💾 Aktuellen Platz {:02} im Drucker speichern",
                                self.selected_slot
                            ))
                            .clicked()
                        {
                            self.write_selected_slot_to_printer();
                            ui.close_menu();
                        }
                        if ui
                            .button(format!(
                                "🗑 Aktuellen Platz {:02} im Drucker leeren",
                                self.selected_slot
                            ))
                            .clicked()
                        {
                            self.clear_selected_slot_on_printer();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button("Einstellungen", |ui| {
                        ui.checkbox(
                            &mut self.show_right_dock,
                            "Rechtes Papier-Einstellungs-Dock anzeigen",
                        );
                        ui.checkbox(
                            &mut self.show_empty_slots,
                            "Auch freie Speicherplätze (1..30) in der Tabelle zeigen",
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("Farbschema (PhotoCraft & KDE Themes):")
                                .strong()
                                .color(t.text_dim),
                        );
                        for &kind in ThemeKind::ALL {
                            if ui
                                .radio_value(&mut self.theme_kind, kind, kind.label())
                                .clicked()
                            {
                                self.tokens = self.theme_kind.tokens();
                            }
                        }
                        ui.separator();
                        ui.label(
                            RichText::new("UI-Skalierung:")
                                .strong()
                                .color(t.text_dim),
                        );
                        ui.horizontal(|ui| {
                            for (scale, lbl) in [
                                (0.90_f32, "90%"),
                                (1.00_f32, "100%"),
                                (1.10_f32, "110%"),
                                (1.25_f32, "125%"),
                            ] {
                                if ui
                                    .selectable_label((self.ui_scale - scale).abs() < 0.02, lbl)
                                    .clicked()
                                {
                                    self.ui_scale = scale;
                                }
                            }
                        });
                    });

                    ui.menu_button("Hilfe", |ui| {
                        if ui.button("ℹ Über epmi…").clicked() {
                            self.show_about_modal = true;
                            ui.close_menu();
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);

                // Row 2: Action Toolbar (Epson Printer Discovery & Workspace Switcher)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    ui.label(RichText::new("Epson-Drucker:").color(t.text_dim));

                    let selected_printer_text = if self.manual_host_mode {
                        String::from("⌨ Manuelle Adresse…")
                    } else if let Some(idx) = self.selected_printer_idx
                        && let Some(p) = self.discovered_printers.get(idx)
                    {
                        p.display_label()
                    } else {
                        String::from("— Kein Epson-Drucker gefunden —")
                    };

                    let mut newly_selected_idx: Option<usize> = None;
                    egui::ComboBox::from_id_salt("toolbar_epson_printer_selector")
                        .selected_text(selected_printer_text)
                        .width(310.0)
                        .show_ui(ui, |ui| {
                            for (idx, p) in self.discovered_printers.iter().enumerate() {
                                let is_sel =
                                    !self.manual_host_mode && self.selected_printer_idx == Some(idx);
                                if ui.selectable_label(is_sel, p.display_label()).clicked() {
                                    newly_selected_idx = Some(idx);
                                }
                            }
                            if !self.discovered_printers.is_empty() {
                                ui.separator();
                            }
                            if ui
                                .selectable_label(
                                    self.manual_host_mode,
                                    "⌨ Manuelle IP / Hostname prüfen…",
                                )
                                .clicked()
                            {
                                self.manual_host_mode = true;
                            }
                        });

                    if let Some(idx) = newly_selected_idx
                        && let Some(p) = self.discovered_printers.get(idx).cloned()
                    {
                        self.selected_printer_idx = Some(idx);
                        self.manual_host_mode = false;
                        self.apply_selected_printer(&p);
                    }

                    if self.manual_host_mode {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.manual_host_input)
                                .desired_width(115.0)
                                .hint_text("IP / Hostname"),
                        );
                        if ui
                            .button("Verbinden")
                            .on_hover_text(
                                "Prüft per SNMP, ob es sich um einen Epson-Drucker handelt, und verbindet ihn",
                            )
                            .clicked()
                        {
                            self.verify_and_connect_manual_host();
                        }
                    }

                    if ui
                        .add(egui::Button::new("🔍 Suchen").min_size(Vec2::new(75.0, 28.0)))
                        .on_hover_text(
                            "Sucht automatisch in CUPS und im lokalen Netzwerk nach Epson-Druckern",
                        )
                        .clicked()
                    {
                        self.refresh_printer_discovery(&[]);
                    }

                    let poll_btn = egui::Button::new(
                        RichText::new("🔄 Aktualisieren")
                            .strong()
                            .color(t.accent_text),
                    )
                    .fill(t.accent)
                    .min_size(Vec2::new(115.0, 28.0));
                    if ui
                        .add(poll_btn)
                        .on_hover_text("Liest alle 30 Speicherplätze frisch vom Epson-Drucker ein")
                        .clicked()
                    {
                        self.poll_printer_slots();
                    }

                    ui.separator();

                    // Workspace View Switcher
                    for (view, label) in [
                        (WorkspaceView::PrinterSlots, "🖨 Speicherplätze (1..30)"),
                        (WorkspaceView::CloudCatalog, "☁ Epson Cloud-Katalog"),
                        (WorkspaceView::EmxPackages, "📦 Lokale .emx-Datei"),
                    ] {
                        let is_active = self.active_view == view;
                        let btn = egui::Button::new(
                            RichText::new(label).strong().color(if is_active {
                                t.accent_text
                            } else {
                                t.text
                            }),
                        )
                        .fill(if is_active { t.accent } else { t.card })
                        .min_size(Vec2::new(155.0, 28.0));
                        if ui.add(btn).clicked() {
                            self.active_view = view;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dock_btn = egui::Button::new(
                            RichText::new("⚙ Papier-Editor").color(if self.show_right_dock {
                                t.accent
                            } else {
                                t.text_dim
                            }),
                        );
                        if ui
                            .add(dock_btn)
                            .on_hover_text("Rechtes Papier-Einstellungs-Dock ein-/ausblenden")
                            .clicked()
                        {
                            self.show_right_dock = !self.show_right_dock;
                        }
                    });
                });
            });
    }

    fn draw_bottom_status_bar(&mut self, ctx: &Context) {
        let t = self.tokens;
        let bottom_frame = egui::Frame::new()
            .fill(t.chrome)
            .inner_margin(Margin::symmetric(16, 8))
            .stroke(Stroke::new(1.0_f32, t.separator));

        egui::TopBottomPanel::bottom("bottom_status_bar")
            .frame(bottom_frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let occupied = self.slots.iter().filter(|s| s.registered).count();
                    let badge_col = if occupied > 0 { t.success } else { t.text_dim };
                    ui.label(
                        RichText::new(format!("● {occupied}/30 Plätze belegt"))
                            .strong()
                            .color(badge_col),
                    );
                    ui.separator();

                    let status_col = if self.status_is_error {
                        t.danger
                    } else {
                        t.text_dim
                    };
                    ui.label(RichText::new(&self.status_msg).color(status_col));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let write_btn = egui::Button::new(
                            RichText::new(format!(
                                "💾 Platz {:02} auf Drucker & System speichern",
                                self.selected_slot
                            ))
                            .strong()
                            .color(t.accent_text),
                        )
                        .fill(t.accent)
                        .min_size(Vec2::new(250.0, 32.0));

                        if ui
                            .add(write_btn)
                            .on_hover_text(
                                "Speichert das rechts eingestellte Papier im Drucker und aktualisiert CUPS & LumaPrint",
                            )
                            .clicked()
                        {
                            self.write_selected_slot_to_printer();
                        }
                    });
                });
            });
    }

    fn draw_right_inspector_dock(&mut self, ctx: &Context) {
        if !self.show_right_dock {
            return;
        }
        let t = self.tokens;

        egui::SidePanel::right("right_slot_inspector_dock")
            .default_width(430.0)
            .min_width(395.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(t.dock)
                    .inner_margin(Margin::symmetric(16, 12))
                    .stroke(Stroke::new(1.0_f32, t.separator)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "★ Papier-Einstellungen — Platz {:02}",
                                    self.selected_slot
                                ))
                                .strong()
                                .color(t.accent),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let is_reg = self.editor_slot.registered;
                                    let (lbl, col) = if is_reg {
                                        ("BELEGT", t.success)
                                    } else {
                                        ("FREI", t.text_faint)
                                    };
                                    ui.label(RichText::new(lbl).small().strong().color(col));
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // Section 1: Speicherplatz & Papiername
                        self.draw_dock_section(
                            ui,
                            "1. Speicherplatz & Papiername",
                            true,
                            |app, ui| {
                                app.draw_dock_identity_section(ui);
                            },
                        );

                        // Section 2: Mechanische Papiereinstellungen
                        self.draw_dock_section(
                            ui,
                            "2. Mechanische Einstellungen",
                            true,
                            |app, ui| {
                                app.draw_dock_mechanics_section(ui);
                            },
                        );

                        // Section 3: Farbprofil (ICC) & Tintenauftrag
                        self.draw_dock_section(
                            ui,
                            "3. Farbprofil (ICC) & Tintenauftrag",
                            true,
                            |app, ui| {
                                app.draw_dock_driver_section(ui);
                            },
                        );

                        // Section 4: Speichern / Löschen
                        self.draw_dock_section(
                            ui,
                            "4. Aktionen (Drucker & Treiber)",
                            true,
                            |app, ui| {
                                app.draw_dock_actions_section(ui);
                            },
                        );

                        // Section 5: Technische Details (Expertenansicht, standardmäßig eingeklappt)
                        self.draw_dock_section(
                            ui,
                            "5. Technische Details (Expertenansicht)",
                            false,
                            |app, ui| {
                                app.draw_dock_expert_section(ui);
                            },
                        );
                    });
            });
    }

    fn draw_dock_section(
        &mut self,
        ui: &mut Ui,
        title: &str,
        default_open: bool,
        add_body: impl FnOnce(&mut Self, &mut Ui),
    ) {
        let t = self.tokens;
        ui.add_space(4.0);
        egui::CollapsingHeader::new(RichText::new(title).strong().color(t.text))
            .default_open(default_open)
            .show_unindented(ui, |ui| {
                ui.add_space(8.0);
                add_body(self, ui);
            });
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
    }

    fn draw_dock_identity_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        let current_slot = self.selected_slot;

        egui::Grid::new("dock_grid_identity")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .min_col_width(135.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Speicherplatz:").color(t.text_dim));
                let mut new_sel = current_slot;
                let cur_name = self
                    .slots
                    .get(usize::from(current_slot - 1))
                    .filter(|s| s.registered)
                    .map(|s| s.media_name.as_str())
                    .unwrap_or("Frei");
                egui::ComboBox::from_id_salt("dock_slot_selector")
                    .selected_text(format!("Platz {:02} ({cur_name})", current_slot))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for idx in 1..=30_u8 {
                            let s_name = self
                                .slots
                                .get(usize::from(idx - 1))
                                .filter(|s| s.registered)
                                .map(|s| s.media_name.as_str())
                                .unwrap_or("— Frei —");
                            let lbl = format!("Platz {:02}: {}", idx, s_name);
                            let _ = ui.selectable_value(&mut new_sel, idx, lbl);
                        }
                    });
                ui.end_row();
                if new_sel != current_slot {
                    self.select_slot(new_sel);
                }

                ui.label(RichText::new("Name am Display:").color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut self.editor_slot.media_name)
                        .desired_width(230.0)
                        .hint_text("z. B. Hahnemühle Photo Rag 308"),
                );
                ui.end_row();

                ui.label(RichText::new("Basis-Papiertyp:").color(t.text_dim));
                let cur_base = find_base_media_by_sn_id(self.editor_slot.base_sn_id);
                let cur_base_lbl = cur_base.map_or_else(
                    || format!("Benutzerdefiniert ({})", self.editor_slot.base_sn_id),
                    |b| {
                        let ink = if b.is_matte {
                            "Matt · MK"
                        } else {
                            "Foto · PK"
                        };
                        format!("{} ({})", b.name, ink)
                    },
                );
                egui::ComboBox::from_id_salt("dock_base_sn_combo")
                    .selected_text(cur_base_lbl)
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for preset in BASE_MEDIA_SPECS {
                            let ink = if preset.is_matte {
                                "Matt · MK"
                            } else {
                                "Foto · PK"
                            };
                            let lbl = format!("{} ({})", preset.name, ink);
                            if ui
                                .selectable_value(
                                    &mut self.editor_slot.base_sn_id,
                                    preset.sn_id,
                                    lbl,
                                )
                                .clicked()
                            {
                                self.editor_slot.media_category = preset.default_category;
                                self.editor_slot.platen_gap = preset.default_platen_gap;
                                self.editor_slot.thickness_01mm = preset.default_thickness_01mm;
                                self.editor_icc_profile = preset.icc_profile.to_string();
                            }
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Schwarztinte:").color(t.text_dim));
                let uses_mk = cur_base.is_some_and(|b| b.is_matte);
                let (ink_txt, ink_col) = if uses_mk {
                    ("Matte Black (MK) — Fine Art / Matt", t.warning)
                } else {
                    ("Photo Black (PK) — Foto / Baryt", t.accent)
                };
                ui.label(RichText::new(ink_txt).strong().color(ink_col));
                ui.end_row();

                ui.label(RichText::new("Papierkategorie:").color(t.text_dim));
                egui::ComboBox::from_id_salt("dock_category_combo")
                    .selected_text(media_category_label(self.editor_slot.media_category))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for cat_code in [1_u8, 2, 3, 4, 5, 6, 254] {
                            ui.selectable_value(
                                &mut self.editor_slot.media_category,
                                cat_code,
                                media_category_label(cat_code),
                            );
                        }
                    });
                ui.end_row();
            });
    }

    fn draw_dock_mechanics_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;

        egui::Grid::new("dock_grid_mechanics")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .min_col_width(135.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Walzenabstand:").color(t.text_dim));
                egui::ComboBox::from_id_salt("dock_platen_gap_combo")
                    .selected_text(platen_gap_label(self.editor_slot.platen_gap))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for gap_code in [9_u8, 12, 15, 21, 24] {
                            ui.selectable_value(
                                &mut self.editor_slot.platen_gap,
                                gap_code,
                                platen_gap_label(gap_code),
                            );
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Papierdicke:").color(t.text_dim));
                let mut mm = self.editor_slot.thickness_mm();
                if ui
                    .add(
                        egui::Slider::new(&mut mm, 0.08..=1.50)
                            .suffix(" mm")
                            .fixed_decimals(2),
                    )
                    .changed()
                {
                    self.editor_slot.thickness_01mm = (mm * 100.0).round() as u16;
                }
                ui.end_row();

                ui.label(RichText::new("Trocknungszeit:").color(t.text_dim));
                ui.add(egui::Slider::new(&mut self.editor_slot.drying_time, 0..=100).suffix(" %"));
                ui.end_row();

                ui.label(RichText::new("Vorschub (Blatt):").color(t.text_dim));
                ui.add(
                    egui::Slider::new(&mut self.editor_slot.cut_sheet_feed_offset, -50..=50)
                        .suffix(" %"),
                );
                ui.end_row();

                ui.label(RichText::new("Vorschub (Rolle):").color(t.text_dim));
                ui.add(
                    egui::Slider::new(&mut self.editor_slot.roll_paper_feed_offset, -50..=50)
                        .suffix(" %"),
                );
                ui.end_row();

                ui.label(RichText::new("Breitenprüfung:").color(t.text_dim));
                let mut check_on = self.editor_slot.paper_width_check != 0;
                if ui
                    .checkbox(&mut check_on, "Papierbreite vor dem Drucken prüfen")
                    .changed()
                {
                    self.editor_slot.paper_width_check = u8::from(check_on);
                }
                ui.end_row();
            });
    }

    fn draw_dock_driver_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;

        egui::Grid::new("dock_grid_driver")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .min_col_width(135.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Farbprofil (.icc):").color(t.text_dim));
                let mut choose = false;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.editor_icc_profile)
                            .desired_width(196.0)
                            .hint_text("z. B. SureColor_P900_FineArtBaryta_v1.icc"),
                    );
                    choose = ui
                        .button("📂")
                        .on_hover_text("Farbprofil-Datei für dieses Papier auswählen")
                        .clicked();
                });
                if choose {
                    self.choose_icc_for_selected_slot();
                }
                ui.end_row();

                ui.label(RichText::new("Tintenauftrag:").color(t.text_dim));
                ui.add(egui::Slider::new(&mut self.editor_print_duty, -50..=50).suffix(" %"));
                ui.end_row();
            });
    }

    fn draw_dock_actions_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        ui.vertical_centered_justified(|ui| {
            let write_btn = egui::Button::new(
                RichText::new(format!(
                    "💾 Platz {:02} auf Drucker & System speichern",
                    self.selected_slot
                ))
                .strong()
                .color(t.accent_text),
            )
            .fill(t.accent)
            .min_size(Vec2::new(0.0, 32.0));

            if ui.add(write_btn).clicked() {
                self.write_selected_slot_to_printer();
            }

            ui.add_space(4.0);

            let clear_btn = egui::Button::new(
                RichText::new(format!(
                    "🗑 Speicherplatz {:02} im Drucker leeren",
                    self.selected_slot
                ))
                .color(t.danger),
            )
            .min_size(Vec2::new(0.0, 28.0));

            if ui.add(clear_btn).clicked() {
                self.clear_selected_slot_on_printer();
            }
        });
    }

    fn draw_dock_expert_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        let preview_key = sanitize_ppd_media_key(self.selected_slot, &self.editor_slot.media_name);

        egui::Grid::new("dock_grid_expert")
            .num_columns(2)
            .spacing([14.0, 8.0])
            .min_col_width(135.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Medien-ID (32-Bit):").color(t.text_dim));
                ui.horizontal(|ui| {
                    let mut dec_id = self.editor_slot.user_media_id.to_string();
                    if ui
                        .add(egui::TextEdit::singleline(&mut dec_id).desired_width(110.0))
                        .changed()
                        && let Ok(parsed) = dec_id.trim().parse::<u32>()
                    {
                        self.editor_slot.user_media_id = parsed;
                    }
                    if ui
                        .button("Neu vergeben")
                        .on_hover_text("Erzeugt eine freie Benutzer-Medien-ID (910000001..)")
                        .clicked()
                    {
                        self.editor_slot.user_media_id =
                            910_000_000 + u32::from(self.selected_slot);
                    }
                });
                ui.end_row();

                ui.label(RichText::new("Treiber-Slot-ID:").color(t.text_dim));
                ui.label(
                    RichText::new(format!(
                        "{} (Basis-Code {})",
                        1900 + u16::from(self.selected_slot),
                        self.editor_slot.base_sn_id
                    ))
                    .monospace()
                    .color(t.text_dim),
                );
                ui.end_row();

                ui.label(RichText::new("CUPS PPD-Schlüssel:").color(t.text_dim));
                ui.label(
                    RichText::new(format!("{preview_key} ({}/32 Zeichen)", preview_key.len()))
                        .small()
                        .monospace()
                        .color(t.text_faint),
                );
                ui.end_row();
            });
    }

    // -----------------------------------------------------------------------
    // Central Workspace Views
    // -----------------------------------------------------------------------

    fn draw_central_workspace(&mut self, ctx: &Context) {
        let t = self.tokens;
        let central_frame = egui::Frame::central_panel(&ctx.style())
            .fill(t.canvas)
            .inner_margin(Margin::symmetric(18, 14));

        egui::CentralPanel::default()
            .frame(central_frame)
            .show(ctx, |ui| match self.active_view {
                WorkspaceView::PrinterSlots => self.draw_slots_workspace(ui),
                WorkspaceView::CloudCatalog => self.draw_cloud_workspace(ui),
                WorkspaceView::EmxPackages => self.draw_emx_workspace(ui),
            });
    }

    fn draw_slots_workspace(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        let existing_json = self.working_copy();
        let system_media = load_system_media();
        let mut action: Option<fn(&mut Self)> = None;

        ui.horizontal(|ui| {
            ui.heading(
                RichText::new(format!(
                    "Speicherplätze im Drucker (1..30) — {}",
                    self.printer_model_name
                ))
                .strong()
                .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let install_btn = egui::Button::new(
                    RichText::new("🖥 Für das System installieren")
                        .strong()
                        .color(t.accent_text),
                )
                .fill(t.accent);
                if ui
                    .add(install_btn)
                    .on_hover_text(
                        "Papierliste und Farbprofile für alle Benutzer, den CUPS-Druckertreiber und LumaPrint installieren (Administrator-Passwort). Profile von Epson-Cloud-Papieren werden automatisch geladen. Wie unter Windows/macOS nötig auf jedem Rechner, der druckt.",
                    )
                    .clicked()
                {
                    action = Some(Self::install_for_system);
                }
                if ui
                    .button("📥 Importieren…")
                    .on_hover_text("Papiere mit Farbprofilen von einem anderen Rechner übernehmen (.epmi)")
                    .clicked()
                {
                    action = Some(Self::import_media_dialog);
                }
                if ui
                    .button("📤 Exportieren…")
                    .on_hover_text("Papiere mit Farbprofilen für einen anderen Rechner sichern (.epmi)")
                    .clicked()
                {
                    action = Some(Self::export_media_dialog);
                }
                if ui
                    .button("☁ Neues Papier aus Cloud-Katalog hinzufügen…")
                    .clicked()
                {
                    self.active_view = WorkspaceView::CloudCatalog;
                }
                ui.checkbox(&mut self.show_empty_slots, "Freie Plätze anzeigen");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_filter)
                        .desired_width(180.0)
                        .hint_text("🔍 Papier suchen…"),
                );
            });
        });

        ui.add_space(8.0);

        let filter_lower = self.search_filter.trim().to_lowercase();
        let mut clicked_slot: Option<u8> = None;

        egui::Frame::new()
            .fill(t.card)
            .corner_radius(CornerRadius::same(t.radius))
            .inner_margin(Margin::symmetric(14, 12))
            .stroke(Stroke::new(1.0_f32, t.card_border))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("central_nvram_slots_grid")
                            .striped(true)
                            .num_columns(8)
                            .spacing([18.0, 10.0])
                            .show(ui, |ui| {
                                for hdr in [
                                    "Platz",
                                    "Papiername (Drucker-Display)",
                                    "Basis-Papiertyp",
                                    "Schwarztinte",
                                    "Papierdicke",
                                    "Walzenabstand",
                                    "Dieser Rechner",
                                    "Zugeordnetes Farbprofil (ICC)",
                                ] {
                                    ui.label(RichText::new(hdr).strong().color(t.text_dim));
                                }
                                ui.end_row();

                                for slot in &self.slots {
                                    let is_reg = slot.registered;
                                    if !self.show_empty_slots && !is_reg {
                                        continue;
                                    }
                                    if !filter_lower.is_empty()
                                        && !slot.media_name.to_lowercase().contains(&filter_lower)
                                        && !format!("{:02}", slot.slot).contains(&filter_lower)
                                    {
                                        continue;
                                    }

                                    let is_sel = self.selected_slot == slot.slot;
                                    let row_col = if is_sel {
                                        t.accent
                                    } else if is_reg {
                                        t.text
                                    } else {
                                        t.text_faint
                                    };

                                    if ui
                                        .selectable_label(
                                            is_sel,
                                            RichText::new(format!("Platz {:02}", slot.slot))
                                                .strong()
                                                .color(row_col),
                                        )
                                        .clicked()
                                    {
                                        clicked_slot = Some(slot.slot);
                                    }

                                    let name_display = if is_reg {
                                        format!("● {}", slot.media_name)
                                    } else {
                                        String::from("○ — Frei —")
                                    };
                                    if ui
                                        .selectable_label(
                                            is_sel,
                                            RichText::new(name_display).strong().color(if is_reg {
                                                t.text
                                            } else {
                                                t.text_faint
                                            }),
                                        )
                                        .clicked()
                                    {
                                        clicked_slot = Some(slot.slot);
                                    }

                                    let base_info = find_base_media_by_sn_id(slot.base_sn_id);
                                    let base_label = if is_reg {
                                        base_info.map_or_else(
                                            || format!("Code {}", slot.base_sn_id),
                                            |b| b.name.to_string(),
                                        )
                                    } else {
                                        String::from("—")
                                    };
                                    ui.label(RichText::new(base_label).color(t.text_dim));

                                    if is_reg {
                                        let uses_mk = base_info.is_some_and(|b| b.is_matte);
                                        let (ink_str, ink_c) = if uses_mk {
                                            ("MK (Matt)", t.warning)
                                        } else {
                                            ("PK (Foto)", t.accent)
                                        };
                                        ui.label(RichText::new(ink_str).strong().color(ink_c));
                                    } else {
                                        ui.label(RichText::new("—").color(t.text_faint));
                                    }

                                    let thick_str = if is_reg {
                                        format!("{:.2} mm", slot.thickness_mm())
                                    } else {
                                        String::from("—")
                                    };
                                    ui.label(RichText::new(thick_str).color(t.text_dim));

                                    let gap_str = if is_reg {
                                        platen_gap_label(slot.platen_gap)
                                    } else {
                                        "—"
                                    };
                                    ui.label(RichText::new(gap_str).color(t.text_dim));

                                    if is_reg {
                                        let entry = existing_json
                                            .iter()
                                            .find(|e| {
                                                e.slot == slot.slot
                                                    && e.user_media_id == slot.user_media_id
                                            })
                                            .cloned()
                                            .unwrap_or_else(|| slot.to_custom_media_entry(None));
                                        let state = system_state(&entry, &system_media);
                                        let state_col = match state {
                                            SystemState::Installed => t.success,
                                            SystemState::Changed => t.warning,
                                            SystemState::NotInstalled => t.danger,
                                        };
                                        ui.label(RichText::new(state.label_de()).color(state_col));
                                        let (icc_str, icc_col) = match resolve_profile(&entry) {
                                            ProfileSource::Own(p) => (
                                                p.file_name()
                                                    .map(|n| n.to_string_lossy().into_owned())
                                                    .unwrap_or_default(),
                                                t.text_dim,
                                            ),
                                            ProfileSource::BaseOnly(n) => {
                                                (format!("nur Basisprofil ({n})"), t.warning)
                                            }
                                            ProfileSource::Missing(n) => {
                                                (format!("fehlt: {n}"), t.danger)
                                            }
                                        };
                                        ui.label(
                                            RichText::new(icc_str)
                                                .small()
                                                .monospace()
                                                .color(icc_col),
                                        );
                                    } else {
                                        ui.label(RichText::new("—").color(t.text_faint));
                                        ui.label(RichText::new("—").color(t.text_faint));
                                    }

                                    ui.end_row();
                                }
                            });
                    });
            });

        if let Some(s) = clicked_slot {
            self.select_slot(s);
            self.show_right_dock = true;
        }
        if let Some(run) = action {
            run(self);
        }
    }

    fn draw_emx_workspace(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("Lokale .emx-Medienpakete öffnen & installieren")
                    .strong()
                    .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.draw_target_slot_selector(ui, "emx_target_slot_combo");
            });
        });
        ui.add_space(8.0);

        // Loader Card
        egui::Frame::new()
            .fill(t.card)
            .corner_radius(CornerRadius::same(t.radius))
            .inner_margin(Margin::symmetric(14, 12))
            .stroke(Stroke::new(1.0_f32, t.card_border))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Datei (.emx):").color(t.text_dim));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.emx_input_path)
                            .desired_width(420.0)
                            .hint_text("Pfad zu einer .emx-Datei wählen…"),
                    );
                    if ui
                        .button("📂 Durchsuchen…")
                        .on_hover_text("Öffnet den System-Dateidialog zur Auswahl einer .emx-Datei")
                        .clicked()
                        && let Some(picked) = open_native_emx_file_dialog()
                    {
                        self.emx_input_path = picked.clone();
                        match parse_emx_file(&picked) {
                            Ok(pkg) => {
                                self.set_status(
                                    format!("Medienpaket '{}' geladen.", pkg.media_name),
                                    false,
                                );
                                self.loaded_emx = Some(pkg);
                            }
                            Err(e) => {
                                self.set_status(format!("Fehler beim Lesen der .emx: {e}"), true);
                            }
                        }
                    }

                    let load_btn =
                        egui::Button::new(RichText::new("Laden").strong().color(t.accent_text))
                            .fill(t.accent);
                    if ui.add(load_btn).clicked() {
                        match parse_emx_file(self.emx_input_path.trim()) {
                            Ok(pkg) => {
                                self.set_status(
                                    format!("Medienpaket '{}' geladen.", pkg.media_name),
                                    false,
                                );
                                self.loaded_emx = Some(pkg);
                            }
                            Err(e) => {
                                self.set_status(format!("Fehler beim Lesen der .emx: {e}"), true);
                            }
                        }
                    }
                });
            });

        ui.add_space(12.0);

        if let Some(pkg) = self.loaded_emx.clone() {
            let target_slot = self.install_target_slot;
            egui::Frame::new()
                .fill(t.card)
                .corner_radius(CornerRadius::same(t.radius))
                .inner_margin(Margin::symmetric(16, 14))
                .stroke(Stroke::new(1.0_f32, t.card_border))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(
                            RichText::new(format!("📦 {} ({})", pkg.media_name, pkg.vendor))
                                .strong()
                                .color(t.accent),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let install_btn = egui::Button::new(
                                RichText::new(format!(
                                    "📲 Direkt auf Platz {:02} installieren",
                                    target_slot
                                ))
                                .strong()
                                .color(t.accent_text),
                            )
                            .fill(t.accent)
                            .min_size(Vec2::new(240.0, 32.0));

                            if ui
                                .add(install_btn)
                                .on_hover_text(
                                    "Extrahiert das ICC-Farbprofil, schreibt das Papier in den Drucker-Speicherplatz und aktualisiert CUPS & LumaPrint in einem Schritt",
                                )
                                .clicked()
                            {
                                let path = PathBuf::from(&pkg.source_path);
                                self.install_emx_one_click(&path, target_slot);
                            }

                            if ui
                                .button(format!("✏ Im Editor öffnen (Platz {:02})", target_slot))
                                .clicked()
                            {
                                self.apply_emx_to_editor(&pkg, target_slot);
                                self.show_right_dock = true;
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    egui::Grid::new("emx_details_grid")
                        .num_columns(2)
                        .spacing([24.0, 10.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Hersteller:").color(t.text_dim));
                            ui.label(RichText::new(&pkg.vendor).strong());
                            ui.end_row();

                            ui.label(RichText::new("Druckermodell:").color(t.text_dim));
                            ui.label(format!("{} ({})", pkg.model_name, pkg.series_name));
                            ui.end_row();

                            ui.label(RichText::new("Basis-Papiertyp:").color(t.text_dim));
                            let base_lbl = find_base_media_by_sn_id(pkg.base_sn_id).map_or_else(
                                || format!("Code {}", pkg.base_sn_id),
                                |b| b.name.to_string(),
                            );
                            ui.label(RichText::new(base_lbl).strong());
                            ui.end_row();

                            ui.label(RichText::new("Papierdicke:").color(t.text_dim));
                            ui.label(format!("{:.2} mm", pkg.thickness_mm()));
                            ui.end_row();

                            ui.label(RichText::new("Walzenabstand:").color(t.text_dim));
                            ui.label(platen_gap_label(pkg.platen_gap));
                            ui.end_row();

                            ui.label(RichText::new("Tintenauftrag:").color(t.text_dim));
                            ui.label(format!("{:+} %", pkg.print_duty()));
                            ui.end_row();

                            ui.label(RichText::new("Enthaltenes ICC-Farbprofil:").color(t.text_dim));
                            let icc_info = if !pkg.icc_profile_name.is_empty() {
                                format!(
                                    "{} ({:.1} KB)",
                                    pkg.icc_profile_name,
                                    pkg.icc_profile_bytes.len() as f64 / 1024.0
                                )
                            } else {
                                String::from("Kein eigenes ICC-Profil im Paket enthalten")
                            };
                            ui.label(RichText::new(icc_info).monospace().color(t.success));
                            ui.end_row();
                        });
                });
        }
    }

    fn draw_cloud_workspace(&mut self, ui: &mut Ui) {
        let t = self.tokens;

        // Top Bar Row 1: Model selector & Fetch Button
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("Online-Medienkatalog für Epson SureColor")
                    .strong()
                    .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fetch_btn = egui::Button::new(
                    RichText::new("☁ Katalog abrufen")
                        .strong()
                        .color(t.accent_text),
                )
                .fill(t.accent)
                .min_size(Vec2::new(145.0, 28.0));

                if ui.add(fetch_btn).clicked() {
                    let selected_label = CLOUD_PRINTER_MODELS
                        .iter()
                        .find(|m| m.code == self.cloud_model_code)
                        .map(|m| m.label)
                        .unwrap_or(self.cloud_model_code.as_str());
                    match cloud_list_media(&self.cloud_model_code, "DE", 8, "de") {
                        Ok(items) => {
                            let custom_count = items.iter().filter(|r| r.is_custom_emx()).count();
                            self.set_status(
                                format!(
                                    "{} Medienprofile ({custom_count} Zusatz-Medien + {} Standard-Medien) für {selected_label} geladen.",
                                    items.len(),
                                    items.len().saturating_sub(custom_count),
                                ),
                                false,
                            );
                            self.cloud_items = items;
                        }
                        Err(e) => {
                            self.set_status(format!("Cloud-Abruf fehlgeschlagen: {e}"), true);
                        }
                    }
                }

                let cur_model_label = CLOUD_PRINTER_MODELS
                    .iter()
                    .find(|m| m.code == self.cloud_model_code)
                    .map(|m| m.label)
                    .unwrap_or(self.cloud_model_code.as_str());

                egui::ComboBox::from_id_salt("cloud_model_all_27_combo")
                    .selected_text(cur_model_label)
                    .width(290.0)
                    .show_ui(ui, |ui| {
                        let mut last_cat = "";
                        for m in CLOUD_PRINTER_MODELS {
                            if m.category != last_cat {
                                if !last_cat.is_empty() {
                                    ui.separator();
                                }
                                ui.label(
                                    RichText::new(m.category)
                                        .small()
                                        .strong()
                                        .color(t.text_dim),
                                );
                                last_cat = m.category;
                            }
                            if ui
                                .selectable_label(self.cloud_model_code == m.code, m.label)
                                .clicked()
                            {
                                self.cloud_model_code = m.code.to_string();
                            }
                        }
                    });

                ui.label(RichText::new("Druckermodell:").color(t.text_dim));
            });
        });

        ui.add_space(6.0);

        // Top Bar Row 2: Target slot & search filter
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search_filter)
                    .desired_width(240.0)
                    .hint_text("🔍 Hersteller oder Papiername filtern…"),
            );
            ui.checkbox(
                &mut self.cloud_only_custom_emx,
                "Nur Zusatz-/Drittanbieter-Papiere zeigen",
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.draw_target_slot_selector(ui, "cloud_target_slot_combo");
            });
        });

        ui.add_space(8.0);

        let filter_lower = self.search_filter.trim().to_lowercase();
        let target_slot = self.install_target_slot;
        let mut install_direct_item: Option<CloudMediaRecord> = None;
        let mut edit_only_item: Option<CloudMediaRecord> = None;

        egui::Frame::new()
            .fill(t.card)
            .corner_radius(CornerRadius::same(t.radius))
            .inner_margin(Margin::symmetric(14, 12))
            .stroke(Stroke::new(1.0_f32, t.card_border))
            .show(ui, |ui| {
                if self.cloud_items.is_empty() {
                    ui.label(
                        RichText::new(
                            "Noch keine Katalogdaten geladen. Wähle oben rechts das gewünschte SureColor-Modell und klicke auf '☁ Katalog abrufen'.",
                        )
                        .color(t.text_dim),
                    );
                    return;
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("cloud_catalog_grid")
                            .striped(true)
                            .num_columns(6)
                            .spacing([16.0, 10.0])
                            .show(ui, |ui| {
                                for hdr in [
                                    "Hersteller",
                                    "Papiername",
                                    "Kategorie",
                                    "Paket-Typ",
                                    "Stand",
                                    "Installation",
                                ] {
                                    ui.label(RichText::new(hdr).strong().color(t.text_dim));
                                }
                                ui.end_row();

                                for item in &self.cloud_items {
                                    if self.cloud_only_custom_emx && !item.is_custom_emx() {
                                        continue;
                                    }
                                    if !filter_lower.is_empty()
                                        && !item
                                            .media_type_name
                                            .to_lowercase()
                                            .contains(&filter_lower)
                                        && !item
                                            .media_manufacturer_name
                                            .to_lowercase()
                                            .contains(&filter_lower)
                                        && !item.file_name.to_lowercase().contains(&filter_lower)
                                    {
                                        continue;
                                    }

                                    ui.label(RichText::new(&item.media_manufacturer_name).strong());
                                    ui.label(
                                        RichText::new(&item.media_type_name)
                                            .strong()
                                            .color(t.text),
                                    );
                                    ui.label(
                                        RichText::new(&item.media_group_name).color(t.text_dim),
                                    );
                                    let (kind_lbl, kind_col) = if item.is_custom_emx() {
                                        ("Zusatz-Profil (.emx)", t.success)
                                    } else {
                                        ("Standard-Profil (.emx)", t.text_dim)
                                    };
                                    ui.label(RichText::new(kind_lbl).small().color(kind_col));
                                    ui.label(
                                        RichText::new(format!(
                                            "{} ({})",
                                            item.updated, item.version
                                        ))
                                        .small()
                                        .color(t.text_faint),
                                    );
                                    if item.has_downloadable_emx() {
                                        ui.horizontal(|ui| {
                                            let inst_btn = egui::Button::new(
                                                RichText::new(format!(
                                                    "📲 Auf Platz {:02} installieren",
                                                    target_slot
                                                ))
                                                .strong()
                                                .color(t.accent_text),
                                            )
                                            .fill(t.accent);
                                            if ui
                                                .add(inst_btn)
                                                .on_hover_text(
                                                    "Lädt das Paket herunter, installiert das ICC-Farbprofil, überträgt es in den Drucker-Speicherplatz und aktualisiert CUPS & LumaPrint",
                                                )
                                                .clicked()
                                            {
                                                install_direct_item = Some(item.clone());
                                            }
                                            if ui
                                                .button("✏")
                                                .on_hover_text("Nur in den rechten Editor laden, ohne sofort in den Drucker zu schreiben")
                                                .clicked()
                                            {
                                                edit_only_item = Some(item.clone());
                                            }
                                        });
                                    } else {
                                        ui.label(RichText::new("Nicht verfügbar").color(t.text_faint));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });

        if let Some(item) = install_direct_item {
            let cache_dir = std::env::temp_dir().join("epmi-downloads");
            match cloud_download_emx(&item, &cache_dir) {
                Ok(path) => {
                    self.install_emx_one_click(&path, target_slot);
                }
                Err(e) => {
                    self.set_status(format!("Download fehlgeschlagen: {e}"), true);
                }
            }
        } else if let Some(item) = edit_only_item {
            let cache_dir = std::env::temp_dir().join("epmi-downloads");
            match cloud_download_emx(&item, &cache_dir).and_then(parse_emx_file) {
                Ok(pkg) => {
                    self.apply_emx_to_editor(&pkg, target_slot);
                    self.loaded_emx = Some(pkg);
                    self.show_right_dock = true;
                }
                Err(e) => {
                    self.set_status(format!("Download/Entpacken fehlgeschlagen: {e}"), true);
                }
            }
        }
    }

    fn draw_about_modal(&mut self, ctx: &Context) {
        if !self.show_about_modal {
            return;
        }
        let t = self.tokens;
        let mut open = self.show_about_modal;
        let mut close_clicked = false;

        egui::Window::new("Über epmi (ep-media-installer)")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(490.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.heading(
                    RichText::new("epmi — Media Management für Epson SureColor Drucker")
                        .strong()
                        .color(t.accent),
                );
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .small()
                        .color(t.text_dim),
                );

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    "epmi (ep-media-installer) verwaltet benutzerdefinierte Papiertypen \
                    und .emx-Medienpakete für Epson SureColor Fine-Art- und Großformatdrucker \
                    unter Linux:",
                );
                ui.add_space(6.0);
                ui.label(
                    "• Automatische Erkennung von Epson-Druckern über CUPS und das lokale Netzwerk\n\
                     • Auslesen, Bearbeiten und Beschreiben der 30 Speicherplätze (1..30) direkt im Drucker\n\
                     • Abrufen und 1-Klick-Installation von Medienprofilen aus dem Online-Katalog (alle 27 SureColor SC-P & SC-T Serien)\n\
                     • Automatische Synchronisation (custom_media.json & ICC) für den Linux-CUPS-Treiber und LumaPrint",
                );

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("📖 Handbuch:").strong().color(t.text_dim));
                    ui.label(
                        RichText::new("Handbuch-Link folgt in einer kommenden Version")
                            .italics()
                            .color(t.text_faint),
                    );
                });

                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Hinweis: Unabhängiges Open-Source-Werkzeug, steht in keiner Verbindung zur Seiko Epson Corp.",
                    )
                    .small()
                    .color(t.text_faint),
                );

                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Schließen").clicked() {
                        close_clicked = true;
                    }
                });
            });

        if close_clicked {
            open = false;
        }
        self.show_about_modal = open;
    }
}

impl eframe::App for EpmiGuiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        ctx.options_mut(|opt| {
            opt.zoom_with_keyboard = false;
        });
        ctx.set_zoom_factor(1.0);
        self.tokens = self.theme_kind.tokens();
        apply_theme_and_typography(ctx, self.theme_kind, &self.kde_typography, self.ui_scale);

        self.draw_top_bars(ctx);
        self.draw_bottom_status_bar(ctx);
        self.draw_right_inspector_dock(ctx);
        self.draw_central_workspace(ctx);
        self.draw_about_modal(ctx);
    }
}

fn main() -> eframe::Result {
    let mut host: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--host"
            && let Some(h) = args.next()
        {
            host = Some(h);
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("epmi — Media Installer für Epson SureColor")
            .with_app_id("epmi-gui")
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([1024.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "epmi — Media Installer für Epson SureColor",
        options,
        Box::new(move |cc| Ok(Box::new(EpmiGuiApp::new(cc, host)))),
    )
}
