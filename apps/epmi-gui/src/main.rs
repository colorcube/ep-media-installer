//! `epmi-gui` — Native desktop GUI for Epson SureColor SC-P900 / SC-P700 media management.
//!
//! Incorporates the LumaPrint / PhotoCraft UI architecture:
//! - Automatic KDE Plasma (`~/.config/kdeglobals`) font family & point-size adoption
//! - Wayland-safe fractional scaling (`pt * (96.0 / 72.0)` logical pixels + UI scale)
//! - 12 PhotoCraft themes (`BreezeDark`, `BreezeLight`, `Pro`, `Studio`, `Adwaita`, `SolarizedDark`, ...)
//! - Clean Dock Layout:
//!   - Top Menu Bar & Action Toolbar
//!   - Central Workspace (Printer NVRAM Slot Grid/Table, Local `.emx` Packages, Epson Cloud Catalog)
//!   - Resizable Right Inspector Dock with structured section cards

mod theme;

use std::path::PathBuf;

use egui::{Context, CornerRadius, Margin, RichText, Stroke, Ui, Vec2};
use epmi_core::{
    BASE_MEDIA_SPECS, CloudMediaRecord, EmxPackage, PrinterMediaSlot,
    SnmpMediaClient, cloud_download_emx, cloud_list_media, default_icc_output_dirs,
    default_user_custom_media_json, discover_sync_targets, find_base_media_by_sn_id,
    load_custom_media_json, media_category_label, merge_slots_with_existing, parse_emx_file,
    platen_gap_label, sanitize_ppd_media_key, save_custom_media_json, try_regenerate_driver_ppds,
};
use theme::{
    KdeTypography, ThemeKind, Tokens, apply_theme_and_typography, install_kde_system_fonts,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    PrinterSlots,
    EmxPackages,
    CloudCatalog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CloudModelChoice {
    ScP900,
    ScP700,
}

impl CloudModelChoice {
    const fn code(self) -> &'static str {
        match self {
            Self::ScP900 => "scp9xx",
            Self::ScP700 => "scp7xx",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ScP900 => "SureColor SC-P900 (scp9xx)",
            Self::ScP700 => "SureColor SC-P700 (scp7xx)",
        }
    }
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
    /// Show/hide the right-hand slot inspector & NVRAM editor dock.
    show_right_dock: bool,

    /// Active central workspace view.
    active_view: WorkspaceView,
    /// Show all 30 slots or only occupied slots in the central view.
    show_empty_slots: bool,

    /// Printer IPv4 address.
    printer_host: String,
    /// All 30 NVRAM user slots (`1..=30`).
    slots: Vec<PrinterMediaSlot>,
    /// Currently selected slot (`1..=30`) in the Right Dock editor.
    selected_slot: u8,
    /// Editable working copy of the selected slot in the Right Dock.
    editor_slot: PrinterMediaSlot,
    /// Editable print duty (`-50..=50%`) for `custom_media.json` / `ESC p setq`.
    editor_print_duty: i8,
    /// Editable ICC profile filename for `custom_media.json` / `*cupsICCProfile`.
    editor_icc_profile: String,

    /// Local `.emx` file path input.
    emx_input_path: String,
    /// Currently loaded `.emx` package.
    loaded_emx: Option<EmxPackage>,

    /// Cloud catalog model selector (`SC-P900` vs `SC-P700`).
    cloud_model: CloudModelChoice,
    /// Fetched cloud catalog items.
    cloud_items: Vec<CloudMediaRecord>,
    /// Filter query for cloud catalog / slots.
    search_filter: String,

    /// Primary `custom_media.json` path.
    custom_media_json_path: PathBuf,

    /// Show/hide the "Über epmi" modal window.
    show_about_modal: bool,

    /// Status bar message & state.
    status_msg: String,
    status_is_error: bool,
}

