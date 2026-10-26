use serde::{Deserialize, Serialize};

/// Official built-in Epson SC-P900 / SC-P700 base media specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseMediaSpec {
    pub key: &'static str,
    pub name: &'static str,
    pub ppd_id: u16,
    /// Hardware `sn_id` (`LegacyBaseMedia` in TLV Tag `0x02` and byte 0 of `ESC q setq`).
    pub sn_id: u8,
    pub icc_profile: &'static str,
    pub win_icc: &'static str,
    pub is_matte: bool,
    pub default_overcoat: u8,
    pub supports_carbon_black: bool,
    pub default_category: u8,
    pub default_platen_gap: u8,
    pub default_thickness_01mm: u16,
}

/// Complete table of all 18 official Epson SC-P900 / SC-P700 base media types.
/// `sn_id` matches `LegacyBaseMedia` (Tag `0x02`) in NVRAM and `<PrinterMediaID>` in `.emx` 1:1.
pub const BASE_MEDIA_SPECS: &[BaseMediaSpec] = &[
    BaseMediaSpec {
        key: "PremiumGlossy",
        name: "Epson Premium Glossy",
        ppd_id: 13,
        sn_id: 11,
        icc_profile: "Epson_SC-P900_700_EpsonPremiumGlossy.icc",
        win_icc: "EE2NE_04.ICC",
        is_matte: false,
        default_overcoat: 2,
        supports_carbon_black: true,
        default_category: 1,
        default_platen_gap: 9,
        default_thickness_01mm: 27,
    },
    BaseMediaSpec {
        key: "PremiumSemigloss",
        name: "Epson Premium Semigloss",
        ppd_id: 15,
        sn_id: 12,
        icc_profile: "Epson_SC-P900_700_EpsonPremiumSemigloss.icc",
        win_icc: "EE2NE_05.ICC",
        is_matte: false,
        default_overcoat: 2,
        supports_carbon_black: true,
        default_category: 1,
        default_platen_gap: 9,
        default_thickness_01mm: 27,
    },
    BaseMediaSpec {
        key: "PremiumLuster",
        name: "Epson Premium Luster Photo Paper",
        ppd_id: 27,
        sn_id: 13,
        icc_profile: "Epson_SC-P900_700_PremiumLusterPhotoPaper.icc",
        win_icc: "EE2NE_08.ICC",
        is_matte: false,
        default_overcoat: 2,
        supports_carbon_black: true,
        default_category: 1,
        default_platen_gap: 9,
        default_thickness_01mm: 27,
    },
    BaseMediaSpec {
        key: "PhotoPaperGlossy",
        name: "Photo Paper Glossy",
        ppd_id: 145,
        sn_id: 52,
        icc_profile: "Epson_SC-P900_700_PhotoPaperGlossy.icc",
        win_icc: "EE2NE_07.ICC",
        is_matte: false,
        default_overcoat: 2,
        supports_carbon_black: true,
        default_category: 1,
        default_platen_gap: 9,
        default_thickness_01mm: 25,
    },
    BaseMediaSpec {
        key: "Baryta",
        name: "Epson Traditional Photo Paper / Baryta",
        ppd_id: 157,
        sn_id: 174,
        icc_profile: "Epson_SC-P900_700_Baryta.icc",
        win_icc: "EE2NE_01.ICC",
        is_matte: false,
        default_overcoat: 2,
        supports_carbon_black: true,
        default_category: 1,
        default_platen_gap: 15,
        default_thickness_01mm: 30,
    },
    BaseMediaSpec {
        key: "VelvetFineArt",
        name: "Velvet Fine Art Paper",
        ppd_id: 53,
        sn_id: 25,
        icc_profile: "Epson_SC-P900_700_VelvetFineArtPaper.icc",
        win_icc: "EE2NE_11.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 4,
        default_platen_gap: 21,
        default_thickness_01mm: 48,
    },
    BaseMediaSpec {
        key: "UltraSmoothFineArt",
        name: "Epson UltraSmooth Fine Art Paper",
        ppd_id: 88,
        sn_id: 39,
        icc_profile: "Epson_SC-P900_700_UltraSmoothFineArtPaper.icc",
        win_icc: "EE2NE_10.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 4,
        default_platen_gap: 21,
        default_thickness_01mm: 46,
    },
    BaseMediaSpec {
        key: "WatercolorRadiantWhite",
        name: "Watercolor Paper Radiant White",
        ppd_id: 17,
        sn_id: 16,
        icc_profile: "Epson_SC-P900_700_WatercolorPaperRadiantWhite.icc",
        win_icc: "EE2NE_13.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 4,
        default_platen_gap: 21,
        default_thickness_01mm: 29,
    },
    BaseMediaSpec {
        key: "FineArtMatte",
        name: "Signature Worthy / Fine Art Matte",
        ppd_id: 183,
        sn_id: 206,
        icc_profile: "Epson_SC-P900_700_FineArtMatte.icc",
        win_icc: "EE2NE_06.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 4,
        default_platen_gap: 21,
        default_thickness_01mm: 48,
    },
    BaseMediaSpec {
        key: "Washi",
        name: "Japanese Kozo Paper Thin / Washi",
        ppd_id: 181,
        sn_id: 205,
        icc_profile: "Epson_SC-P900_700_Washi.icc",
        win_icc: "EE2NE_12.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 4,
        default_platen_gap: 15,
        default_thickness_01mm: 15,
    },
    BaseMediaSpec {
        key: "ArchivalMatte",
        name: "Epson Archival / Enhanced Matte Paper",
        ppd_id: 14,
        sn_id: 15,
        icc_profile: "Epson_SC-P900_700_ArchivalMattePaper.icc",
        win_icc: "EE2NE_00.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 3,
        default_platen_gap: 9,
        default_thickness_01mm: 25,
    },
    BaseMediaSpec {
        key: "PhotoQualityInkJet",
        name: "Epson Photo Quality Ink Jet",
        ppd_id: 2,
        sn_id: 29,
        icc_profile: "Epson_SC-P900_700_ArchivalMattePaper.icc",
        win_icc: "EE2NE_00.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 3,
        default_platen_gap: 9,
        default_thickness_01mm: 12,
    },
    BaseMediaSpec {
        key: "CanvasSatin",
        name: "Exhibition Canvas Satin",
        ppd_id: 153,
        sn_id: 171,
        icc_profile: "Epson_SC-P900_700_CanvasSatin.icc",
        win_icc: "EE2NE_03.ICC",
        is_matte: false,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 5,
        default_platen_gap: 21,
        default_thickness_01mm: 45,
    },
    BaseMediaSpec {
        key: "CanvasMatte",
        name: "Exhibition Canvas Matte",
        ppd_id: 155,
        sn_id: 169,
        icc_profile: "Epson_SC-P900_700_CanvasMatte.icc",
        win_icc: "EE2NE_02.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 5,
        default_platen_gap: 21,
        default_thickness_01mm: 43,
    },
    BaseMediaSpec {
        key: "PlainPaper",
        name: "Plain Paper / Bright White Paper",
        ppd_id: 0,
        sn_id: 0,
        icc_profile: "Epson_SC-P900_700_Standard.icc",
        win_icc: "EE2NE_09.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 6,
        default_platen_gap: 9,
        default_thickness_01mm: 11,
    },
    BaseMediaSpec {
        key: "ProofingWhiteSemimatte",
        name: "Epson Proofing Paper White Semimatte",
        ppd_id: 106,
        sn_id: 142,
        icc_profile: "Epson_SC-P900_700_ProofingPaperWhiteSemimatte.icc",
        win_icc: "EE2NE_14.ICC",
        is_matte: false,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 2,
        default_platen_gap: 9,
        default_thickness_01mm: 25,
    },
    BaseMediaSpec {
        key: "DiscLabel",
        name: "CD/DVD Disc Label",
        ppd_id: 26,
        sn_id: 91,
        icc_profile: "Epson_SC-P900_700_Standard.icc",
        win_icc: "EE2NE_09.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 8,
        default_platen_gap: 9,
        default_thickness_01mm: 120,
    },
    BaseMediaSpec {
        key: "DiscLabelPremium",
        name: "CD/DVD Premium Surface",
        ppd_id: 76,
        sn_id: 92,
        icc_profile: "Epson_SC-P900_700_Standard.icc",
        win_icc: "EE2NE_09.ICC",
        is_matte: true,
        default_overcoat: 1,
        supports_carbon_black: false,
        default_category: 8,
        default_platen_gap: 9,
        default_thickness_01mm: 120,
    },
];

