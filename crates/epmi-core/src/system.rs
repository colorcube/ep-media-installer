//! System-wide installation of custom media ("Für das System installieren").
//!
//! Mirrors Epson's Windows/macOS design: the printer's NVRAM slots hold the mechanical settings,
//! every computer needs the paper list and ICC profiles locally. On Linux the driver package ships a
//! privileged helper (`epson-p900-media-install`, started via `pkexec`) that writes
//! [`SYSTEM_CUSTOM_MEDIA_JSON`], installs the profiles to [`SYSTEM_CUSTOM_ICC_DIR`] and refreshes the
//! PPDs of all CUPS queues of the driver. This module prepares the staging directory for it, keeps
//! the user's working copy in sync with the printer, fetches profiles of Epson cloud media
//! automatically, and exports/imports custom media between computers (`.epmi` archive).
//!
//! SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cloud::{CloudMediaRecord, cloud_download_emx};
use crate::emx::{extract_zip_entries, parse_emx_file};
use crate::error::{EpmiError, Result};
use crate::media::{BASE_MEDIA_SPECS, CustomMediaEntry, PrinterMediaSlot};
use crate::sync::{home_dir, user_icc_dir};

/// System media database read by the CUPS filter, the PPD generator and LumaPrint.
pub const SYSTEM_CUSTOM_MEDIA_JSON: &str = "/etc/epson-p900-escpr-cups/custom_media.json";
/// ICC profiles of custom media, installed by the helper.
pub const SYSTEM_CUSTOM_ICC_DIR: &str = "/usr/local/share/color/icc/epson-p900-custom";
/// Privileged helper of the `epson-p900-escpr-cups` driver package.
pub const MEDIA_INSTALL_HELPER: &str =
    "/usr/libexec/epson-p900-escpr-cups/epson-p900-media-install";
/// Epson factory profiles installed by the driver (base media profiles).
pub const BUILTIN_ICC_DIR: &str = "/usr/share/color/icc/epson-scp900";

/// Paper list currently installed on this computer (empty if none).
pub fn load_system_media() -> Vec<CustomMediaEntry> {
    crate::sync::load_custom_media_json(Path::new(SYSTEM_CUSTOM_MEDIA_JSON))
}

/// Where the ICC profile of an entry comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSource {
    /// A profile file for this paper (absolute path, user store or installed custom profile).
    Own(PathBuf),
    /// Only the Epson profile of the base medium (no paper-specific profile assigned).
    BaseOnly(String),
    /// The named profile cannot be found.
    Missing(String),
}

/// Resolves `entry.icc_profile` against the user ICC store, the installed custom profiles and the
/// Epson factory profiles.
pub fn resolve_profile(entry: &CustomMediaEntry) -> ProfileSource {
    let raw = entry.icc_profile.trim();
    if raw.is_empty() {
        return ProfileSource::Missing(String::new());
    }
    let is_base = BASE_MEDIA_SPECS.iter().any(|b| b.icc_profile == raw);
    if is_base {
        return ProfileSource::BaseOnly(raw.to_string());
    }
    let direct = PathBuf::from(raw);
    if direct.is_absolute() && direct.is_file() {
        return ProfileSource::Own(direct);
    }
    let name = Path::new(raw)
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_default();
    for dir in [
        user_icc_dir(),
        PathBuf::from(SYSTEM_CUSTOM_ICC_DIR),
        PathBuf::from(BUILTIN_ICC_DIR),
    ] {
        let p = dir.join(&name);
        if p.is_file() {
            return ProfileSource::Own(p);
        }
    }
    ProfileSource::Missing(raw.to_string())
}

/// Installation state of one paper on this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    /// Installed system-wide with the same settings and profile.
    Installed,
    /// Installed, but the working copy differs (settings or profile).
    Changed,
    /// Not installed system-wide yet.
    NotInstalled,
}

impl SystemState {
    pub fn label_de(self) -> &'static str {
        match self {
            Self::Installed => "Im System",
            Self::Changed => "Geändert – erneut installieren",
            Self::NotInstalled => "Nicht installiert",
        }
    }
}

