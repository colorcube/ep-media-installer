use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{EpmiError, Result};
use crate::media::media_category_label;

pub const CMS_BASE_URL: &str = "https://support.epson.biz/lfp/cms/v2/";

/// Supported Epson SureColor printer series in the online `.emx` media catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CloudPrinterModel {
    /// Cloud API series code (e.g. `"scp9xx"`, `"scp7xx"`, `"scp53xx"`).
    pub code: &'static str,
    /// User-facing model name (e.g. `"SureColor SC-P900 Series"`).
    pub label: &'static str,
    /// Product line category (`"Photo / Fine Art"` or `"Technical / CAD"`).
    pub category: &'static str,
    /// Substrings used for automatic matching from SNMP / CUPS model strings.
    pub match_patterns: &'static [&'static str],
}

/// Complete catalog of all 27 Epson SureColor printer series supported by the LFP Cloud Content Server.
pub const CLOUD_PRINTER_MODELS: &[CloudPrinterModel] = &[
    // Photo / Fine Art / Proofing (SC-P Series)
    CloudPrinterModel {
        code: "scp9xx",
        label: "SureColor SC-P900 Series (SC-P900 / SC-PX1VL)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P900", "P900", "SC-PX1VL", "PX1VL", "scp9xx"],
    },
    CloudPrinterModel {
        code: "scp7xx",
        label: "SureColor SC-P700 Series (SC-P700 / SC-PX1V)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P700", "P700", "SC-PX1V", "PX1V", "scp7xx"],
    },
    CloudPrinterModel {
        code: "scp53xx",
        label: "SureColor SC-P5300 Series (SC-P5300 / SC-P5370)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P5300", "SC-P5370", "P5300", "P5370", "scp53xx"],
    },
    CloudPrinterModel {
        code: "scp65xx",
        label: "SureColor SC-P6500 Series",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P6500 ", "SC-P6500E", "scp65xx"],
    },
    CloudPrinterModel {
        code: "scp65xxD",
        label: "SureColor SC-P6500D Series",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P6500D", "SC-P6570D", "scp65xxD"],
    },
    CloudPrinterModel {
        code: "scp73xx",
        label: "SureColor SC-P7300 Series (SC-P7300 / SC-P7370)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P7300", "SC-P7370", "P7300", "scp73xx"],
    },
    CloudPrinterModel {
        code: "scp75xx",
        label: "SureColor SC-P7500 Series (SC-P7500 / SC-P7570)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P7500", "SC-P7570", "P7500", "scp75xx"],
    },
    CloudPrinterModel {
        code: "scp85xxD",
        label: "SureColor SC-P8500D Series",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P8500D ", "SC-P8570D", "scp85xxD"],
    },
    CloudPrinterModel {
        code: "scp85xxDL",
        label: "SureColor SC-P8500DL Series",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P8500DL", "SC-P8570DL", "scp85xxDL"],
    },
    CloudPrinterModel {
        code: "scp85xxDM",
        label: "SureColor SC-P8500DM Series",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P8500DM", "scp85xxDM"],
    },
    CloudPrinterModel {
        code: "scp93xx",
        label: "SureColor SC-P9300 Series (SC-P9300 / SC-P9370)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P9300", "SC-P9370", "P9300", "scp93xx"],
    },
    CloudPrinterModel {
        code: "scp95xx",
        label: "SureColor SC-P9500 Series (SC-P9500 / SC-P9570)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P9500", "SC-P9570", "P9500", "scp95xx"],
    },
    CloudPrinterModel {
        code: "scp110xxD",
        label: "SureColor SC-P11000D Series",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P11000", "P11000", "scp110xxD"],
    },
    CloudPrinterModel {
        code: "scp205xx",
        label: "SureColor SC-P20500 Series (SC-P20500 / SC-P20570)",
        category: "Photo / Fine Art",
        match_patterns: &["SC-P20500", "SC-P20570", "P20500", "scp205xx"],
    },
    // Technical / CAD (SC-T Series)
    CloudPrinterModel {
        code: "sct37xx",
        label: "SureColor SC-T3700 Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T3700 ", "SC-T3700E", "SC-T3770E", "sct37xx"],
    },
    CloudPrinterModel {
        code: "sct37xxD",
        label: "SureColor SC-T3700D Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T3700D", "SC-T3770D", "sct37xxD"],
    },
    CloudPrinterModel {
        code: "sct56xxD",
        label: "SureColor SC-T5600D Series (CMYK)",
        category: "Technical / CAD",
        match_patterns: &["SC-T5600D", "sct56xxD"],
    },
    CloudPrinterModel {
        code: "sct56xxDLcLm",
        label: "SureColor SC-T5600D Series (CMYKLcLm)",
        category: "Technical / CAD",
        match_patterns: &["sct56xxDLcLm"],
    },
    CloudPrinterModel {
        code: "sct56xxDM",
        label: "SureColor SC-T5600DM Series (CMYK)",
        category: "Technical / CAD",
        match_patterns: &["SC-T5600DM", "sct56xxDM"],
    },
    CloudPrinterModel {
        code: "sct56xxDMLcLm",
        label: "SureColor SC-T5600DM Series (CMYKLcLm)",
        category: "Technical / CAD",
        match_patterns: &["sct56xxDMLcLm"],
    },
    CloudPrinterModel {
        code: "sct57xxD",
        label: "SureColor SC-T5700D Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T5700D", "SC-T5770D", "sct57xxD"],
    },
    CloudPrinterModel {
        code: "sct57xxDM",
        label: "SureColor SC-T5700DM Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T5700DM", "SC-T5770DM", "sct57xxDM"],
    },
    CloudPrinterModel {
        code: "sct76xxD",
        label: "SureColor SC-T7600D Series (CMYK)",
        category: "Technical / CAD",
        match_patterns: &["SC-T7600D", "sct76xxD"],
    },
    CloudPrinterModel {
        code: "sct76xxDLcLm",
        label: "SureColor SC-T7600D Series (CMYKLcLm)",
        category: "Technical / CAD",
        match_patterns: &["sct76xxDLcLm"],
    },
    CloudPrinterModel {
        code: "sct77xxD",
        label: "SureColor SC-T7700D Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T7700D ", "SC-T7770D", "sct77xxD"],
    },
    CloudPrinterModel {
        code: "sct77xxDL",
        label: "SureColor SC-T7700DL Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T7700DL", "SC-T7770DL", "sct77xxDL"],
    },
    CloudPrinterModel {
        code: "sct77xxDM",
        label: "SureColor SC-T7700DM Series",
        category: "Technical / CAD",
        match_patterns: &["SC-T7700DM", "SC-T7770DM", "sct77xxDM"],
    },
];

