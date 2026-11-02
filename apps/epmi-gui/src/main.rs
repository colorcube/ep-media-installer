//! `epmi-gui` — Native desktop GUI for Epson SureColor media management.
//!
//! Incorporates the LumaPrint / PhotoCraft UI architecture:
//! - Automatic KDE Plasma (`~/.config/kdeglobals`) font family & point-size adoption
//! - Wayland-safe fractional scaling (`pt * (96.0 / 72.0)` logical pixels + UI scale)
//! - 12 PhotoCraft themes (`BreezeDark`, `BreezeLight`, `Pro`, `Studio`, `Adwaita`, `SolarizedDark`, ...)
//! - Automatic Epson printer discovery (CUPS queues + SNMPv1 network scan)
//! - All 27 Epson SureColor models supported in the Cloud Catalog
//! - Intuitive 1-click installation workflow for `.emx` packages and Cloud profiles

#[macro_use]
mod i18n;
mod de;
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
    let kde_filter = format!("{label} ({patterns})|{}", tr!("All files (*)"));
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
        tr!("Select Epson media package (.emx)"),
        tr!("Epson media package"),
        "*.emx *.emy",
        false,
    )
}

/// Embedded 128×128 RGBA application icon (`share/icons/hicolor/128x128/apps/de.colorcube.epmi.png`).
const APP_ICON_PNG: &[u8] =
    include_bytes!("../../../share/icons/hicolor/128x128/apps/de.colorcube.epmi.png");