fn same_settings(a: &CustomMediaEntry, b: &CustomMediaEntry) -> bool {
    // Compare like the helper stores it (sanitized file name, absolute system path).
    let icc_name = |e: &CustomMediaEntry| {
        system_icc_file_name(
            &Path::new(e.icc_profile.trim())
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    };
    a.key == b.key
        && a.name == b.name
        && a.base_sn_id == b.base_sn_id
        && a.user_media_id == b.user_media_id
        && a.print_duty == b.print_duty
        && a.drying_time == b.drying_time
        && a.bottom_edge_priority == b.bottom_edge_priority
        && a.rubbing_reduction == b.rubbing_reduction
        && icc_name(a) == icc_name(b)
}

/// Compares a working-copy entry with the system database.
pub fn system_state(entry: &CustomMediaEntry, system: &[CustomMediaEntry]) -> SystemState {
    match system
        .iter()
        .find(|s| s.slot == entry.slot && s.user_media_id == entry.user_media_id)
    {
        Some(s) if same_settings(entry, s) => SystemState::Installed,
        Some(_) => SystemState::Changed,
        None => SystemState::NotInstalled,
    }
}

fn emx_cache_dir() -> PathBuf {
    home_dir().join(".cache").join("epmi").join("emx")
}

/// For every entry without an own profile whose media ID is an Epson cloud package
/// (`emx_id == user_media_id`), downloads the `.emx` (cached in `~/.cache/epmi/emx/`), stores its
/// ICC profile in the user ICC store and takes over profile and ink density. Returns the media
/// names that were completed.
pub fn fetch_cloud_profiles(
    entries: &mut [CustomMediaEntry],
    cloud: &[CloudMediaRecord],
) -> Result<Vec<String>> {
    let by_id: HashMap<&str, &CloudMediaRecord> = cloud
        .iter()
        .filter(|r| r.has_downloadable_emx())
        .map(|r| (r.emx_id.as_str(), r))
        .collect();
    let mut completed = Vec::new();
    for entry in entries.iter_mut() {
        if matches!(resolve_profile(entry), ProfileSource::Own(_)) || entry.user_media_id == 0 {
            continue;
        }
        let Some(record) = by_id.get(entry.user_media_id.to_string().as_str()) else {
            continue;
        };
        let cache = emx_cache_dir();
        let cached = cache.join(&record.file_name);
        let emx_path = if cached.is_file() {
            cached
        } else {
            cloud_download_emx(record, &cache)?
        };
        let pkg = parse_emx_file(&emx_path)?;
        if pkg.user_media_id != entry.user_media_id || pkg.icc_profile_bytes.is_empty() {
            continue; // not the package of this slot, or no profile inside
        }
        let store = user_icc_dir();
        fs::create_dir_all(&store)?;
        fs::write(store.join(&pkg.icc_profile_name), &pkg.icc_profile_bytes)?;
        entry.icc_profile = pkg.icc_profile_name.clone();
        entry.print_duty = pkg.print_duty();
        completed.push(entry.media_name.clone());
    }
    Ok(completed)
}

/// Copies `icc_file` into the user ICC store and assigns it to `entry`.
pub fn assign_profile(entry: &mut CustomMediaEntry, icc_file: &Path) -> Result<()> {
    let data = fs::read(icc_file)?;
    if data.get(36..40) != Some(b"acsp".as_slice()) {
        return Err(EpmiError::InvalidEmx(format!(
            "{} is not an ICC profile",
            icc_file.display()
        )));
    }
    let name = icc_file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| EpmiError::InvalidEmx("ICC file without name".to_string()))?;
    let store = user_icc_dir();
    fs::create_dir_all(&store)?;
    fs::write(store.join(&name), data)?;
    entry.icc_profile = name;
    Ok(())
}

/// File name accepted by the driver's helper (`[A-Za-z0-9._-]`, e.g. Epson's
/// "Traditional Photo PaperV2.icc" becomes "Traditional_Photo_PaperV2.icc").
pub fn system_icc_file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string()
}