/// Look up a `CloudPrinterModel` by its series code or model name substring.
pub fn match_printer_to_cloud_series(model_or_code: &str) -> Option<&'static CloudPrinterModel> {
    let q = model_or_code.trim();
    if q.is_empty() {
        return None;
    }
    if let Some(exact) = CLOUD_PRINTER_MODELS
        .iter()
        .find(|m| m.code.eq_ignore_ascii_case(q))
    {
        return Some(exact);
    }
    let upper = q.to_ascii_uppercase();
    // Check more specific patterns (longer pattern first) so e.g. SC-P8500DL matches before SC-P8500D
    let mut best: Option<(&'static CloudPrinterModel, usize)> = None;
    for model in CLOUD_PRINTER_MODELS {
        for &pat in model.match_patterns {
            let pat_upper = pat.trim().to_ascii_uppercase();
            if upper.contains(&pat_upper) {
                let len = pat_upper.len();
                if best.is_none_or(|(_, best_len)| len > best_len) {
                    best = Some((model, len));
                }
            }
        }
    }
    best.map(|(m, _)| m)
}

/// Record returned by Epson's LFP Cloud Content Server (`search` endpoint).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudMediaRecord {
    pub id: String,
    pub region_key: String,
    pub media_manufacturer_id: String,
    pub media_manufacturer_name: String,
    pub media_type_id: String,
    pub media_type_name: String,
    pub media_group_id: String,
    pub media_group_name: String,
    pub media_shape_id: String,
    pub emx_id: String,
    pub version: String,
    pub revision: String,
    pub updated: String,
    pub url: String,
    pub file_name: String,
    pub size: u64,
}

impl CloudMediaRecord {
    /// Returns `true` if this record has a downloadable `.emx` package URL.
    pub fn has_downloadable_emx(&self) -> bool {
        !self.emx_id.is_empty() && !self.url.is_empty()
    }

    /// Returns `true` if this record is a custom/add-on `.emx` package (e.g. `230009xxx`)
    /// rather than a factory preset (`102000xxx`).
    pub fn is_custom_emx(&self) -> bool {
        self.has_downloadable_emx() && !self.emx_id.starts_with("102000")
    }
}

