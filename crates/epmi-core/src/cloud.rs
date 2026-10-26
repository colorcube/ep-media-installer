use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{EpmiError, Result};
use crate::media::media_category_label;

pub const CMS_BASE_URL: &str = "https://support.epson.biz/lfp/cms/v2/";

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
        "37" => "SiHL",
        "128" => "FRUBO",
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

/// Query Epson's LFP Content Server (`search`) for available media and `.emx` packages
/// for `scp9xx` (SC-P900) or `scp7xx` (SC-P700).
///
/// Note on `region`:
/// Epson's CMS uses a `RegionTypes` bitmask:
/// - `4` = `All of Europe` (`eeb_all`)
/// - `8` = `Western Europe` (`eeb_all` + `eeb_w`, recommended default for `DE`)
/// - `1` = `North America` (`eai_n`)
/// - `256` = `Japan` (`ehc`, with `country="JP"`, `locale="ja"`)
pub fn cloud_list_media(
    printer: &str,
    country: &str,
    region: u32,
    locale: &str,
) -> Result<Vec<CloudMediaRecord>> {
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
    let params = [
        ("printer", printer),
        ("country", country_norm.as_str()),
        ("region", region_str.as_str()),
        ("locale", locale),
        ("category", "1"),
        ("media_mfr", ""),
        ("media_type", ""),
        ("media_group", ""),
        ("emx_id", ""),
        ("limit", "100"),
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
            if s.is_empty() {
                vendor_name_by_id(&mfr_id).to_string()
            } else {
                s
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