/// Creates a private staging directory (`custom_media.json` + `icc/`) for the helper. Own profiles
/// are copied into `icc/` and referenced by file name; base profiles keep their Epson name.
pub fn stage_media(entries: &[CustomMediaEntry]) -> Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("epmi-staging-{}-{nanos}", std::process::id()));
    fs::create_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    let icc_dir = dir.join("icc");
    fs::create_dir(&icc_dir)?;

    let mut staged = Vec::with_capacity(entries.len());
    for entry in entries.iter().filter(|e| (1..=30).contains(&e.slot)) {
        let mut e = entry.clone();
        match resolve_profile(entry) {
            ProfileSource::Own(path) => {
                let name = system_icc_file_name(
                    &path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                fs::copy(&path, icc_dir.join(&name))?;
                e.icc_profile = name;
            }
            ProfileSource::BaseOnly(name) => e.icc_profile = name,
            ProfileSource::Missing(name) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(EpmiError::InvalidEmx(format!(
                    "Farbprofil '{name}' für '{}' nicht gefunden",
                    e.name
                )));
            }
        }
        staged.push(e);
    }
    let mut json = serde_json::to_string_pretty(&staged)?;
    json.push('\n');
    fs::write(dir.join("custom_media.json"), json)?;
    Ok(dir)
}

fn run_helper(args: &[&str]) -> Result<String> {
    if !Path::new(MEDIA_INSTALL_HELPER).is_file() {
        return Err(EpmiError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "{MEDIA_INSTALL_HELPER} fehlt – bitte den Druckertreiber epson-p900-escpr-cups installieren"
            ),
        )));
    }
    let out = Command::new("pkexec")
        .arg(MEDIA_INSTALL_HELPER)
        .args(args)
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    match out.status.code() {
        Some(0) => Ok(stdout),
        // pkexec: 126 = authorization dismissed, 127 = not authorized
        Some(126) | Some(127) => Err(EpmiError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Anmeldung abgebrochen oder nicht berechtigt",
        ))),
        _ => Err(EpmiError::Io(std::io::Error::other(if stderr.is_empty() {
            stdout
        } else {
            stderr
        }))),
    }
}

/// Installs `entries` (slots `1..=30`) as the complete system paper list via `pkexec` and the
/// driver's helper. Returns the helper's report.
pub fn install_system_media(entries: &[CustomMediaEntry]) -> Result<String> {
    let dir = stage_media(entries)?;
    let dir_str = dir.display().to_string();
    let result = run_helper(&[dir_str.as_str()]);
    let _ = fs::remove_dir_all(&dir);
    result
}

/// Removes one paper (by key) from the system database via `pkexec`.
pub fn remove_system_media(key: &str) -> Result<String> {
    run_helper(&["--remove", key])
}

// --- Export / import between computers (`.epmi` = ZIP with custom_media.json + icc/) ---

fn crc32(data: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(data);
    crc.sum()
}

