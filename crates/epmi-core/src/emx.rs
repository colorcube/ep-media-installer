use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use flate2::read::DeflateDecoder;
use serde::{Deserialize, Serialize};

use crate::error::{EpmiError, Result};
use crate::media::{
    BASE_MEDIA_SPECS, CustomMediaEntry, PrinterMediaSlot, find_base_media_by_sn_id,
    sanitize_ppd_media_key,
};

/// Parsed representation of an Epson `.emx` media package (`.xml` + `.cpb` + `.icc` ZIP archive).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmxPackage {
    pub source_path: String,
    pub model_name: String,
    pub series_name: String,
    pub user_media_id: u32,
    pub vendor: String,
    pub media_name: String,
    pub media_full_name: String,
    pub base_sn_id: u8,
    pub mac_ppd_id: u16,
    pub win_drv_id: u16,
    pub media_category: u8,
    pub platen_gap: u8,
    pub thickness_01mm: u16,
    pub drying_time: u8,
    pub paper_width_check: u8,
    pub cut_sheet_feed_offset: i16,
    pub roll_paper_feed_offset: i16,
    pub ink_density: i16,
    pub icc_profile_name: String,
    #[serde(skip)]
    pub icc_profile_bytes: Vec<u8>,
}

impl EmxPackage {
    pub fn print_duty(&self) -> i8 {
        (self.ink_density - 100).clamp(-50, 50) as i8
    }

    pub fn thickness_mm(&self) -> f64 {
        (f64::from(self.thickness_01mm) / 100.0 * 100.0).round() / 100.0
    }

    pub fn base_media_key(&self) -> &'static str {
        find_base_media_by_sn_id(self.base_sn_id)
            .unwrap_or(&BASE_MEDIA_SPECS[5])
            .key
    }

    pub fn to_printer_slot(&self, slot: u8) -> PrinterMediaSlot {
        PrinterMediaSlot {
            slot,
            registered: true,
            media_name: self.media_name.chars().take(60).collect(),
            user_media_id: self.user_media_id,
            base_sn_id: self.base_sn_id,
            media_category: self.media_category,
            platen_gap: self.platen_gap,
            thickness_01mm: self.thickness_01mm,
            drying_time: self.drying_time,
            paper_width_check: self.paper_width_check,
            cut_sheet_feed_offset: self.cut_sheet_feed_offset,
            roll_paper_feed_offset: self.roll_paper_feed_offset,
            margin_sets: 0,
        }
    }

    pub fn to_custom_media_entry(&self, slot: u8) -> CustomMediaEntry {
        let base_spec =
            find_base_media_by_sn_id(self.base_sn_id).unwrap_or(&BASE_MEDIA_SPECS[5]);
        let key = sanitize_ppd_media_key(slot, &self.media_name);
        let disp_name = if (1..=30).contains(&slot) {
            format!("{} (Slot {})", self.media_name, slot)
        } else {
            self.media_name.clone()
        };
        let icc_profile = if self.icc_profile_name.is_empty() {
            base_spec.icc_profile.to_string()
        } else {
            self.icc_profile_name.clone()
        };

        CustomMediaEntry {
            slot,
            user_media_id: self.user_media_id,
            key,
            name: disp_name,
            media_name: self.media_name.clone(),
            vendor: self.vendor.clone(),
            base_media: base_spec.key.to_string(),
            base_sn_id: self.base_sn_id,
            media_category: self.media_category,
            print_duty: self.print_duty(),
            platen_gap: self.platen_gap,
            thickness: self.thickness_01mm,
            thickness_mm: self.thickness_mm(),
            feed_offset: self.cut_sheet_feed_offset,
            cut_sheet_feed_offset: self.cut_sheet_feed_offset,
            roll_paper_feed_offset: self.roll_paper_feed_offset,
            drying_time: self.drying_time,
            paper_width_check: self.paper_width_check,
            bottom_edge_priority: false,
            rubbing_reduction: 0,
            icc_profile,
        }
    }
}