pub fn vendor_name_by_id(mfr_id: &str) -> &'static str {
    match mfr_id.trim() {
        "9" => "EPSON",
        "12" => "Canson",
        "26" => "Felix Schoeller",
        "32" => "Harman",
        "35" => "Mitsubishi",
        "37" => "SiHL",
        "43" => "Somerset",
        "128" => "FRUBO",
        "129" => "Awagami Factory",
        "130" => "Hahnemühle",
        "131" => "ILFORD",
        "132" => "IseWashi",
        "149" => "PICTORICO",
        "150" => "GEKKO",
        _ => "EPSON",
    }
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                let hi = (b >> 4) & 0x0F;
                let lo = b & 0x0F;
                out.push(char::from(if hi < 10 { b'0' + hi } else { b'A' + (hi - 10) }));
                out.push(char::from(if lo < 10 { b'0' + lo } else { b'A' + (lo - 10) }));
            }
        }
    }
    out
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "25", "-A", "epmi/0.1", url])
        .output()
        .map_err(|e| EpmiError::Cloud(format!("Failed to execute curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(EpmiError::Cloud(format!(
            "HTTP GET failed for {url}: {}",
            stderr.trim()
        )));
    }
    Ok(output.stdout)
}

fn query_cms_jsonp(endpoint: &str, params: &[(&str, &str)]) -> Result<Value> {
    let mut qs = String::from("callback=__jsonp_callback__");
    for (k, v) in params {
        qs.push('&');
        qs.push_str(&percent_encode(k));
        qs.push('=');
        qs.push_str(&percent_encode(v));
    }
    let url = format!("{CMS_BASE_URL}{endpoint}?{qs}");
    let raw = http_get_bytes(&url)?;
    let text = String::from_utf8_lossy(&raw);
    let trimmed = text.trim();
    let json_slice = match (trimmed.find('('), trimmed.rfind(')')) {
        (Some(start), Some(end)) if start < end => &trimmed[start + 1..end],
        _ => trimmed,
    };
    let val: Value = serde_json::from_str(json_slice)?;
    Ok(val)
}

fn val_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn extract_file_name_from_url(url: &str, emx_id: &str) -> String {
    if let Some((_, query)) = url.split_once('?') {
        for pair in query.split('&') {
            if let Some(("file", val)) = pair.split_once('=')
                && !val.trim().is_empty()
            {
                return val.trim().to_string();
            }
        }
    }
    if emx_id.is_empty() {
        String::from("media.emx")
    } else {
        format!("{emx_id}.emx")
    }
}

/// Optional helper to fetch dynamic vendor ID -> name mappings from the `media` endpoint.
fn fetch_dynamic_vendor_map(printer: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(root) = query_cms_jsonp("media", &[("printer", printer)]) else {
        return map;
    };
    if let Some(vendors) = root.get("mediaVendors").and_then(Value::as_array) {
        for v in vendors {
            let id = val_to_string(v.get("id"));
            let name = val_to_string(v.get("name"));
            if !id.is_empty() && !name.is_empty() {
                map.insert(id, name);
            }
        }
    }
    map
}