/// Minimal ZIP writer (stored entries, no compression).
fn write_zip(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>> {
    let too_big = || EpmiError::InvalidEmx("Export zu groß".to_string());
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = u32::try_from(out.len()).map_err(|_| too_big())?;
        let size = u32::try_from(data.len()).map_err(|_| too_big())?;
        let name_len = u16::try_from(name.len()).map_err(|_| too_big())?;
        let crc = crc32(data);
        // local file header
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version, flags, method 0, time, date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        // central directory entry
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&[0u8; 8]); // extra, comment, disk start, internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = u32::try_from(out.len()).map_err(|_| too_big())?;
    let cd_size = u32::try_from(central.len()).map_err(|_| too_big())?;
    let count = u16::try_from(files.len()).map_err(|_| too_big())?;
    out.extend_from_slice(&central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

/// Writes `entries` with their own profiles to a `.epmi` archive (for another computer that prints
/// on the same printer).
pub fn export_media(entries: &[CustomMediaEntry], path: &Path) -> Result<()> {
    let mut files = Vec::new();
    let mut exported = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut e = entry.clone();
        if let ProfileSource::Own(p) = resolve_profile(entry) {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((format!("icc/{name}"), fs::read(&p)?));
            e.icc_profile = name;
        }
        exported.push(e);
    }
    files.insert(
        0,
        (
            String::from("custom_media.json"),
            serde_json::to_vec_pretty(&exported)?,
        ),
    );
    fs::write(path, write_zip(&files)?)?;
    Ok(())
}

/// Reads a `.epmi` archive: profiles go to the user ICC store, the entries are returned for merging
/// into the working copy.
pub fn import_media(path: &Path) -> Result<Vec<CustomMediaEntry>> {
    let files = extract_zip_entries(&fs::read(path)?)?;
    let json = files
        .get("custom_media.json")
        .ok_or_else(|| EpmiError::InvalidEmx("custom_media.json fehlt im Archiv".to_string()))?;
    let entries: Vec<CustomMediaEntry> = serde_json::from_slice(json)?;
    let store = user_icc_dir();
    for (name, data) in &files {
        let Some(icc_name) = name.strip_prefix("icc/") else {
            continue;
        };
        let safe = Path::new(icc_name).file_name().map(PathBuf::from);
        if let Some(safe) = safe
            && data.get(36..40) == Some(b"acsp".as_slice())
        {
            fs::create_dir_all(&store)?;
            fs::write(store.join(safe), data)?;
        }
    }
    Ok(entries)
}

/// Merges imported entries into `working` (same slot or media ID is replaced).
pub fn merge_imported(working: &mut Vec<CustomMediaEntry>, imported: Vec<CustomMediaEntry>) {
    for entry in imported {
        if let Some(existing) = working
            .iter_mut()
            .find(|e| e.slot == entry.slot || e.user_media_id == entry.user_media_id)
        {
            *existing = entry;
        } else {
            working.push(entry);
        }
    }
    working.sort_by_key(|e| e.slot);
}

/// Builds the working entries for the printer's registered slots, keeping settings of `existing`.
pub fn entries_for_slots(
    slots: &[PrinterMediaSlot],
    existing: &[CustomMediaEntry],
) -> Vec<CustomMediaEntry> {
    crate::sync::merge_slots_with_existing(slots, existing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(slot: u8, icc: &str) -> CustomMediaEntry {
        let mut s = PrinterMediaSlot::empty(slot);
        s.registered = true;
        s.media_name = format!("Paper {slot}");
        s.user_media_id = 915_000_000 + u32::from(slot);
        let mut e = s.to_custom_media_entry(None);
        e.icc_profile = icc.to_string();
        e
    }

    #[test]
    fn profile_sources_and_states() {
        let base = entry(1, "Epson_SC-P900_700_VelvetFineArtPaper.icc");
        assert!(matches!(resolve_profile(&base), ProfileSource::BaseOnly(_)));
        assert!(matches!(
            resolve_profile(&entry(2, "Does_Not_Exist_xyz.icc")),
            ProfileSource::Missing(_)
        ));

        let system = vec![base.clone()];
        assert_eq!(system_state(&base, &system), SystemState::Installed);
        let mut changed = base.clone();
        changed.print_duty = 5;
        assert_eq!(system_state(&changed, &system), SystemState::Changed);
        assert_eq!(
            system_state(&entry(3, ""), &system),
            SystemState::NotInstalled
        );

        // An installed profile path and the plain file name count as the same profile.
        let mut installed = base.clone();
        installed.icc_profile =
            format!("{SYSTEM_CUSTOM_ICC_DIR}/Epson_SC-P900_700_VelvetFineArtPaper.icc");
        assert_eq!(system_state(&base, &[installed]), SystemState::Installed);
    }

    #[test]
    fn icc_names_are_made_helper_safe() {
        assert_eq!(
            system_icc_file_name("Epson_SC-P900_700_Traditional Photo PaperV2.icc"),
            "Epson_SC-P900_700_Traditional_Photo_PaperV2.icc"
        );
        assert_eq!(
            system_icc_file_name("Hahnemühle Rag.icc"),
            "Hahnem_hle_Rag.icc"
        );
        assert_eq!(system_icc_file_name("..hidden.icc"), "hidden.icc");
    }

    #[test]
    fn zip_roundtrip_with_existing_reader() -> Result<()> {
        let files = vec![
            (String::from("custom_media.json"), b"[]".to_vec()),
            (String::from("icc/a.icc"), vec![7u8; 300]),
        ];
        let read = extract_zip_entries(&write_zip(&files)?)?;
        assert_eq!(read.get("custom_media.json"), Some(&b"[]".to_vec()));
        assert_eq!(read.get("icc/a.icc"), Some(&vec![7u8; 300]));
        Ok(())
    }

    #[test]
    fn merge_replaces_same_slot() {
        let mut working = vec![entry(1, "a.icc"), entry(2, "b.icc")];
        let mut new1 = entry(1, "new.icc");
        new1.media_name = String::from("New");
        merge_imported(&mut working, vec![new1, entry(5, "c.icc")]);
        assert_eq!(working.len(), 3);
        assert_eq!(working[0].icc_profile, "new.icc");
        assert_eq!(working[2].slot, 5);
    }
}