/// Extract all files from a standard ZIP buffer using the End-of-Central-Directory (EOCD)
/// and Central Directory headers so even streaming/data-descriptor ZIP entries work reliably.
fn extract_zip_entries(zip_bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>> {
    if zip_bytes.len() < 22 {
        return Err(EpmiError::InvalidEmx("Archive smaller than 22 bytes".to_string()));
    }

    // Locate EOCD signature 0x06054b50 ("PK\x05\x06") from the end
    let min_eocd = zip_bytes.len().saturating_sub(65557);
    let mut eocd_pos = None;
    for i in (min_eocd..=zip_bytes.len().saturating_sub(22)).rev() {
        if zip_bytes.get(i..i + 4) == Some(b"PK\x05\x06") {
            eocd_pos = Some(i);
            break;
        }
    }
    let eocd = eocd_pos
        .ok_or_else(|| EpmiError::InvalidEmx("Missing ZIP EOCD signature".to_string()))?;

    let total_entries = usize::from(u16::from_le_bytes([
        zip_bytes[eocd + 10],
        zip_bytes[eocd + 11],
    ]));
    let cd_offset = usize::try_from(u32::from_le_bytes([
        zip_bytes[eocd + 16],
        zip_bytes[eocd + 17],
        zip_bytes[eocd + 18],
        zip_bytes[eocd + 19],
    ]))
    .map_err(|_| EpmiError::InvalidEmx("Central directory offset overflow".to_string()))?;

    let mut entries = HashMap::with_capacity(total_entries.min(64));
    let mut pos = cd_offset;

    for _ in 0..total_entries {
        let hdr = zip_bytes.get(pos..pos.saturating_add(46)).ok_or_else(|| {
            EpmiError::InvalidEmx("Truncated ZIP central directory header".to_string())
        })?;
        if &hdr[0..4] != b"PK\x01\x02" {
            return Err(EpmiError::InvalidEmx(
                "Invalid ZIP central directory signature".to_string(),
            ));
        }
        let method = u16::from_le_bytes([hdr[10], hdr[11]]);
        let comp_size = usize::try_from(u32::from_le_bytes([
            hdr[20], hdr[21], hdr[22], hdr[23],
        ]))
        .map_err(|_| EpmiError::InvalidEmx("ZIP compressed size overflow".to_string()))?;
        let uncomp_size = usize::try_from(u32::from_le_bytes([
            hdr[24], hdr[25], hdr[26], hdr[27],
        ]))
        .map_err(|_| EpmiError::InvalidEmx("ZIP uncompressed size overflow".to_string()))?;
        let name_len = usize::from(u16::from_le_bytes([hdr[28], hdr[29]]));
        let extra_len = usize::from(u16::from_le_bytes([hdr[30], hdr[31]]));
        let comment_len = usize::from(u16::from_le_bytes([hdr[32], hdr[33]]));
        let local_offset = usize::try_from(u32::from_le_bytes([
            hdr[42], hdr[43], hdr[44], hdr[45],
        ]))
        .map_err(|_| EpmiError::InvalidEmx("ZIP local header offset overflow".to_string()))?;

        let name_start = pos.saturating_add(46);
        let name_end = name_start.saturating_add(name_len);
        let name_bytes = zip_bytes.get(name_start..name_end).ok_or_else(|| {
            EpmiError::InvalidEmx("Truncated ZIP entry filename".to_string())
        })?;
        let name = String::from_utf8_lossy(name_bytes).to_string();
        pos = name_end.saturating_add(extra_len).saturating_add(comment_len);

        // Read local file header at local_offset ("PK\x03\x04")
        let lhdr = zip_bytes
            .get(local_offset..local_offset.saturating_add(30))
            .ok_or_else(|| EpmiError::InvalidEmx("Truncated ZIP local header".to_string()))?;
        if &lhdr[0..4] != b"PK\x03\x04" {
            return Err(EpmiError::InvalidEmx(
                "Invalid ZIP local file header signature".to_string(),
            ));
        }
        let lname_len = usize::from(u16::from_le_bytes([lhdr[26], lhdr[27]]));
        let lextra_len = usize::from(u16::from_le_bytes([lhdr[28], lhdr[29]]));
        let data_start = local_offset
            .saturating_add(30)
            .saturating_add(lname_len)
            .saturating_add(lextra_len);
        let data_end = data_start.saturating_add(comp_size);
        let comp_slice = zip_bytes.get(data_start..data_end).ok_or_else(|| {
            EpmiError::InvalidEmx("Truncated ZIP compressed payload".to_string())
        })?;

        if uncomp_size > 64 * 1024 * 1024 {
            return Err(EpmiError::InvalidEmx(
                "ZIP entry exceeds 64 MiB safety cap".to_string(),
            ));
        }

        let data = match method {
            0 => comp_slice.to_vec(),
            8 => {
                let mut decoder = DeflateDecoder::new(comp_slice);
                let mut out = Vec::with_capacity(uncomp_size.min(8 * 1024 * 1024));
                decoder.read_to_end(&mut out)?;
                out
            }
            other => {
                return Err(EpmiError::InvalidEmx(format!(
                    "Unsupported ZIP compression method {other}"
                )));
            }
        };
        entries.insert(name, data);
    }

    Ok(entries)
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open_prefix = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let start_idx = xml.find(&open_prefix)?;
    let after_open = &xml[start_idx + open_prefix.len()..];
    if after_open.starts_with("/>") {
        return Some(String::new());
    }
    let gt_idx = after_open.find('>')?;
    let content_start = &after_open[gt_idx + 1..];
    let end_idx = content_start.find(&close_tag)?;
    Some(content_start[..end_idx].trim().to_string())
}

fn parse_cpb_ini(cpb_text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw_line in cpb_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('[') || line.starts_with(';') || line.starts_with('#')
        {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

pub fn parse_emx_bytes(source_label: &str, zip_bytes: &[u8]) -> Result<EmxPackage> {
    let files = extract_zip_entries(zip_bytes)?;

    let mut xml_name = None;
    let mut cpb_name = None;
    let mut icc_names = Vec::new();
    for k in files.keys() {
        let lower = k.to_ascii_lowercase();
        if lower.ends_with(".xml") && xml_name.is_none() {
            xml_name = Some(k.clone());
        } else if lower.ends_with(".cpb") && cpb_name.is_none() {
            cpb_name = Some(k.clone());
        } else if lower.ends_with(".icc") || lower.ends_with(".icm") {
            icc_names.push(k.clone());
        }
    }

    let xml_key = xml_name.ok_or_else(|| EpmiError::InvalidEmx("Missing .xml in .emx".to_string()))?;
    let cpb_key = cpb_name.ok_or_else(|| EpmiError::InvalidEmx("Missing .cpb in .emx".to_string()))?;

    let xml_text = String::from_utf8_lossy(&files[&xml_key]).to_string();
    let cpb_text = String::from_utf8_lossy(&files[&cpb_key]).to_string();
    let cpb = parse_cpb_ini(&cpb_text);

    let model_name = extract_xml_tag(&xml_text, "Name").unwrap_or_else(|| "SC-P900 Series".to_string());
    let series_name = extract_xml_tag(&xml_text, "Series").unwrap_or_else(|| model_name.clone());

    let xml_media_id = extract_xml_tag(&xml_text, "ID")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let xml_vendor = extract_xml_tag(&xml_text, "Manufacturer").unwrap_or_else(|| "EPSON".to_string());
    let xml_media_name = extract_xml_tag(&xml_text, "MediaName").unwrap_or_else(|| "Custom_Media".to_string());
    let printer_media_id = extract_xml_tag(&xml_text, "PrinterMediaID")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(25);
    let mac_ppd_id = extract_xml_tag(&xml_text, "MacDrvMediaID")
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(53);
    let win_drv_id = extract_xml_tag(&xml_text, "WinDrvMediaID")
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let ink_density = extract_xml_tag(&xml_text, "InkDensity")
        .and_then(|s| s.parse::<i16>().ok())
        .unwrap_or(100);
    let mut icc_name_rgb = extract_xml_tag(&xml_text, "ICCProfileNameRGB")
        .or_else(|| extract_xml_tag(&xml_text, "ICCProfileName"))
        .unwrap_or_default();

    let media_name = cpb
        .get("MediaName")
        .cloned()
        .unwrap_or(xml_media_name);
    let media_full_name = cpb
        .get("MediaFullName")
        .cloned()
        .unwrap_or_else(|| media_name.clone());
    let vendor = cpb.get("Vendor").cloned().unwrap_or(xml_vendor);
    let user_media_id = cpb
        .get("MediaID")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(xml_media_id);
    let base_sn_id = cpb
        .get("LegacyBaseMedia")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(printer_media_id);
    let media_category = cpb
        .get("MediaCategory")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(4);
    let platen_gap = cpb
        .get("PlatenGap")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(15);
    let thickness_01mm = cpb
        .get("PaperThickness")
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(30);
    let drying_time = cpb
        .get("DryTime")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    let paper_width_check = cpb
        .get("MeasurementPaperWidth")
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(1);
    let cut_sheet_feed_offset = cpb
        .get("CutSheetFeedOffset")
        .and_then(|s| s.parse::<i16>().ok())
        .unwrap_or(0);
    let roll_paper_feed_offset = cpb
        .get("RollPaperFeedOffset")
        .and_then(|s| s.parse::<i16>().ok())
        .unwrap_or(0);

    let icc_member = if !icc_name_rgb.is_empty() && files.contains_key(&icc_name_rgb) {
        Some(icc_name_rgb.clone())
    } else if let Some(first_icc) = icc_names.first() {
        if icc_name_rgb.is_empty() {
            icc_name_rgb = Path::new(first_icc)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| first_icc.clone());
        }
        Some(first_icc.clone())
    } else {
        None
    };

    let icc_profile_bytes = icc_member
        .and_then(|m| files.get(&m).cloned())
        .unwrap_or_default();

    Ok(EmxPackage {
        source_path: source_label.to_string(),
        model_name,
        series_name,
        user_media_id,
        vendor,
        media_name,
        media_full_name,
        base_sn_id,
        mac_ppd_id,
        win_drv_id,
        media_category,
        platen_gap,
        thickness_01mm,
        drying_time,
        paper_width_check,
        cut_sheet_feed_offset,
        roll_paper_feed_offset,
        ink_density,
        icc_profile_name: icc_name_rgb,
        icc_profile_bytes,
    })
}

pub fn parse_emx_file(path: impl AsRef<Path>) -> Result<EmxPackage> {
    let p = path.as_ref();
    let bytes = fs::read(p)?;
    parse_emx_bytes(&p.to_string_lossy(), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_uncompressed_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cd = Vec::new();
        for &(name, data) in files {
            let local_offset = out.len() as u32;
            let name_bytes = name.as_bytes();
            let size = data.len() as u32;
            // Local file header ("PK\x03\x04")
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&0u16.to_le_bytes()); // method 0 = stored
            out.extend_from_slice(&0u16.to_le_bytes()); // time
            out.extend_from_slice(&0u16.to_le_bytes()); // date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc32
            out.extend_from_slice(&size.to_le_bytes()); // comp size
            out.extend_from_slice(&size.to_le_bytes()); // uncomp size
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data);

            // Central directory header ("PK\x01\x02")
            cd.extend_from_slice(b"PK\x01\x02");
            cd.extend_from_slice(&20u16.to_le_bytes()); // version made by
            cd.extend_from_slice(&20u16.to_le_bytes()); // version needed
            cd.extend_from_slice(&0u16.to_le_bytes()); // flags
            cd.extend_from_slice(&0u16.to_le_bytes()); // method 0 = stored
            cd.extend_from_slice(&0u16.to_le_bytes()); // time
            cd.extend_from_slice(&0u16.to_le_bytes()); // date
            cd.extend_from_slice(&0u32.to_le_bytes()); // crc32
            cd.extend_from_slice(&size.to_le_bytes()); // comp size
            cd.extend_from_slice(&size.to_le_bytes()); // uncomp size
            cd.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // extra len
            cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal attr
            cd.extend_from_slice(&0u32.to_le_bytes()); // external attr
            cd.extend_from_slice(&local_offset.to_le_bytes());
            cd.extend_from_slice(name_bytes);
        }
        let cd_offset = out.len() as u32;
        let cd_size = cd.len() as u32;
        let count = files.len() as u16;
        out.extend_from_slice(&cd);
        // EOCD ("PK\x05\x06")
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn test_parse_synthetic_emx_package() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<Model>
  <Name>SC-P900 Series</Name>
  <Series>SC-P900 Series</Series>
  <MediaInfo>
    <ID>230009156</ID>
    <Manufacturer xml:lang="en">EPSON</Manufacturer>
    <MediaName xml:lang="en">Fine Art Baryta</MediaName>
    <PrinterMediaID>174</PrinterMediaID>
    <MacDrvMediaID>157</MacDrvMediaID>
    <WinDrvMediaID>411</WinDrvMediaID>
    <ModeInfo>
      <InkDensity>100</InkDensity>
      <ICCProfileNameRGB>Sample_Baryta.icc</ICCProfileNameRGB>
    </ModeInfo>
  </MediaInfo>
</Model>"#;
        let cpb = b"[Setting01]\nMediaName=Fine Art Baryta\nVendor=EPSON\nMediaID=230009156\nLegacyBaseMedia=174\nMediaCategory=1\nPlatenGap=15\nPaperThickness=30\nDryTime=0\n";
        let icc = b"SAMPLE_ICC_BINARY_HEADER";
        let zip_bytes = build_uncompressed_zip(&[
            ("MediaInfo.xml", xml),
            ("Media.cpb", cpb),
            ("Sample_Baryta.icc", icc),
        ]);

        let res = parse_emx_bytes("synthetic.emx", &zip_bytes);
        assert!(res.is_ok());
        if let Ok(pkg) = res {
            assert_eq!(pkg.user_media_id, 230_009_156);
            assert_eq!(pkg.base_sn_id, 174);
            assert_eq!(pkg.base_media_key(), "Baryta");
            assert_eq!(pkg.mac_ppd_id, 157);
            assert_eq!(pkg.platen_gap, 15);
            assert_eq!(pkg.thickness_01mm, 30);
            assert_eq!(pkg.icc_profile_bytes, icc);
            let entry = pkg.to_custom_media_entry(6);
            assert_eq!(entry.slot, 6);
            assert_eq!(entry.user_media_id, 230_009_156);
            assert!(entry.key.len() <= 32);
        }
    }
}