pub fn find_base_media_by_sn_id(sn_id: u8) -> Option<&'static BaseMediaSpec> {
    BASE_MEDIA_SPECS.iter().find(|m| m.sn_id == sn_id)
}

pub fn find_base_media(query: &str) -> Option<&'static BaseMediaSpec> {
    let q = query.trim();
    if let Ok(sn) = q.parse::<u8>()
        && let Some(spec) = find_base_media_by_sn_id(sn)
    {
        return Some(spec);
    }
    BASE_MEDIA_SPECS
        .iter()
        .find(|m| m.key.eq_ignore_ascii_case(q) || m.name.eq_ignore_ascii_case(q))
}

pub fn media_category_label(category: u8) -> &'static str {
    match category {
        1 => "Photo Paper",
        2 => "Proofing Paper",
        3 => "Matte Paper",
        4 => "Fine Art Paper",
        5 => "Canvas",
        6 => "Plain Paper",
        7 => "Post Card",
        8 => "CD/DVD",
        254 => "Others",
        255 => "Unregistered",
        _ => "Custom",
    }
}

pub fn platen_gap_label(gap: u8) -> &'static str {
    match gap {
        9 => "0.9 mm (Auto / Narrow)",
        12 => "1.2 mm (Standard Narrow)",
        15 => "1.5 / 1.6 mm (Standard)",
        21 => "2.1 mm (Wide)",
        24 => "2.4 mm (Wider)",
        _ => "Custom Gap",
    }
}

