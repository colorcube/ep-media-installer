use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::emx::{EmxPackage, parse_emx_file};
use crate::error::Result;
use crate::media::{CustomMediaEntry, PrinterMediaSlot};
use crate::snmp::SnmpMediaClient;

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Default user-writable `custom_media.json` path (`~/.config/epson-p900-escpr/custom_media.json`).
pub fn default_user_custom_media_json() -> PathBuf {
    if let Some(env_path) = env::var_os("EPSON_P900_CUSTOM_MEDIA_JSON") {
        return PathBuf::from(env_path);
    }
    home_dir()
        .join(".config")
        .join("epson-p900-escpr")
        .join("custom_media.json")
}

/// Files the working copy is written to (only the user's `custom_media.json`; the system database
/// is written by the driver's privileged helper, see [`crate::system`]).
pub fn discover_sync_targets(primary_path: Option<&Path>) -> Vec<PathBuf> {
    vec![
        primary_path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_user_custom_media_json),
    ]
}

/// User ICC store for profiles of custom media (`~/.local/share/color/icc/epson-scp900`).
pub fn user_icc_dir() -> PathBuf {
    home_dir()
        .join(".local")
        .join("share")
        .join("color")
        .join("icc")
        .join("epson-scp900")
}

pub fn default_icc_output_dirs() -> Vec<PathBuf> {
    vec![user_icc_dir()]
}

pub fn load_custom_media_json(path: &Path) -> Vec<CustomMediaEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<CustomMediaEntry>>(&text).unwrap_or_default()
}

pub fn save_custom_media_json(path: &Path, entries: &[CustomMediaEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut json = serde_json::to_string_pretty(entries)?;
    json.push('\n');
    fs::write(path, json)?;
    Ok(())
}

/// Merge a list of live registered `PrinterMediaSlot`s (`1..=30`) with existing `CustomMediaEntry`
/// records (preserving custom ICC paths, `print_duty`, and unslotted `slot == 0` entries).
pub fn merge_slots_with_existing(
    registered_slots: &[PrinterMediaSlot],
    existing: &[CustomMediaEntry],
) -> Vec<CustomMediaEntry> {
    let mut by_media_id: HashMap<u32, &CustomMediaEntry> = HashMap::new();
    let mut by_slot: HashMap<u8, &CustomMediaEntry> = HashMap::new();
    for e in existing {
        if e.user_media_id > 0 {
            by_media_id.insert(e.user_media_id, e);
        }
        if e.slot > 0 {
            by_slot.insert(e.slot, e);
        }
    }

    let mut updated = Vec::with_capacity(registered_slots.len() + existing.len());
    let mut seen_ids = HashSet::new();

    for s in registered_slots {
        if !s.registered {
            continue;
        }
        let prev = by_media_id
            .get(&s.user_media_id)
            .or_else(|| by_slot.get(&s.slot))
            .copied();
        let entry = s.to_custom_media_entry(prev);
        seen_ids.insert(s.user_media_id);
        updated.push(entry);
    }

    for e in existing {
        if e.slot == 0 && !seen_ids.contains(&e.user_media_id) {
            updated.push(e.clone());
        }
    }

    updated
}

/// Synchronize active NVRAM slots (`1..=30`) from the printer at `host` into `custom_media.json`.
pub fn sync_printer_slots_to_json(
    client: &SnmpMediaClient,
    json_path: Option<&Path>,
) -> Result<(Vec<PathBuf>, Vec<CustomMediaEntry>)> {
    let registered_slots = client.read_all_slots(true)?;
    let targets = discover_sync_targets(json_path);

    let mut existing = Vec::new();
    for t in &targets {
        let loaded = load_custom_media_json(t);
        if !loaded.is_empty() {
            existing = loaded;
            break;
        }
    }

    let updated = merge_slots_with_existing(&registered_slots, &existing);
    let mut written_paths = Vec::new();
    for t in targets {
        if save_custom_media_json(&t, &updated).is_ok() {
            written_paths.push(t);
        }
    }
    Ok((written_paths, updated))
}

/// Import an `.emx` file:
/// 1. Extract its `.icc` profile into `~/.local/share/color/icc/epson-scp900/`.
/// 2. Optionally write its mechanical parameters into `slot` (`1..=30`) on the printer.
/// 3. Update the working copy `custom_media.json` (install system-wide with [`crate::system`]).
pub fn import_emx_package(
    emx_path: impl AsRef<Path>,
    slot: u8,
    json_path: Option<&Path>,
    printer_client: Option<&SnmpMediaClient>,
) -> Result<(EmxPackage, CustomMediaEntry, Vec<PathBuf>)> {
    let pkg = parse_emx_file(emx_path)?;

    let mut saved_icc_paths = Vec::new();
    if !pkg.icc_profile_name.is_empty() && !pkg.icc_profile_bytes.is_empty() {
        for dir in default_icc_output_dirs() {
            if fs::create_dir_all(&dir).is_ok() {
                let out_icc = dir.join(&pkg.icc_profile_name);
                if fs::write(&out_icc, &pkg.icc_profile_bytes).is_ok() {
                    saved_icc_paths.push(out_icc);
                }
            }
        }
    }

    if let Some(client) = printer_client
        && (1..=30).contains(&slot)
    {
        let slot_obj = pkg.to_printer_slot(slot);
        client.write_slot(&slot_obj)?;
    }

    let entry = pkg.to_custom_media_entry(slot);
    let targets = discover_sync_targets(json_path);
    let mut entries = Vec::new();
    for t in &targets {
        let loaded = load_custom_media_json(t);
        if !loaded.is_empty() {
            entries = loaded;
            break;
        }
    }

    let mut replaced = false;
    for existing in &mut entries {
        if (slot > 0 && existing.slot == slot)
            || (pkg.user_media_id > 0 && existing.user_media_id == pkg.user_media_id)
        {
            *existing = entry.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        entries.push(entry.clone());
    }

    for t in &targets {
        let _ = save_custom_media_json(t, &entries);
    }

    Ok((pkg, entry, saved_icc_paths))
}