/// Query Epson's LFP Content Server (`search`) for available media and `.emx` packages
/// for any supported printer series (e.g. `scp9xx`, `scp7xx`, `scp53xx`, `scp75xx`, `scp95xx`, `sct37xx`, ...).
pub fn cloud_list_media(
    printer: &str,
    country: &str,
    region: u32,
    locale: &str,
) -> Result<Vec<CloudMediaRecord>> {
    // Allow passing either a series code ("scp9xx") or a model name ("SC-P900")
    let resolved_printer = match_printer_to_cloud_series(printer)
        .map(|m| m.code)
        .unwrap_or_else(|| printer.trim());

    let country_norm = if country.trim().is_empty() {
        String::from("DE")
    } else {
        country.trim().to_ascii_uppercase()
    };
    let region_norm = if region == 0 || (country_norm == "DE" && region < 4) {
        8
    } else {
        region
    };
    let region_str = region_norm.to_string();
    let vendor_map = fetch_dynamic_vendor_map(resolved_printer);

    let params = [
        ("printer", resolved_printer),
        ("country", country_norm.as_str()),
        ("region", region_str.as_str()),
        ("locale", locale),
        ("category", "1"),
        ("media_mfr", ""),
        ("media_type", ""),
        ("media_group", ""),
        ("emx_id", ""),
        ("limit", "200"),
        ("offset", "0"),
        ("app", "6"),
    ];

    let root = query_cms_jsonp("search", &params)?;
    let Some(records_arr) = root.get("records").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(records_arr.len());
    for item in records_arr {
        let size = item
            .get("size")
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
            })
            .unwrap_or(0);

        let mfr_id = {
            let s = val_to_string(item.get("media_mfr"));
            if s.is_empty() {
                val_to_string(item.get("media_manufacturer_id"))
            } else {
                s
            }
        };
        let mfr_name = {
            let s = val_to_string(item.get("media_manufacturer_name"));
            if !s.is_empty() {
                s
            } else if let Some(dyn_name) = vendor_map.get(&mfr_id) {
                dyn_name.clone()
            } else {
                vendor_name_by_id(&mfr_id).to_string()
            }
        };

        let title = {
            let t = val_to_string(item.get("title"));
            if !t.is_empty() {
                t
            } else if let Some(detail) = item
                .get("emx_infos")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(|info| info.get("detail"))
                .and_then(Value::as_str)
            {
                detail.to_string()
            } else {
                val_to_string(item.get("media_type_name"))
            }
        };

        let group_id = val_to_string(item.get("media_group"));
        let group_num = group_id.parse::<u8>().unwrap_or(0);
        let group_name = if group_num > 0 {
            media_category_label(group_num).to_string()
        } else {
            String::from("-")
        };

        let emx_id = val_to_string(item.get("emx_id"));
        let url = val_to_string(item.get("url"));
        let file_name = extract_file_name_from_url(&url, &emx_id);
        let version = val_to_string(item.get("version"));
        let revision = val_to_string(item.get("revision"));
        let ver_display = match (version.is_empty(), revision.is_empty()) {
            (false, false) => format!("v{version} (Rev {revision})"),
            (false, true) => format!("v{version}"),
            (true, false) => format!("Rev {revision}"),
            (true, true) => String::from("1"),
        };

        let updated_raw = val_to_string(item.get("emx_updated"));
        let updated = updated_raw
            .split('T')
            .next()
            .unwrap_or(updated_raw.as_str())
            .to_string();

        out.push(CloudMediaRecord {
            id: val_to_string(item.get("id")),
            region_key: val_to_string(item.get("region")),
            media_manufacturer_id: mfr_id,
            media_manufacturer_name: mfr_name,
            media_type_id: val_to_string(item.get("media_type")),
            media_type_name: title,
            media_group_id: group_id,
            media_group_name: group_name,
            media_shape_id: val_to_string(item.get("media_shape_id")),
            emx_id,
            version: ver_display,
            revision,
            updated,
            url,
            file_name,
            size,
        });
    }
    Ok(out)
}

/// Download an `.emx` package from a `CloudMediaRecord` into `out_dir`.
pub fn cloud_download_emx(record: &CloudMediaRecord, out_dir: impl AsRef<Path>) -> Result<PathBuf> {
    if record.url.is_empty() {
        return Err(EpmiError::Cloud(format!(
            "Record '{}' (emx_id={}) has no download URL",
            record.media_type_name, record.emx_id
        )));
    }

    let safe_url = record.url.replace(' ', "%20");
    let file_name = if record.file_name.is_empty() {
        extract_file_name_from_url(&record.url, &record.emx_id)
    } else {
        record.file_name.clone()
    };

    let dir = out_dir.as_ref();
    fs::create_dir_all(dir)?;
    let dest = dir.join(file_name);
    let bytes = http_get_bytes(&safe_url)?;
    fs::write(&dest, bytes)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_printer_to_cloud_series() {
        assert_eq!(
            match_printer_to_cloud_series("EPSON SC-P900 Series").map(|m| m.code),
            Some("scp9xx")
        );
        assert_eq!(
            match_printer_to_cloud_series("Epson SureColor SC-P700").map(|m| m.code),
            Some("scp7xx")
        );
        assert_eq!(
            match_printer_to_cloud_series("SC-P5300 Series").map(|m| m.code),
            Some("scp53xx")
        );
        assert_eq!(
            match_printer_to_cloud_series("SC-P8500DL Series").map(|m| m.code),
            Some("scp85xxDL")
        );
        assert_eq!(
            match_printer_to_cloud_series("SC-T5700DM Series").map(|m| m.code),
            Some("sct57xxDM")
        );
    }
}