/// Represents one of the 30 custom media NVRAM slots (`1..=30`) on the SC-P900 / SC-P700.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterMediaSlot {
    pub slot: u8,
    pub registered: bool,
    pub media_name: String,
    /// Tag `0x24`: 32-bit `MediaID` (`MA_ID` sent Big-Endian in `ESC q setl`).
    pub user_media_id: u32,
    /// Tag `0x02`: `LegacyBaseMedia` (`sn_id` sent in `ESC q setq` byte 0).
    pub base_sn_id: u8,
    /// Tag `0x25`: `MediaCategory` (`1`=Photo, `2`=Proofing, `3`=Matte, `4`=FineArt, `5`=Canvas, `6`=Plain).
    pub media_category: u8,
    /// Tag `0x03`: `PlatenGap` (`9`=0.9mm, `12`=1.2mm, `15`=1.5/1.6mm, `21`=2.1mm, `24`=2.4mm).
    pub platen_gap: u8,
    /// Tag `0x19`: `PaperThickness` in `0.01 mm` units (`27` = `0.27 mm`, `48` = `0.48 mm`).
    pub thickness_01mm: u16,
    /// Tag `0x04`: `DryTime` per pass (`0..=100`).
    pub drying_time: u8,
    /// Tag `0x22`: `MeasurementPaperWidth` (`1` = On, `0` = Off).
    pub paper_width_check: u8,
    /// Tag `0x28[0..2]`: `CutSheetFeedOffset` (`i16 LE`).
    pub cut_sheet_feed_offset: i16,
    /// Tag `0x28[2..4]`: `RollPaperFeedOffset` (`i16 LE`).
    pub roll_paper_feed_offset: i16,
    /// Tag `0x2B`: `NumberOfMarginSet` (always `0`).
    pub margin_sets: u16,
}

impl Default for PrinterMediaSlot {
    fn default() -> Self {
        Self {
            slot: 1,
            registered: false,
            media_name: String::new(),
            user_media_id: 0,
            base_sn_id: 25,
            media_category: 4,
            platen_gap: 21,
            thickness_01mm: 48,
            drying_time: 0,
            paper_width_check: 1,
            cut_sheet_feed_offset: 0,
            roll_paper_feed_offset: 0,
            margin_sets: 0,
        }
    }
}

impl PrinterMediaSlot {
    pub fn empty(slot: u8) -> Self {
        Self {
            slot,
            ..Self::default()
        }
    }

    pub fn thickness_mm(&self) -> f64 {
        f64::from(self.thickness_01mm) / 100.0
    }

    pub fn driver_ui_id(&self) -> u16 {
        1900 + u16::from(self.slot)
    }