/// Decode the embedded application icon into [`egui::IconData`] for the window viewport.
fn app_icon_data() -> egui::IconData {
    let decoder = png::Decoder::new(std::io::Cursor::new(APP_ICON_PNG));
    if let Ok(mut reader) = decoder.read_info() {
        let buf_len = reader.output_buffer_size().unwrap_or(128 * 128 * 4);
        let mut buf = vec![0_u8; buf_len];
        if let Ok(info) = reader.next_frame(&mut buf) {
            buf.truncate(info.buffer_size());
            return egui::IconData {
                rgba: buf,
                width: info.width,
                height: info.height,
            };
        }
    }
    egui::IconData::default()
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
    /// Cached application icon texture for the About modal.
    app_icon_texture: Option<egui::TextureHandle>,

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
            printer_model_name: String::from(tr!("No Epson printer connected")),
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
            app_icon_texture: None,
            status_msg: String::from(tr!(
                "Searching for Epson printers in CUPS and on the network…"
            )),
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
                tr!("No Epson printer found automatically. Click '🔍 Search' or choose 'Manual address…'."),
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
                tr!("Epson printer '{model}' ({host}) selected. Click '🔄 Refresh' once the printer is switched on.", model = printer.model_name, host = printer.host),
                false,
            );
        }
    }

    fn verify_and_connect_manual_host(&mut self) {
        let candidate = self.manual_host_input.trim().to_string();
        if candidate.is_empty() {
            self.set_status(tr!("Please enter an IP address or host name."), true);
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
                    tr!("No supported Epson printer found via SNMP at '{candidate}' (only Epson printers are allowed).", candidate = candidate),
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
            self.set_status(tr!("No Epson printer selected."), true);
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
                    tr!("Connected to {model} ({host}): {registered} of 30 slots in the printer occupied.", model = self.printer_model_name, host = self.printer_host, registered = registered),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    tr!(
                        "Printer {model} ({host}) does not respond: {error}",
                        model = self.printer_model_name,
                        host = self.printer_host,
                        error = e
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
            tr!("{count} papers taken over into the working copy ({files}). For the printer driver & LumaPrint: “Install for this computer”.", count = updated.len(), files = written.join(", ")),
            false,
        );
    }

    fn write_selected_slot_to_printer(&mut self) {
        if self.printer_host.trim().is_empty() {
            self.set_status(
                tr!("Please select an Epson printer at the top first."),
                true,
            );
            return;
        }
        if self.editor_slot.media_name.trim().is_empty() {
            self.set_status(tr!("Please enter a paper name first."), true);
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
                    tr!("Slot {slot} ('{name}') saved in the printer. For the printer driver & LumaPrint: “Install for this computer”.", slot = format!("{:02}", self.selected_slot), name = verified.media_name),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    tr!(
                        "Error saving slot {slot}: {error}",
                        slot = format!("{:02}", self.selected_slot),
                        error = e
                    ),
                    true,
                );
            }
        }
    }

    fn clear_selected_slot_on_printer(&mut self) {
        if self.printer_host.trim().is_empty() {
            self.set_status(
                tr!("Please select an Epson printer at the top first."),
                true,
            );
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
                    tr!(
                        "Slot {slot} cleared in the printer, working copy updated.",
                        slot = format!("{:02}", self.selected_slot)
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    tr!(
                        "Error clearing slot {slot}: {error}",
                        slot = format!("{:02}", self.selected_slot),
                        error = e
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
            tr!("'{name}' loaded into the editor for slot {slot}. Click '💾 Save slot {slot} to printer' on the right to transfer it.", name = pkg.media_name, slot = format!("{clamped:02}")),
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
                    String::from(tr!("standard ICC"))
                } else {
                    tr!("ICC '{profile}' taken over", profile = entry.icc_profile)
                };
                let printer_note = if client_opt.is_some() {
                    tr!(
                        "transferred to printer slot {slot}",
                        slot = format!("{clamped:02}")
                    )
                } else {
                    tr!(
                        "saved locally for slot {slot}",
                        slot = format!("{clamped:02}")
                    )
                };
                self.set_status(
                    tr!("✅ '{name}' successfully {printer_note} ({icc_note}). For the printer driver & LumaPrint: “Install for this computer”.", name = pkg.media_name, printer_note = printer_note, icc_note = icc_note),
                    false,
                );
            }
            Err(e) => {
                self.set_status(tr!("Installation failed: {error}", error = e), true);
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
                tr!("No occupied slots – please read the printer first."),
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
                tr!(
                    " Profiles from the Epson cloud: {names}.",
                    names = done.join(", ")
                )
            }
            Ok(_) => String::new(),
            Err(e) => tr!(" (Epson cloud not reachable: {error})", error = e),
        };
        let _ = save_custom_media_json(&self.custom_media_json_path, &entries);
        match install_system_media(&entries) {
            Ok(_) => self.set_status(
                tr!("✅ {count} paper(s) installed for this computer – printer driver and LumaPrint are updated.{cloud_note}", count = entries.len(), cloud_note = cloud_note),
                false,
            ),
            Err(e) => self.set_status(tr!("Installation failed: {error}{cloud_note}", error = e, cloud_note = cloud_note), true),
        }
    }

    /// Assigns an ICC profile file to the selected slot in the working copy.
    fn choose_icc_for_selected_slot(&mut self) {
        let Some(file) = native_file_dialog(
            tr!("Select color profile (.icc) for the paper"),
            tr!("ICC profile"),
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
                self.set_status(tr!("The slot is empty – please save a paper first."), true);
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
                    tr!("Color profile '{profile}' assigned to slot {slot}. Now “Install for this computer”.", profile = self.editor_icc_profile, slot = format!("{slot:02}")),
                    false,
                );
            }
            Err(e) => self.set_status(
                tr!("Color profile not taken over: {error}", error = e),
                true,
            ),
        }
    }

    fn export_media_dialog(&mut self) {
        let Some(file) = native_file_dialog(
            tr!("Export papers"),
            tr!("epmi paper archive"),
            "*.epmi",
            true,
        ) else {
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
                tr!(
                    "{count} paper(s) with color profiles exported: {path}",
                    count = entries.len(),
                    path = path
                ),
                false,
            ),
            Err(e) => self.set_status(tr!("Export failed: {error}", error = e), true),
        }
    }

    fn import_media_dialog(&mut self) {
        let Some(file) = native_file_dialog(
            tr!("Import papers"),
            tr!("epmi paper archive"),
            "*.epmi",
            false,
        ) else {
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
                    tr!(
                        "{count} paper(s) imported. Now “Install for this computer”.",
                        count = count
                    ),
                    false,
                );
            }
            Err(e) => self.set_status(tr!("Import failed: {error}", error = e), true),
        }
    }

    /// Helper widget to select the target slot (`1..30`) for 1-click installations.
    fn draw_target_slot_selector(&mut self, ui: &mut Ui, id_salt: &str) {
        let t = self.tokens;
        ui.label(RichText::new(tr!("Target slot:")).color(t.text_dim));
        let cur = self.install_target_slot.clamp(1, 30);
        let cur_status = self
            .slots
            .get(usize::from(cur - 1))
            .filter(|s| s.registered)
            .map(|s| {
                tr!(
                    "Slot {slot} (occupied: {name})",
                    slot = format!("{cur:02}"),
                    name = s.media_name
                )
            })
            .unwrap_or_else(|| tr!("Slot {slot} (free)", slot = format!("{cur:02}")));

        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(cur_status)
            .width(230.0)
            .show_ui(ui, |ui| {
                for idx in 1..=30_u8 {
                    let s_lbl = self
                        .slots
                        .get(usize::from(idx - 1))
                        .filter(|s| s.registered)
                        .map(|s| {
                            tr!(
                                "Slot {slot}: ● {name}",
                                slot = format!("{idx:02}"),
                                name = s.media_name
                            )
                        })
                        .unwrap_or_else(|| {
                            tr!("Slot {slot}: ○ — free —", slot = format!("{idx:02}"))
                        });
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
                .button(tr!("✨ Free slot {slot}", slot = format!("{next_free:02}")))
                .on_hover_text(tr!(
                    "Selects the next free slot in the printer automatically"
                ))
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
                    ui.menu_button(tr!("File"), |ui| {
                        if ui.button(tr!("📂 Open local .emx file…")).clicked() {
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
                            .button(tr!("💾 Update working copy from printer"))
                            .clicked()
                        {
                            self.sync_slots_to_driver();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(tr!("Quit")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });

                    ui.menu_button(tr!("Printer"), |ui| {
                        if ui
                            .button(tr!("🔍 Search Epson printers in CUPS & network"))
                            .clicked()
                        {
                            self.refresh_printer_discovery(&[]);
                            ui.close_menu();
                        }
                        if ui
                            .button(tr!("🔄 Reload all 30 slots from the printer"))
                            .clicked()
                        {
                            self.poll_printer_slots();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(tr!(
                                "💾 Save current slot {slot} in the printer",
                                slot = format!("{:02}", self.selected_slot)
                            ))
                            .clicked()
                        {
                            self.write_selected_slot_to_printer();
                            ui.close_menu();
                        }
                        if ui
                            .button(tr!(
                                "🗑 Clear current slot {slot} in the printer",
                                slot = format!("{:02}", self.selected_slot)
                            ))
                            .clicked()
                        {
                            self.clear_selected_slot_on_printer();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button(tr!("Settings"), |ui| {
                        ui.checkbox(&mut self.show_right_dock, tr!("Show paper settings dock"));
                        ui.checkbox(
                            &mut self.show_empty_slots,
                            tr!("Also show free slots (1..30) in the table"),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(tr!("Color scheme (PhotoCraft & KDE themes):"))
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
                        ui.label(RichText::new(tr!("UI scale:")).strong().color(t.text_dim));
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
                        ui.separator();
                        ui.label(RichText::new(tr!("Language:")).strong().color(t.text_dim));
                        ui.horizontal(|ui| {
                            for lang in i18n::Language::ALL {
                                if ui
                                    .selectable_label(i18n::language() == lang, lang.label())
                                    .clicked()
                                {
                                    i18n::set_language(lang);
                                }
                            }
                        });
                    });

                    ui.menu_button(tr!("Help"), |ui| {
                        if ui.button(tr!("ℹ About epmi…")).clicked() {
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

                    ui.label(RichText::new(tr!("Epson printer:")).color(t.text_dim));

                    let selected_printer_text = if self.manual_host_mode {
                        String::from(tr!("⌨ Manual address…"))
                    } else if let Some(idx) = self.selected_printer_idx
                        && let Some(p) = self.discovered_printers.get(idx)
                    {
                        p.display_label()
                    } else {
                        String::from(tr!("— No Epson printer found —"))
                    };

                    let mut newly_selected_idx: Option<usize> = None;
                    egui::ComboBox::from_id_salt("toolbar_epson_printer_selector")
                        .selected_text(selected_printer_text)
                        .width(310.0)
                        .show_ui(ui, |ui| {
                            for (idx, p) in self.discovered_printers.iter().enumerate() {
                                let is_sel = !self.manual_host_mode
                                    && self.selected_printer_idx == Some(idx);
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
                                    tr!("⌨ Check manual IP / host name…"),
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
                                .hint_text(tr!("IP / host name")),
                        );
                        if ui
                            .button(tr!("Connect"))
                            .on_hover_text(tr!(
                                "Checks via SNMP that it is an Epson printer and connects it"
                            ))
                            .clicked()
                        {
                            self.verify_and_connect_manual_host();
                        }
                    }

                    if ui
                        .add(egui::Button::new(tr!("🔍 Search")).min_size(Vec2::new(75.0, 28.0)))
                        .on_hover_text(tr!(
                            "Searches CUPS and the local network for Epson printers"
                        ))
                        .clicked()
                    {
                        self.refresh_printer_discovery(&[]);
                    }

                    let poll_btn = egui::Button::new(
                        RichText::new(tr!("🔄 Refresh"))
                            .strong()
                            .color(t.accent_text),
                    )
                    .fill(t.accent)
                    .min_size(Vec2::new(115.0, 28.0));
                    if ui
                        .add(poll_btn)
                        .on_hover_text(tr!("Reads all 30 slots from the Epson printer again"))
                        .clicked()
                    {
                        self.poll_printer_slots();
                    }

                    ui.separator();

                    // Workspace View Switcher
                    for (view, label) in [
                        (WorkspaceView::PrinterSlots, tr!("🖨 Slots (1..30)")),
                        (WorkspaceView::CloudCatalog, tr!("☁ Epson cloud catalog")),
                        (WorkspaceView::EmxPackages, tr!("📦 Local .emx file")),
                    ] {
                        let is_active = self.active_view == view;
                        let btn =
                            egui::Button::new(RichText::new(label).strong().color(if is_active {
                                t.accent_text
                            } else {
                                t.text
                            }))
                            .fill(if is_active { t.accent } else { t.card })
                            .min_size(Vec2::new(155.0, 28.0));
                        if ui.add(btn).clicked() {
                            self.active_view = view;
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dock_btn = egui::Button::new(
                            RichText::new(tr!("⚙ Paper editor")).color(if self.show_right_dock {
                                t.accent
                            } else {
                                t.text_dim
                            }),
                        );
                        if ui
                            .add(dock_btn)
                            .on_hover_text(tr!("Show/hide paper settings dock"))
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
                        RichText::new(tr!("● {occupied}/30 slots occupied", occupied = occupied))
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
                            RichText::new(tr!("💾 Save slot {slot} to printer", slot = format!("{:02}", self.selected_slot)))
                            .strong()
                            .color(t.accent_text),
                        )
                        .fill(t.accent)
                        .min_size(Vec2::new(250.0, 32.0));

                        if ui
                            .add(write_btn)
                            .on_hover_text(
                                tr!("Saves the paper set on the right in the printer slot; then use “Install for this computer”"),
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
                                RichText::new(tr!(
                                    "★ Paper settings — slot {slot}",
                                    slot = format!("{:02}", self.selected_slot)
                                ))
                                .strong()
                                .color(t.accent),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let is_reg = self.editor_slot.registered;
                                    let (lbl, col) = if is_reg {
                                        (tr!("OCCUPIED"), t.success)
                                    } else {
                                        (tr!("FREE"), t.text_faint)
                                    };
                                    ui.label(RichText::new(lbl).small().strong().color(col));
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // Section 1: Speicherplatz & Papiername
                        self.draw_dock_section(ui, tr!("1. Slot & paper name"), true, |app, ui| {
                            app.draw_dock_identity_section(ui);
                        });

                        // Section 2: Mechanische Papiereinstellungen
                        self.draw_dock_section(
                            ui,
                            tr!("2. Mechanical settings"),
                            true,
                            |app, ui| {
                                app.draw_dock_mechanics_section(ui);
                            },
                        );

                        // Section 3: Farbprofil (ICC) & Tintenauftrag
                        self.draw_dock_section(
                            ui,
                            tr!("3. Color profile (ICC) & ink density"),
                            true,
                            |app, ui| {
                                app.draw_dock_driver_section(ui);
                            },
                        );

                        // Section 4: Speichern / Löschen
                        self.draw_dock_section(
                            ui,
                            tr!("4. Actions (printer & driver)"),
                            true,
                            |app, ui| {
                                app.draw_dock_actions_section(ui);
                            },
                        );

                        // Section 5: Technische Details (Expertenansicht, standardmäßig eingeklappt)
                        self.draw_dock_section(
                            ui,
                            tr!("5. Technical details (expert view)"),
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
                ui.label(RichText::new(tr!("Slot:")).color(t.text_dim));
                let mut new_sel = current_slot;
                let cur_name = self
                    .slots
                    .get(usize::from(current_slot - 1))
                    .filter(|s| s.registered)
                    .map(|s| s.media_name.as_str())
                    .unwrap_or(tr!("Free"));
                egui::ComboBox::from_id_salt("dock_slot_selector")
                    .selected_text(tr!(
                        "Slot {slot} ({name})",
                        slot = format!("{current_slot:02}"),
                        name = cur_name
                    ))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for idx in 1..=30_u8 {
                            let s_name = self
                                .slots
                                .get(usize::from(idx - 1))
                                .filter(|s| s.registered)
                                .map(|s| s.media_name.as_str())
                                .unwrap_or(tr!("— free —"));
                            let lbl = tr!(
                                "Slot {slot}: {name}",
                                slot = format!("{idx:02}"),
                                name = s_name
                            );
                            let _ = ui.selectable_value(&mut new_sel, idx, lbl);
                        }
                    });
                ui.end_row();
                if new_sel != current_slot {
                    self.select_slot(new_sel);
                }

                ui.label(RichText::new(tr!("Name on display:")).color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut self.editor_slot.media_name)
                        .desired_width(230.0)
                        .hint_text(tr!("e.g. Hahnemühle Photo Rag 308")),
                );
                ui.end_row();

                ui.label(RichText::new(tr!("Base paper type:")).color(t.text_dim));
                let cur_base = find_base_media_by_sn_id(self.editor_slot.base_sn_id);
                let cur_base_lbl = cur_base.map_or_else(
                    || tr!("Custom ({code})", code = self.editor_slot.base_sn_id),
                    |b| {
                        let ink = if b.is_matte {
                            tr!("Matte · MK")
                        } else {
                            tr!("Photo · PK")
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
                                tr!("Matte · MK")
                            } else {
                                tr!("Photo · PK")
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

                ui.label(RichText::new(tr!("Black ink:")).color(t.text_dim));
                let uses_mk = cur_base.is_some_and(|b| b.is_matte);
                let (ink_txt, ink_col) = if uses_mk {
                    (tr!("Matte Black (MK) — fine art / matte"), t.warning)
                } else {
                    (tr!("Photo Black (PK) — photo / baryta"), t.accent)
                };
                ui.label(RichText::new(ink_txt).strong().color(ink_col));
                ui.end_row();

                ui.label(RichText::new(tr!("Paper category:")).color(t.text_dim));
                egui::ComboBox::from_id_salt("dock_category_combo")
                    .selected_text(i18n::tr_dyn(media_category_label(
                        self.editor_slot.media_category,
                    )))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for cat_code in [1_u8, 2, 3, 4, 5, 6, 254] {
                            ui.selectable_value(
                                &mut self.editor_slot.media_category,
                                cat_code,
                                i18n::tr_dyn(media_category_label(cat_code)),
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
                ui.label(RichText::new(tr!("Platen gap:")).color(t.text_dim));
                egui::ComboBox::from_id_salt("dock_platen_gap_combo")
                    .selected_text(i18n::tr_dyn(platen_gap_label(self.editor_slot.platen_gap)))
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        for gap_code in [9_u8, 12, 15, 21, 24] {
                            ui.selectable_value(
                                &mut self.editor_slot.platen_gap,
                                gap_code,
                                i18n::tr_dyn(platen_gap_label(gap_code)),
                            );
                        }
                    });
                ui.end_row();

                ui.label(RichText::new(tr!("Paper thickness:")).color(t.text_dim));
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

                ui.label(RichText::new(tr!("Drying time:")).color(t.text_dim));
                ui.add(egui::Slider::new(&mut self.editor_slot.drying_time, 0..=100).suffix(" %"));
                ui.end_row();

                ui.label(RichText::new(tr!("Feed offset (sheet):")).color(t.text_dim));
                ui.add(
                    egui::Slider::new(&mut self.editor_slot.cut_sheet_feed_offset, -50..=50)
                        .suffix(" %"),
                );
                ui.end_row();

                ui.label(RichText::new(tr!("Feed offset (roll):")).color(t.text_dim));
                ui.add(
                    egui::Slider::new(&mut self.editor_slot.roll_paper_feed_offset, -50..=50)
                        .suffix(" %"),
                );
                ui.end_row();

                ui.label(RichText::new(tr!("Width check:")).color(t.text_dim));
                let mut check_on = self.editor_slot.paper_width_check != 0;
                if ui
                    .checkbox(&mut check_on, tr!("Check paper width before printing"))
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
                ui.label(RichText::new(tr!("Color profile (.icc):")).color(t.text_dim));
                let mut choose = false;
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.editor_icc_profile)
                            .desired_width(196.0)
                            .hint_text("z. B. SureColor_P900_FineArtBaryta_v1.icc"),
                    );
                    choose = ui
                        .button("📂")
                        .on_hover_text(tr!("Select color profile file for this paper"))
                        .clicked();
                });
                if choose {
                    self.choose_icc_for_selected_slot();
                }
                ui.end_row();

                ui.label(RichText::new(tr!("Ink density:")).color(t.text_dim));
                ui.add(egui::Slider::new(&mut self.editor_print_duty, -50..=50).suffix(" %"));
                ui.end_row();
            });
    }

    fn draw_dock_actions_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        ui.vertical_centered_justified(|ui| {
            let write_btn = egui::Button::new(
                RichText::new(tr!(
                    "💾 Save slot {slot} to printer",
                    slot = format!("{:02}", self.selected_slot)
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
                RichText::new(tr!(
                    "🗑 Clear slot {slot} in the printer",
                    slot = format!("{:02}", self.selected_slot)
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
                ui.label(RichText::new(tr!("Media ID (32-bit):")).color(t.text_dim));
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
                        .button(tr!("Assign new"))
                        .on_hover_text(tr!("Generates a free user media ID (910000001..)"))
                        .clicked()
                    {
                        self.editor_slot.user_media_id =
                            910_000_000 + u32::from(self.selected_slot);
                    }
                });
                ui.end_row();

                ui.label(RichText::new(tr!("Driver slot ID:")).color(t.text_dim));
                ui.label(
                    RichText::new(tr!(
                        "{id} (base code {code})",
                        id = 1900 + u16::from(self.selected_slot),
                        code = self.editor_slot.base_sn_id
                    ))
                    .monospace()
                    .color(t.text_dim),
                );
                ui.end_row();

                ui.label(RichText::new(tr!("CUPS PPD key:")).color(t.text_dim));
                ui.label(
                    RichText::new(tr!(
                        "{key} ({len}/32 characters)",
                        key = preview_key,
                        len = preview_key.len()
                    ))
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
                RichText::new(tr!("Slots in the printer (1..30) — {model}", model = self.printer_model_name))
                .strong()
                .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let install_btn = egui::Button::new(
                    RichText::new(tr!("🖥 Install for this computer"))
                        .strong()
                        .color(t.accent_text),
                )
                .fill(t.accent);
                if ui
                    .add(install_btn)
                    .on_hover_text(
                        tr!("Installs paper list and color profiles for all users, the CUPS printer driver and LumaPrint (administrator password). Profiles of Epson cloud papers are fetched automatically. As on Windows/macOS, this is needed on every computer that prints."),
                    )
                    .clicked()
                {
                    action = Some(Self::install_for_system);
                }
                if ui
                    .button(tr!("📥 Import…"))
                    .on_hover_text(tr!("Take over papers with color profiles from another computer (.epmi)"))
                    .clicked()
                {
                    action = Some(Self::import_media_dialog);
                }
                if ui
                    .button(tr!("📤 Export…"))
                    .on_hover_text(tr!("Save papers with color profiles for another computer (.epmi)"))
                    .clicked()
                {
                    action = Some(Self::export_media_dialog);
                }
                if ui
                    .button(tr!("☁ Add new paper from cloud catalog…"))
                    .clicked()
                {
                    self.active_view = WorkspaceView::CloudCatalog;
                }
                ui.checkbox(&mut self.show_empty_slots, tr!("Show free slots"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_filter)
                        .desired_width(180.0)
                        .hint_text(tr!("🔍 Search paper…")),
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
                                    tr!("Slot"),
                                    tr!("Paper name (printer display)"),
                                    tr!("Base paper type"),
                                    tr!("Black ink"),
                                    tr!("Paper thickness"),
                                    tr!("Platen gap"),
                                    tr!("This computer"),
                                    tr!("Assigned color profile (ICC)"),
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
                                            RichText::new(tr!(
                                                "Slot {slot}",
                                                slot = format!("{:02}", slot.slot)
                                            ))
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
                                        String::from(tr!("○ — free —"))
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
                                            || tr!("Code {code}", code = slot.base_sn_id),
                                            |b| b.name.to_string(),
                                        )
                                    } else {
                                        String::from("—")
                                    };
                                    ui.label(RichText::new(base_label).color(t.text_dim));

                                    if is_reg {
                                        let uses_mk = base_info.is_some_and(|b| b.is_matte);
                                        let (ink_str, ink_c) = if uses_mk {
                                            (tr!("MK (matte)"), t.warning)
                                        } else {
                                            (tr!("PK (photo)"), t.accent)
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
                                        i18n::tr_dyn(platen_gap_label(slot.platen_gap))
                                    } else {
                                        String::from("—")
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
                                        ui.label(
                                            RichText::new(i18n::tr_dyn(state.label()))
                                                .color(state_col),
                                        );
                                        let (icc_str, icc_col) = match resolve_profile(&entry) {
                                            ProfileSource::Own(p) => (
                                                p.file_name()
                                                    .map(|n| n.to_string_lossy().into_owned())
                                                    .unwrap_or_default(),
                                                t.text_dim,
                                            ),
                                            ProfileSource::BaseOnly(n) => (
                                                tr!("base profile only ({name})", name = n),
                                                t.warning,
                                            ),
                                            ProfileSource::Missing(n) => {
                                                (tr!("missing: {name}", name = n), t.danger)
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
                RichText::new(tr!("Open & install local .emx media packages"))
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
                    ui.label(RichText::new(tr!("File (.emx):")).color(t.text_dim));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.emx_input_path)
                            .desired_width(420.0)
                            .hint_text(tr!("Choose path to an .emx file…")),
                    );
                    if ui
                        .button(tr!("📂 Browse…"))
                        .on_hover_text(tr!("Opens the system file dialog to choose an .emx file"))
                        .clicked()
                        && let Some(picked) = open_native_emx_file_dialog()
                    {
                        self.emx_input_path = picked.clone();
                        match parse_emx_file(&picked) {
                            Ok(pkg) => {
                                self.set_status(
                                    tr!("Media package '{name}' loaded.", name = pkg.media_name),
                                    false,
                                );
                                self.loaded_emx = Some(pkg);
                            }
                            Err(e) => {
                                self.set_status(
                                    tr!("Error reading the .emx: {error}", error = e),
                                    true,
                                );
                            }
                        }
                    }

                    let load_btn =
                        egui::Button::new(RichText::new(tr!("Load")).strong().color(t.accent_text))
                            .fill(t.accent);
                    if ui.add(load_btn).clicked() {
                        match parse_emx_file(self.emx_input_path.trim()) {
                            Ok(pkg) => {
                                self.set_status(
                                    tr!("Media package '{name}' loaded.", name = pkg.media_name),
                                    false,
                                );
                                self.loaded_emx = Some(pkg);
                            }
                            Err(e) => {
                                self.set_status(
                                    tr!("Error reading the .emx: {error}", error = e),
                                    true,
                                );
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
                                RichText::new(tr!("📲 Install directly to slot {slot}", slot = format!("{target_slot:02}")))
                                .strong()
                                .color(t.accent_text),
                            )
                            .fill(t.accent)
                            .min_size(Vec2::new(240.0, 32.0));

                            if ui
                                .add(install_btn)
                                .on_hover_text(
                                    tr!("Extracts the ICC color profile and writes the paper into the printer slot in one step"),
                                )
                                .clicked()
                            {
                                let path = PathBuf::from(&pkg.source_path);
                                self.install_emx_one_click(&path, target_slot);
                            }

                            if ui
                                .button(tr!("✏ Open in editor (slot {slot})", slot = format!("{target_slot:02}")))
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
                            ui.label(RichText::new(tr!("Manufacturer:")).color(t.text_dim));
                            ui.label(RichText::new(&pkg.vendor).strong());
                            ui.end_row();

                            ui.label(RichText::new(tr!("Printer model:")).color(t.text_dim));
                            ui.label(format!("{} ({})", pkg.model_name, pkg.series_name));
                            ui.end_row();

                            ui.label(RichText::new(tr!("Base paper type:")).color(t.text_dim));
                            let base_lbl = find_base_media_by_sn_id(pkg.base_sn_id).map_or_else(
                                || tr!("Code {code}", code = pkg.base_sn_id),
                                |b| b.name.to_string(),
                            );
                            ui.label(RichText::new(base_lbl).strong());
                            ui.end_row();

                            ui.label(RichText::new(tr!("Paper thickness:")).color(t.text_dim));
                            ui.label(format!("{:.2} mm", pkg.thickness_mm()));
                            ui.end_row();

                            ui.label(RichText::new(tr!("Platen gap:")).color(t.text_dim));
                            ui.label(i18n::tr_dyn(platen_gap_label(pkg.platen_gap)));
                            ui.end_row();

                            ui.label(RichText::new(tr!("Ink density:")).color(t.text_dim));
                            ui.label(format!("{:+} %", pkg.print_duty()));
                            ui.end_row();

                            ui.label(RichText::new(tr!("Included ICC color profile:")).color(t.text_dim));
                            let icc_info = if !pkg.icc_profile_name.is_empty() {
                                format!(
                                    "{} ({:.1} KB)",
                                    pkg.icc_profile_name,
                                    pkg.icc_profile_bytes.len() as f64 / 1024.0
                                )
                            } else {
                                String::from(tr!("No own ICC profile in the package"))
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
                RichText::new(tr!("Online media catalog for Epson SureColor"))
                    .strong()
                    .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fetch_btn = egui::Button::new(
                    RichText::new(tr!("☁ Fetch catalog"))
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
                                tr!("{count} media profiles ({custom} add-on media + {standard} standard media) loaded for {model}.", count = items.len(), custom = custom_count, standard = items.len().saturating_sub(custom_count), model = selected_label),
                                false,
                            );
                            self.cloud_items = items;
                        }
                        Err(e) => {
                            self.set_status(tr!("Cloud request failed: {error}", error = e), true);
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

                ui.label(RichText::new(tr!("Printer model:")).color(t.text_dim));
            });
        });

        ui.add_space(6.0);

        // Top Bar Row 2: Target slot & search filter
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search_filter)
                    .desired_width(240.0)
                    .hint_text(tr!("🔍 Filter manufacturer or paper name…")),
            );
            ui.checkbox(
                &mut self.cloud_only_custom_emx,
                tr!("Show only add-on / third-party papers"),
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
                            tr!("No catalog data loaded yet. Choose the SureColor model at the top right and click '☁ Fetch catalog'."),
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
                                    tr!("Manufacturer"),
                                    tr!("Paper name"),
                                    tr!("Category"),
                                    tr!("Package type"),
                                    tr!("Date"),
                                    tr!("Installation"),
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
                                        (tr!("Add-on profile (.emx)"), t.success)
                                    } else {
                                        (tr!("Standard profile (.emx)"), t.text_dim)
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
                                                RichText::new(tr!("📲 Install to slot {slot}", slot = format!("{target_slot:02}")))
                                                .strong()
                                                .color(t.accent_text),
                                            )
                                            .fill(t.accent);
                                            if ui
                                                .add(inst_btn)
                                                .on_hover_text(
                                                    tr!("Downloads the package, extracts the ICC color profile and writes the paper into the printer slot"),
                                                )
                                                .clicked()
                                            {
                                                install_direct_item = Some(item.clone());
                                            }
                                            if ui
                                                .button("✏")
                                                .on_hover_text(tr!("Only load into the editor on the right, without writing to the printer"))
                                                .clicked()
                                            {
                                                edit_only_item = Some(item.clone());
                                            }
                                        });
                                    } else {
                                        ui.label(RichText::new(tr!("Not available")).color(t.text_faint));
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
                    self.set_status(tr!("Download failed: {error}", error = e), true);
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
                    self.set_status(tr!("Download/extraction failed: {error}", error = e), true);
                }
            }
        }
    }

    fn draw_about_modal(&mut self, ctx: &Context) {
        if !self.show_about_modal {
            return;
        }
        let icon_tex = self
            .app_icon_texture
            .get_or_insert_with(|| {
                let icon = app_icon_data();
                let w = usize::try_from(icon.width).unwrap_or(128);
                let h = usize::try_from(icon.height).unwrap_or(128);
                let img = if icon.rgba.len() == w.saturating_mul(h).saturating_mul(4) && w > 0 {
                    egui::ColorImage::from_rgba_unmultiplied([w, h], &icon.rgba)
                } else {
                    egui::ColorImage::new([1, 1], egui::Color32::TRANSPARENT)
                };
                ctx.load_texture("epmi_app_icon", img, egui::TextureOptions::LINEAR)
            })
            .clone();
        let t = self.tokens;
        let mut open = self.show_about_modal;
        let mut close_clicked = false;

        egui::Window::new(tr!("About epmi (ep-media-installer)"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(490.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.image((icon_tex.id(), Vec2::splat(48.0)));
                    ui.vertical(|ui| {
                        ui.heading(
                            RichText::new(tr!("epmi — media management for Epson SureColor printers"))
                                .strong()
                                .color(t.accent),
                        );
                        ui.label(
                            RichText::new(tr!(
                                "Version {version}",
                                version = env!("CARGO_PKG_VERSION")
                            ))
                            .small()
                            .color(t.text_dim),
                        );
                    });
                });

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
                    ui.label(RichText::new(tr!("📖 Manual:")).strong().color(t.text_dim));
                    ui.label(
                        RichText::new(tr!("Link to the manual follows in a later version"))
                            .italics()
                            .color(t.text_faint),
                    );
                });

                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        tr!("Note: independent open-source tool, not affiliated with Seiko Epson Corp."),
                    )
                    .small()
                    .color(t.text_faint),
                );

                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(tr!("Close")).clicked() {
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
    i18n::set_language(i18n::Language::from_env());
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
            .with_title(tr!("epmi — Media Installer for Epson SureColor"))
            .with_app_id("de.colorcube.epmi")
            .with_icon(app_icon_data())
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([1024.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        tr!("epmi — Media Installer for Epson SureColor"),
        options,
        Box::new(move |cc| Ok(Box::new(EpmiGuiApp::new(cc, host)))),
    )
}

#[cfg(test)]
mod tests {
    use super::app_icon_data;
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn embedded_app_icon_and_desktop_metadata_are_valid() {
        let icon = app_icon_data();
        assert_eq!(icon.width, 128);
        assert_eq!(icon.height, 128);
        assert_eq!(icon.rgba.len(), 128 * 128 * 4);

        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let desktop_file = root.join("share/applications/de.colorcube.epmi.desktop");
        let metainfo_file = root.join("share/metainfo/de.colorcube.epmi.metainfo.xml");
        let svg_icon = root.join("share/icons/hicolor/scalable/apps/de.colorcube.epmi.svg");
        assert!(desktop_file.is_file());
        assert!(metainfo_file.is_file());
        assert!(svg_icon.is_file());
        if let Ok(out) = Command::new("desktop-file-validate")
            .arg(&desktop_file)
            .output()
        {
            assert!(
                out.status.success(),
                "desktop-file-validate failed: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        if let Ok(out) = Command::new("appstreamcli")
            .args(["validate", "--no-net"])
            .arg(&metainfo_file)
            .output()
        {
            assert!(
                out.status.success(),
                "appstreamcli validate failed: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