impl EpmiGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_host: String) -> Self {
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
            printer_host: initial_host,
            slots,
            selected_slot: 1,
            editor_slot,
            editor_print_duty: 0,
            editor_icc_profile: String::from("Epson_SC-P900_700_VelvetFineArtPaper.icc"),
            emx_input_path: String::new(),
            loaded_emx: None,
            cloud_model: CloudModelChoice::ScP900,
            cloud_items: Vec::new(),
            search_filter: String::new(),
            custom_media_json_path,
            show_about_modal: false,
            status_msg: String::from(
                "Bereit. Klicke auf '🔄 Slots abfragen', um die 30 NVRAM-Slots des SC-P900 per SNMPv1 auszulesen.",
            ),
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
            app.set_status(
                format!(
                    "{} gespeicherte Custom-Media-Slots aus {} vorgeladen.",
                    existing.len(),
                    app.custom_media_json_path.display()
                ),
                false,
            );
        }

        app
    }

    fn set_status(&mut self, msg: impl Into<String>, is_error: bool) {
        self.status_msg = msg.into();
        self.status_is_error = is_error;
    }

    fn select_slot(&mut self, slot_num: u8) {
        let clamped = slot_num.clamp(1, 30);
        self.selected_slot = clamped;
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
        let client = SnmpMediaClient::new(self.printer_host.trim());
        match client.read_all_slots(false) {
            Ok(read_slots) => {
                let registered = read_slots.iter().filter(|s| s.registered).count();
                self.slots = read_slots;
                self.select_slot(self.selected_slot);
                self.set_status(
                    format!(
                        "SNMP-Abfrage erfolgreich ({}): {registered} von 30 Slots im Drucker belegt.",
                        self.printer_host
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    format!("SNMP-Fehler bei {}: {e}", self.printer_host),
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
        let ppd_msg = try_regenerate_driver_ppds()
            .map(|_| " + PPD aktualisiert")
            .unwrap_or("");

        self.set_status(
            format!(
                "{} aktive Slots synchronisiert nach [{}]{ppd_msg}",
                updated.len(),
                written.join(", ")
            ),
            false,
        );
    }

    fn write_selected_slot_to_printer(&mut self) {
        if self.editor_slot.media_name.trim().is_empty() {
            self.set_status("Bitte zuerst einen Mediennamen eingeben.", true);
            return;
        }
        if self.editor_slot.user_media_id == 0 {
            self.editor_slot.user_media_id = 0xC000_0000 | u32::from(self.selected_slot);
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

                // Also update custom_media.json with the custom ICC & print_duty
                let targets = discover_sync_targets(Some(&self.custom_media_json_path));
                let existing = load_custom_media_json(&self.custom_media_json_path);
                let mut entry = verified.to_custom_media_entry(
                    existing.iter().find(|e| e.slot == self.selected_slot),
                );
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
                let _ = try_regenerate_driver_ppds();

                self.set_status(
                    format!(
                        "Slot {:02} ('{}', MA_ID 0x{:08X}) erfolgreich in den Drucker-NVRAM geschrieben!",
                        self.selected_slot, verified.media_name, verified.user_media_id
                    ),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    format!("Fehler beim Schreiben von Slot {:02}: {e}", self.selected_slot),
                    true,
                );
            }
        }
    }

    fn clear_selected_slot_on_printer(&mut self) {
        let client = SnmpMediaClient::new(self.printer_host.trim());
        match client.clear_slot(self.selected_slot) {
            Ok(cleared) => {
                if let Some(slot_ref) = self.slots.get_mut(usize::from(self.selected_slot - 1)) {
                    *slot_ref = cleared.clone();
                }
                self.editor_slot = cleared;
                self.set_status(
                    format!("Slot {:02} im Drucker-NVRAM gelöscht.", self.selected_slot),
                    false,
                );
            }
            Err(e) => {
                self.set_status(
                    format!("Fehler beim Löschen von Slot {:02}: {e}", self.selected_slot),
                    true,
                );
            }
        }
    }

    fn apply_emx_to_editor(&mut self, pkg: &EmxPackage) {
        // Save embedded ICC profile if present
        if !pkg.icc_profile_name.is_empty() && !pkg.icc_profile_bytes.is_empty() {
            for dir in default_icc_output_dirs() {
                if std::fs::create_dir_all(&dir).is_ok() {
                    let _ = std::fs::write(dir.join(&pkg.icc_profile_name), &pkg.icc_profile_bytes);
                }
            }
        }

        let slot_obj = pkg.to_printer_slot(self.selected_slot);
        self.editor_slot = slot_obj;
        self.editor_print_duty = pkg.print_duty();
        self.editor_icc_profile = if pkg.icc_profile_name.is_empty() {
            self.editor_slot.base_media_spec().icc_profile.to_string()
        } else {
            pkg.icc_profile_name.clone()
        };

        self.set_status(
            format!(
                ".emx '{}' (MA_ID 0x{:08X}) in den Slot-Editor (Slot {:02}) übernommen. Klicke rechts auf 'Slot in Drucker-NVRAM schreiben', um ihn zu übertragen.",
                pkg.media_name, pkg.user_media_id, self.selected_slot
            ),
            false,
        );
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
                        if ui.button("📂 Lokale .emx-Datei laden…").clicked() {
                            self.active_view = WorkspaceView::EmxPackages;
                            ui.close_menu();
                        }
                        if ui
                            .button("💾 Aktive Slots → custom_media.json synchronisieren")
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

                    ui.menu_button("Drucker-NVRAM", |ui| {
                        if ui
                            .button("🔄 Alle 30 NVRAM-Slots per SNMPv1 auslesen")
                            .clicked()
                        {
                            self.poll_printer_slots();
                            ui.close_menu();
                        }
                        if ui
                            .button(format!(
                                "📤 Aktuellen Slot {:02} in Drucker schreiben",
                                self.selected_slot
                            ))
                            .clicked()
                        {
                            self.write_selected_slot_to_printer();
                            ui.close_menu();
                        }
                        if ui
                            .button(format!(
                                "🗑 Aktuellen Slot {:02} im Drucker löschen",
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
                            "Rechtes Slot-Inspektor-Dock anzeigen",
                        );
                        ui.checkbox(
                            &mut self.show_empty_slots,
                            "Auch freie Slots (1..30) in der Übersicht zeigen",
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
                            RichText::new(format!(
                                "KDE-Schrift: {} ({:.0} pt) — UI-Skalierung:",
                                self.kde_typography.family, self.kde_typography.point_size
                            ))
                            .small()
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

                // Row 2: Action Toolbar
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    ui.label(RichText::new("Drucker-IP:").color(t.text_dim));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.printer_host)
                            .desired_width(120.0)
                            .hint_text("10.10.10.124"),
                    );

                    let poll_btn = egui::Button::new(
                        RichText::new("🔄 Slots abfragen")
                            .strong()
                            .color(t.accent_text),
                    )
                    .fill(t.accent)
                    .min_size(Vec2::new(135.0, 28.0));
                    if ui
                        .add(poll_btn)
                        .on_hover_text(
                            "Liest alle 30 Benutzer-Slots per SNMPv1 (REMOTE1 ex) vom SC-P900/P700",
                        )
                        .clicked()
                    {
                        self.poll_printer_slots();
                    }

                    if ui
                        .add(
                            egui::Button::new(RichText::new("💾 Sync → Treiber").strong())
                                .min_size(Vec2::new(130.0, 28.0)),
                        )
                        .on_hover_text(
                            "Schreibt alle belegten Slots in custom_media.json für CUPS & LumaPrint",
                        )
                        .clicked()
                    {
                        self.sync_slots_to_driver();
                    }

                    ui.separator();

                    // Workspace View Switcher
                    for (view, label) in [
                        (WorkspaceView::PrinterSlots, "🖨 Drucker-Slots (1..30)"),
                        (WorkspaceView::EmxPackages, "📦 Lokale .emx-Pakete"),
                        (WorkspaceView::CloudCatalog, "☁ Epson Cloud-Katalog"),
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

                    // Right-aligned Dock Toggle (Theme selection stays in 'Ansicht' menu)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dock_btn = egui::Button::new(
                            RichText::new("⚙ Slot-Dock").color(if self.show_right_dock {
                                t.accent
                            } else {
                                t.text_dim
                            }),
                        );
                        if ui
                            .add(dock_btn)
                            .on_hover_text("Rechtes Slot-Inspektor-Dock ein-/ausblenden")
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
                        RichText::new(format!("● {occupied}/30 Slots belegt"))
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
                                "📤 Slot {:02} in Drucker schreiben",
                                self.selected_slot
                            ))
                            .strong()
                            .color(t.accent_text),
                        )
                        .fill(t.accent)
                        .min_size(Vec2::new(220.0, 32.0));

                        if ui
                            .add(write_btn)
                            .on_hover_text(
                                "Überträgt den rechts bearbeiteten Slot per SNMPv1 (REMOTE1 ex) in den SC-P900",
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
                                    "★ Slot {:02} Inspektor (UI-ID {})",
                                    self.selected_slot,
                                    1900 + u16::from(self.selected_slot)
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
                                        ("LEER", t.text_faint)
                                    };
                                    ui.label(RichText::new(lbl).small().strong().color(col));
                                },
                            );
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);

                        // Section 1: Slot-Auswahl & Identität
                        self.draw_dock_section(
                            ui,
                            "1. Slot-Identität & Basismedium",
                            true,
                            |app, ui| {
                                app.draw_dock_identity_section(ui);
                            },
                        );

                        // Section 2: Mechanische NVRAM-Parameter (TLV)
                        self.draw_dock_section(
                            ui,
                            "2. Mechanische Parameter (NVRAM TLV)",
                            true,
                            |app, ui| {
                                app.draw_dock_mechanics_section(ui);
                            },
                        );

                        // Section 3: Treiber-, PPD- & ICC-Verknüpfung
                        self.draw_dock_section(
                            ui,
                            "3. CUPS / LumaPrint & ICC-Profil",
                            true,
                            |app, ui| {
                                app.draw_dock_driver_section(ui);
                            },
                        );

                        // Section 4: NVRAM-Schreib-/Lösch-Aktionen
                        self.draw_dock_section(
                            ui,
                            "4. Drucker-NVRAM Aktionen",
                            true,
                            |app, ui| {
                                app.draw_dock_actions_section(ui);
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
            .min_col_width(130.0)
            .show(ui, |ui| {
                ui.label(RichText::new("NVRAM-Slot:").color(t.text_dim));
                let mut new_sel = current_slot;
                egui::ComboBox::from_id_salt("dock_slot_selector")
                    .selected_text(format!(
                        "Slot {:02} (ID {})",
                        current_slot,
                        1900 + u16::from(current_slot)
                    ))
                    .width(235.0)
                    .show_ui(ui, |ui| {
                        for idx in 1..=30_u8 {
                            let s_name = self
                                .slots
                                .get(usize::from(idx - 1))
                                .filter(|s| s.registered)
                                .map(|s| s.media_name.as_str())
                                .unwrap_or("— Frei —");
                            let lbl = format!("Slot {:02}: {}", idx, s_name);
                            let _ = ui.selectable_value(&mut new_sel, idx, lbl);
                        }
                    });
                ui.end_row();
                if new_sel != current_slot {
                    self.select_slot(new_sel);
                }

                ui.label(RichText::new("Medienname:").color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut self.editor_slot.media_name)
                        .desired_width(235.0)
                        .hint_text("z. B. Photolux StudioDUO 250"),
                );
                ui.end_row();

                ui.label(RichText::new("MA_ID (Tag 0x24):").color(t.text_dim));
                ui.horizontal(|ui| {
                    let mut hex_id = format!("0x{:08X}", self.editor_slot.user_media_id);
                    if ui
                        .add(egui::TextEdit::singleline(&mut hex_id).desired_width(120.0))
                        .changed()
                    {
                        let clean = hex_id.trim().trim_start_matches("0x").trim_start_matches("0X");
                        if let Ok(parsed) = u32::from_str_radix(clean, 16) {
                            self.editor_slot.user_media_id = parsed;
                        }
                    }
                    if ui
                        .button("Auto-ID")
                        .on_hover_text("Vergibt eigene ID 0xC0000001..0xC000001E")
                        .clicked()
                    {
                        self.editor_slot.user_media_id =
                            0xC000_0000 | u32::from(self.selected_slot);
                    }
                });
                ui.end_row();

                ui.label(RichText::new("Basismedium:").color(t.text_dim));
                let cur_base = find_base_media_by_sn_id(self.editor_slot.base_sn_id);
                let cur_base_lbl = cur_base.map_or_else(
                    || format!("SN {} (0x{:02X})", self.editor_slot.base_sn_id, self.editor_slot.base_sn_id),
                    |b| {
                        let ink = if b.is_matte { "MK" } else { "PK" };
                        format!("{} [{} · SN {}]", b.name, ink, b.sn_id)
                    },
                );
                egui::ComboBox::from_id_salt("dock_base_sn_combo")
                    .selected_text(cur_base_lbl)
                    .width(235.0)
                    .show_ui(ui, |ui| {
                        for preset in BASE_MEDIA_SPECS {
                            let ink = if preset.is_matte { "MK" } else { "PK" };
                            let lbl = format!(
                                "{} [{} · SN {} / 0x{:02X}]",
                                preset.name, ink, preset.sn_id, preset.sn_id
                            );
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
                    ("Photo Black (PK) — Photo / Baryta", t.accent)
                };
                ui.label(RichText::new(ink_txt).strong().color(ink_col));
                ui.end_row();

                ui.label(RichText::new("Kategorie:").color(t.text_dim));
                egui::ComboBox::from_id_salt("dock_category_combo")
                    .selected_text(media_category_label(self.editor_slot.media_category))
                    .width(235.0)
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
            .min_col_width(130.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Walzenabstand:").color(t.text_dim));
                egui::ComboBox::from_id_salt("dock_platen_gap_combo")
                    .selected_text(platen_gap_label(self.editor_slot.platen_gap))
                    .width(235.0)
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
                ui.add(
                    egui::Slider::new(&mut self.editor_slot.drying_time, 0..=100).suffix(" %"),
                );
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
                    .checkbox(&mut check_on, "Papierbreite vor Druck prüfen")
                    .changed()
                {
                    self.editor_slot.paper_width_check = u8::from(check_on);
                }
                ui.end_row();
            });
    }

    fn draw_dock_driver_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        let preview_key = sanitize_ppd_media_key(self.selected_slot, &self.editor_slot.media_name);

        egui::Grid::new("dock_grid_driver")
            .num_columns(2)
            .spacing([14.0, 10.0])
            .min_col_width(130.0)
            .show(ui, |ui| {
                ui.label(RichText::new("PPD MediaType Key:").color(t.text_dim));
                ui.label(
                    RichText::new(format!("{preview_key} ({} B)", preview_key.len()))
                        .monospace()
                        .color(t.accent),
                );
                ui.end_row();

                ui.label(RichText::new("Tintenauftrag:").color(t.text_dim));
                ui.add(
                    egui::Slider::new(&mut self.editor_print_duty, -50..=50)
                        .suffix(" % (PrintDuty)"),
                );
                ui.end_row();

                ui.label(RichText::new("ICC-Profil (.icc):").color(t.text_dim));
                ui.add(
                    egui::TextEdit::singleline(&mut self.editor_icc_profile)
                        .desired_width(235.0)
                        .hint_text("z. B. SureColor_P900_FineArtBaryta_v1.icc"),
                );
                ui.end_row();

                ui.label(RichText::new("ESC/P-R Level 5:").color(t.text_dim));
                ui.label(
                    RichText::new(format!(
                        "setq[0]=0x{:02X} · setl=0x{:08X}",
                        self.editor_slot.base_sn_id, self.editor_slot.user_media_id
                    ))
                    .small()
                    .monospace()
                    .color(t.text_faint),
                );
                ui.end_row();
            });
    }

    fn draw_dock_actions_section(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        ui.vertical_centered_justified(|ui| {
            let write_btn = egui::Button::new(
                RichText::new(format!(
                    "📤 Slot {:02} in Drucker-NVRAM schreiben",
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
                    "🗑 Slot {:02} im Drucker-NVRAM löschen",
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
                WorkspaceView::EmxPackages => self.draw_emx_workspace(ui),
                WorkspaceView::CloudCatalog => self.draw_cloud_workspace(ui),
            });
    }

    fn draw_slots_workspace(&mut self, ui: &mut Ui) {
        let t = self.tokens;

        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("Drucker-NVRAM Slots (1..30 — Epson SureColor SC-P900 / SC-P700)")
                    .strong()
                    .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.show_empty_slots, "Freie Slots anzeigen");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_filter)
                        .desired_width(180.0)
                        .hint_text("🔍 Slot / Papier filtern…"),
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
                            .spacing([16.0, 10.0])
                            .show(ui, |ui| {
                                for hdr in [
                                    "Slot",
                                    "UI-ID",
                                    "Status / Medienname",
                                    "MA_ID (ESC q setl)",
                                    "Basismedium (ESC q setq)",
                                    "Tinte",
                                    "Dicke",
                                    "Walzenabstand",
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
                                            RichText::new(format!("Slot {:02}", slot.slot))
                                                .strong()
                                                .color(row_col),
                                        )
                                        .clicked()
                                    {
                                        clicked_slot = Some(slot.slot);
                                    }

                                    ui.label(
                                        RichText::new(format!("{}", slot.driver_ui_id()))
                                            .monospace()
                                            .color(t.text_dim),
                                    );

                                    let name_display = if is_reg {
                                        format!("● {}", slot.media_name)
                                    } else {
                                        String::from("○ — Frei —")
                                    };
                                    if ui
                                        .selectable_label(
                                            is_sel,
                                            RichText::new(name_display)
                                                .strong()
                                                .color(if is_reg { t.text } else { t.text_faint }),
                                        )
                                        .clicked()
                                    {
                                        clicked_slot = Some(slot.slot);
                                    }

                                    let id_text = if is_reg {
                                        format!("0x{:08X}", slot.user_media_id)
                                    } else {
                                        String::from("—")
                                    };
                                    ui.label(
                                        RichText::new(id_text).monospace().color(if is_reg {
                                            t.accent
                                        } else {
                                            t.text_faint
                                        }),
                                    );

                                    let base_info = find_base_media_by_sn_id(slot.base_sn_id);
                                    let base_label = if is_reg {
                                        base_info.map_or_else(
                                            || format!("SN {} (0x{:02X})", slot.base_sn_id, slot.base_sn_id),
                                            |b| format!("{} (SN {})", b.name, b.sn_id),
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

                                    ui.end_row();
                                }
                            });
                    });
            });

        if let Some(s) = clicked_slot {
            self.select_slot(s);
            self.show_right_dock = true;
        }
    }

    fn draw_emx_workspace(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        ui.heading(
            RichText::new("Lokale .emx / .emy Medienpakete inspizieren & importieren")
                .strong()
                .color(t.text),
        );
        ui.add_space(8.0);

        // Loader Card
        egui::Frame::new()
            .fill(t.card)
            .corner_radius(CornerRadius::same(t.radius))
            .inner_margin(Margin::symmetric(14, 12))
            .stroke(Stroke::new(1.0_f32, t.card_border))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Pfad zur .emx-Datei:").color(t.text_dim));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.emx_input_path).desired_width(480.0),
                    );
                    let load_btn = egui::Button::new(
                        RichText::new("📂 .emx laden").strong().color(t.accent_text),
                    )
                    .fill(t.accent);
                    if ui.add(load_btn).clicked() {
                        match parse_emx_file(self.emx_input_path.trim()) {
                            Ok(pkg) => {
                                self.set_status(
                                    format!(
                                        ".emx '{}' erfolgreich geladen (MA_ID 0x{:08X}).",
                                        pkg.media_name, pkg.user_media_id
                                    ),
                                    false,
                                );
                                self.loaded_emx = Some(pkg);
                            }
                            Err(e) => {
                                self.set_status(format!("Fehler beim Laden der .emx: {e}"), true);
                            }
                        }
                    }
                });
            });

        ui.add_space(12.0);

        if let Some(pkg) = self.loaded_emx.clone() {
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
                            let import_btn = egui::Button::new(
                                RichText::new(format!(
                                    "➡ In Slot {:02} übernehmen & ICC speichern",
                                    self.selected_slot
                                ))
                                .strong()
                                .color(t.accent_text),
                            )
                            .fill(t.accent)
                            .min_size(Vec2::new(260.0, 32.0));

                            if ui.add(import_btn).clicked() {
                                self.apply_emx_to_editor(&pkg);
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
                            ui.label(RichText::new("Hersteller (Vendor):").color(t.text_dim));
                            ui.label(RichText::new(&pkg.vendor).strong());
                            ui.end_row();

                            ui.label(RichText::new("Modell / Serie:").color(t.text_dim));
                            ui.label(format!("{} ({})", pkg.model_name, pkg.series_name));
                            ui.end_row();

                            ui.label(
                                RichText::new("Offizielle MediaID (Tag 0x24 / ESC q setl):")
                                    .color(t.text_dim),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "0x{:08X} ({})",
                                    pkg.user_media_id, pkg.user_media_id
                                ))
                                .monospace()
                                .strong()
                                .color(t.accent),
                            );
                            ui.end_row();

                            ui.label(
                                RichText::new("Basismedium (ESC q setq Byte 0):").color(t.text_dim),
                            );
                            let base_lbl = find_base_media_by_sn_id(pkg.base_sn_id).map_or_else(
                                || format!("SN {} (0x{:02X})", pkg.base_sn_id, pkg.base_sn_id),
                                |b| format!("{} (SN {} / 0x{:02X})", b.name, b.sn_id, b.sn_id),
                            );
                            ui.label(RichText::new(base_lbl).strong());
                            ui.end_row();

                            ui.label(RichText::new("Papierdicke:").color(t.text_dim));
                            ui.label(format!(
                                "{:.2} mm ({} × 0.01 mm)",
                                pkg.thickness_mm(),
                                pkg.thickness_01mm
                            ));
                            ui.end_row();

                            ui.label(RichText::new("Walzenabstand (Platen Gap):").color(t.text_dim));
                            ui.label(platen_gap_label(pkg.platen_gap));
                            ui.end_row();

                            ui.label(
                                RichText::new("Tintenauftrag (PrintDuty / ESC p setq[5]):")
                                    .color(t.text_dim),
                            );
                            ui.label(format!(
                                "{} % (InkDensity {})",
                                pkg.print_duty(),
                                pkg.ink_density
                            ));
                            ui.end_row();

                            ui.label(RichText::new("Enthaltenes ICC-Profil:").color(t.text_dim));
                            let icc_info = if !pkg.icc_profile_name.is_empty() {
                                format!(
                                    "{} ({:.1} KB)",
                                    pkg.icc_profile_name,
                                    pkg.icc_profile_bytes.len() as f64 / 1024.0
                                )
                            } else {
                                String::from("Kein ICC-Profil im .emx enthalten")
                            };
                            ui.label(RichText::new(icc_info).monospace().color(t.success));
                            ui.end_row();
                        });
                });
        }
    }

    fn draw_cloud_workspace(&mut self, ui: &mut Ui) {
        let t = self.tokens;
        ui.horizontal(|ui| {
            ui.heading(
                RichText::new("Epson Cloud-Medienkatalog (support.epson.biz/lfp/cms/v2)")
                    .strong()
                    .color(t.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let fetch_btn = egui::Button::new(
                    RichText::new("☁ Katalog abrufen")
                        .strong()
                        .color(t.accent_text),
                )
                .fill(t.accent);
                if ui.add(fetch_btn).clicked() {
                    match cloud_list_media(self.cloud_model.code(), "DE", 8, "de") {
                        Ok(items) => {
                            let custom_count = items.iter().filter(|r| r.is_custom_emx()).count();
                            self.set_status(
                                format!(
                                    "{} Medienpakete ({custom_count} Zusatz-EMX + {} Standard-EMX) für {} vom Epson-Cloud-Server geladen.",
                                    items.len(),
                                    items.len().saturating_sub(custom_count),
                                    self.cloud_model.label()
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

                egui::ComboBox::from_id_salt("cloud_model_combo")
                    .selected_text(self.cloud_model.label())
                    .width(210.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.cloud_model,
                            CloudModelChoice::ScP900,
                            CloudModelChoice::ScP900.label(),
                        );
                        ui.selectable_value(
                            &mut self.cloud_model,
                            CloudModelChoice::ScP700,
                            CloudModelChoice::ScP700.label(),
                        );
                    });

                ui.add(
                    egui::TextEdit::singleline(&mut self.search_filter)
                        .desired_width(180.0)
                        .hint_text("🔍 Hersteller / Name…"),
                );
            });
        });

        ui.add_space(8.0);

        let filter_lower = self.search_filter.trim().to_lowercase();
        let mut download_item: Option<CloudMediaRecord> = None;

        egui::Frame::new()
            .fill(t.card)
            .corner_radius(CornerRadius::same(t.radius))
            .inner_margin(Margin::symmetric(14, 12))
            .stroke(Stroke::new(1.0_f32, t.card_border))
            .show(ui, |ui| {
                if self.cloud_items.is_empty() {
                    ui.label(
                        RichText::new(
                            "Noch keine Katalogdaten geladen. Klicke oben rechts auf '☁ Katalog abrufen'.",
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
                            .num_columns(7)
                            .spacing([16.0, 10.0])
                            .show(ui, |ui| {
                                for hdr in [
                                    "Hersteller",
                                    "Medienname",
                                    "Kategorie",
                                    "EMX-ID (MA_ID)",
                                    "Typ / Datei",
                                    "Stand",
                                    "Aktion",
                                ] {
                                    ui.label(RichText::new(hdr).strong().color(t.text_dim));
                                }
                                ui.end_row();

                                for item in &self.cloud_items {
                                    if !filter_lower.is_empty()
                                        && !item
                                            .media_type_name
                                            .to_lowercase()
                                            .contains(&filter_lower)
                                        && !item
                                            .media_manufacturer_name
                                            .to_lowercase()
                                            .contains(&filter_lower)
                                        && !item.emx_id.to_lowercase().contains(&filter_lower)
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
                                    ui.label(
                                        RichText::new(&item.emx_id).monospace().color(t.accent),
                                    );
                                    let kind_col = if item.is_custom_emx() {
                                        t.success
                                    } else {
                                        t.text_dim
                                    };
                                    ui.label(
                                        RichText::new(&item.file_name)
                                            .small()
                                            .monospace()
                                            .color(kind_col),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} ({})",
                                            item.updated, item.version
                                        ))
                                        .small()
                                        .color(t.text_faint),
                                    );
                                    if item.has_downloadable_emx() {
                                        if ui
                                            .button(format!(
                                                "⬇ Laden → Slot {:02}",
                                                self.selected_slot
                                            ))
                                            .clicked()
                                        {
                                            download_item = Some(item.clone());
                                        }
                                    } else {
                                        ui.label(RichText::new("Kein .emx").color(t.text_faint));
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });

        if let Some(item) = download_item {
            let cache_dir = std::env::temp_dir().join("epmi-downloads");
            match cloud_download_emx(&item, &cache_dir).and_then(parse_emx_file) {
                Ok(pkg) => {
                    self.apply_emx_to_editor(&pkg);
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
            .default_width(480.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.heading(
                    RichText::new("epmi — Media Management für Epson SureColor SC-P900 / SC-P700")
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
                    und .emx-Medienpakete für die Fine-Art-Fotodrucker Epson SureColor SC-P900 \
                    und SC-P700 unter Linux:",
                );
                ui.add_space(6.0);
                ui.label(
                    "• Auslesen, Bearbeiten und Beschreiben der 30 NVRAM-Slots (1..30) direkt im Drucker per SNMPv1\n\
                     • Inspizieren und Importieren von .emx-Medienpaketen (inkl. mechanischer Parameter und ICC-Profile)\n\
                     • Abrufen und Herunterladen von Medienprofilen aus dem Online-Medienkatalog\n\
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
    let mut host = String::from("10.10.10.124");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--host"
            && let Some(h) = args.next()
        {
            host = h;
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("epmi — Epson SureColor SC-P900 / SC-P700 Media Installer")
            .with_app_id("epmi-gui")
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([1024.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "epmi — Epson SureColor SC-P900 / SC-P700 Media Installer",
        options,
        Box::new(move |cc| Ok(Box::new(EpmiGuiApp::new(cc, host)))),
    )
}