    pub fn base_media_spec(&self) -> &'static BaseMediaSpec {
        find_base_media_by_sn_id(self.base_sn_id).unwrap_or(&BASE_MEDIA_SPECS[5])
    }

    pub fn base_media_key(&self) -> &'static str {
        self.base_media_spec().key
    }

    /// Generate a CUPS PPD-compliant option key (`<= 32` ASCII characters, umlauts transliterated).
    pub fn suggested_driver_key(&self) -> String {
        sanitize_ppd_media_key(self.slot, &self.media_name)
    }

    pub fn to_custom_media_entry(&self, existing: Option<&CustomMediaEntry>) -> CustomMediaEntry {
        let base = self.base_media_spec();
        let key = match existing {
            Some(prev)
                if !prev.key.is_empty()
                    && prev.key.len() <= 32
                    && !prev.key.starts_with("CustomSlot") =>
            {
                prev.key.clone()
            }
            _ => self.suggested_driver_key(),
        };
        let icc_profile = match existing {
            Some(prev) if !prev.icc_profile.is_empty() => prev.icc_profile.clone(),
            _ => base.icc_profile.to_string(),
        };
        let vendor = existing.map(|e| e.vendor.clone()).unwrap_or_default();
        let print_duty = existing.map_or(0, |e| e.print_duty);
        let bottom_edge_priority = existing.is_some_and(|e| e.bottom_edge_priority);
        let rubbing_reduction = existing.map_or(0, |e| e.rubbing_reduction);

        CustomMediaEntry {
            slot: self.slot,
            user_media_id: self.user_media_id,
            key,
            name: format!("{} (Slot {})", self.media_name, self.slot),
            media_name: self.media_name.clone(),
            vendor,
            base_media: base.key.to_string(),
            base_sn_id: self.base_sn_id,
            media_category: self.media_category,
            print_duty,
            platen_gap: self.platen_gap,
            thickness: self.thickness_01mm,
            thickness_mm: (f64::from(self.thickness_01mm) / 100.0 * 100.0).round() / 100.0,
            feed_offset: self.cut_sheet_feed_offset,
            cut_sheet_feed_offset: self.cut_sheet_feed_offset,
            roll_paper_feed_offset: self.roll_paper_feed_offset,
            drying_time: self.drying_time,
            paper_width_check: self.paper_width_check,
            bottom_edge_priority,
            rubbing_reduction,
            icc_profile,
        }
    }
}

/// Shared JSON schema (`custom_media.json`) between `epmi`, `epson-p900-escpr-cups`, and `LumaPrint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomMediaEntry {
    #[serde(default)]
    pub slot: u8,
    #[serde(default)]
    pub user_media_id: u32,
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub media_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub vendor: String,
    pub base_media: String,
    pub base_sn_id: u8,
    #[serde(default = "default_category_fine_art")]
    pub media_category: u8,
    #[serde(default)]
    pub print_duty: i8,
    #[serde(default = "default_platen_gap")]
    pub platen_gap: u8,
    #[serde(default = "default_thickness_01mm")]
    pub thickness: u16,
    #[serde(default = "default_thickness_mm")]
    pub thickness_mm: f64,
    #[serde(default)]
    pub feed_offset: i16,
    #[serde(default)]
    pub cut_sheet_feed_offset: i16,
    #[serde(default)]
    pub roll_paper_feed_offset: i16,
    #[serde(default)]
    pub drying_time: u8,
    #[serde(default = "default_one")]
    pub paper_width_check: u8,
    #[serde(default)]
    pub bottom_edge_priority: bool,
    #[serde(default)]
    pub rubbing_reduction: u16,
    pub icc_profile: String,
}

fn default_category_fine_art() -> u8 {
    4
}
fn default_platen_gap() -> u8 {
    21
}
fn default_thickness_01mm() -> u16 {
    48
}
fn default_thickness_mm() -> f64 {
    0.48
}
fn default_one() -> u8 {
    1
}

/// Transliterate German umlauts and build an ASCII slug capped at 32 characters for CUPS PPD compliance.
pub fn sanitize_ppd_media_key(slot: u8, media_name: &str) -> String {
    let mut translit = String::with_capacity(media_name.len() + 8);
    for ch in media_name.chars() {
        match ch {
            'ä' => translit.push_str("ae"),
            'ö' => translit.push_str("oe"),
            'ü' => translit.push_str("ue"),
            'Ä' => translit.push_str("Ae"),
            'Ö' => translit.push_str("Oe"),
            'Ü' => translit.push_str("Ue"),
            'ß' => translit.push_str("ss"),
            c if c.is_ascii_alphanumeric() => translit.push(c),
            _ => translit.push('_'),
        }
    }
    let mut collapsed = String::with_capacity(translit.len());
    let mut prev_underscore = true;
    for ch in translit.chars() {
        if ch == '_' {
            if !prev_underscore {
                collapsed.push('_');
            }
            prev_underscore = true;
        } else {
            collapsed.push(ch);
            prev_underscore = false;
        }
    }
    let slug = collapsed.trim_matches('_');
    let prefix = if (1..=30).contains(&slot) {
        format!("Slot{slot:02}_")
    } else {
        "EMX_".to_string()
    };
    let full = if slug.is_empty() {
        format!("Slot{slot:02}")
    } else {
        format!("{prefix}{slug}")
    };
    let truncated: String = full.chars().take(32).collect();
    truncated.trim_end_matches('_').to_string()
}
