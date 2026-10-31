use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use epmi_core::{
    BASE_MEDIA_SPECS, CLOUD_PRINTER_MODELS, CustomMediaEntry, PrinterMediaSlot, ProfileSource,
    SnmpMediaClient, assign_profile, cloud_download_emx, cloud_list_media,
    default_user_custom_media_json, discover_epson_printers, entries_for_slots, export_media,
    fetch_cloud_profiles, find_base_media, import_emx_package, import_media, install_system_media,
    load_custom_media_json, load_system_media, match_printer_to_cloud_series, merge_imported,
    parse_emx_file, platen_gap_label, probe_epson_printer_snmp, remove_system_media,
    resolve_profile, save_custom_media_json, sync_printer_slots_to_json, system_state,
};

fn print_usage() {
    eprintln!(
        "\
epmi 0.1.0 - Media Management CLI for Epson SureColor Printers

USAGE:
  epmi <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  discover [--json]
      Auto-discover Epson printers from CUPS queues and the local network via SNMPv1.

  list-slots [--host <IP>] [--all] [--json]
      Query custom media slots 1..30 from the printer via SNMPv1 (auto-discovers printer if --host omitted).

  sync [--host <IP>] [--json-path <PATH>]
      Read active NVRAM slots 1..30 from the printer into the working copy
      (~/.config/epson-p900-escpr/custom_media.json).

  status [--host <IP>]
      Show each paper: printer slot, installed on this computer?, colour profile.

  install-system [--host <IP>] [--printer <CODE|MODEL>] [--icc <SLOT>=<FILE.icc>]... [--no-cloud]
      „Für das System installieren“: read the printer slots, fetch profiles of Epson cloud media
      automatically, assign own profiles, and install paper list + profiles for all users and the
      CUPS driver (asks for the administrator password via pkexec).

  remove-system <KEY>
      Remove one paper from the system-wide installation.

  export [--slot <1..30>]... -o <FILE.epmi>
      Export papers with their profiles for another computer.

  import <FILE.epmi>
      Import exported papers into the working copy (then run install-system).

  inspect-emx <FILE.emx>
      Inspect XML, CPB, and ICC metadata inside an .emx archive.

  import-emx <FILE.emx> [--slot <1..30>] [--host <IP>] [--write-printer] [--json-path <PATH>]
      Extract ICC profile, optionally write slot to printer NVRAM, and update the working copy.

  write-slot --slot <1..30> --name <NAME> [--host <IP>] [--base-media <KEY|SN_ID>]
             [--media-id <U32>] [--platen-gap <U8>] [--thickness-mm <F64>]
             [--dry-time <U8>] [--cut-offset <I16>] [--roll-offset <I16>] [--sync]
      Write custom paper parameters directly into printer NVRAM slot 1..30.

  clear-slot --slot <1..30> [--host <IP>] [--sync]
      Clear/reset printer NVRAM slot 1..30.

  cloud-models
      List all 27 supported Epson SureColor printer models and their cloud series codes.

  cloud-list [--printer <CODE|MODEL>] [--country DE] [--region 8] [--locale de] [--emx-only] [--json]
      Query downloadable media definitions from the LFP Cloud Content Server.

  cloud-download <EMX_ID> [--out-dir <DIR>] [--printer <CODE|MODEL>] [--import] [--slot <1..30>] [--host <IP>] [--write-printer]
      Download an .emx package by EMX ID (and optionally import it)."
    );
}

fn resolve_printer_host(explicit_host: Option<String>) -> Result<String, String> {
    if let Some(h) = explicit_host
        && !h.trim().is_empty()
    {
        return Ok(h);
    }
    let found = discover_epson_printers(&[]);
    if let Some(first) = found.first() {
        eprintln!(
            "Auto-selected Epson printer: {} ({})",
            first.model_name, first.host
        );
        return Ok(first.host.clone());
    }
    Err(
        "No Epson printer found automatically via CUPS or SNMP. Please pass --host <IP>."
            .to_string(),
    )
}

fn print_slots_table(slots: &[PrinterMediaSlot], show_all: bool) {
    let rows: Vec<&PrinterMediaSlot> = slots.iter().filter(|s| show_all || s.registered).collect();
    if rows.is_empty() {
        println!("No registered custom media slots found on printer.");
        return;
    }
    println!(
        "{:>4}  {:>5}  {:>12}  {:>6}  {:<24}  {:>7}  {:>3}  {:>6}  {:>7}  Media Name (Printer Display)",
        "Slot", "UI ID", "Media ID", "BaseSN", "Platen Gap", "Thick", "Dry", "CutOff", "RollOff"
    );
    println!("{}", "-".repeat(118));
    for s in rows {
        let base_key = s.base_media_key();
        let gap_str = platen_gap_label(s.platen_gap);
        let name_str = if s.registered {
            s.media_name.as_str()
        } else {
            "(Empty)"
        };
        println!(
            "{:>4}  {:>5}  {:>12}  {:>6}  {:<24}  {:>5.2}mm  {:>3}  {:+6}  {:+7}  {} [{} ({})]",
            s.slot,
            s.driver_ui_id(),
            s.user_media_id,
            s.base_sn_id,
            gap_str,
            s.thickness_mm(),
            s.drying_time,
            s.cut_sheet_feed_offset,
            s.roll_paper_feed_offset,
            name_str,
            s.base_sn_id,
            base_key
        );
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(subcmd) = args.first().map(String::as_str) else {
        print_usage();
        return Ok(());
    };

    match subcmd {
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        "discover" => {
            let as_json = args.iter().any(|a| a == "--json");
            let found = discover_epson_printers(&[]);
            if as_json {
                let s = serde_json::to_string_pretty(&found).map_err(|e| e.to_string())?;
                println!("{s}");
                return Ok(());
            }
            if found.is_empty() {
                println!("No Epson printers discovered in CUPS or on the local network.");
                return Ok(());
            }
            println!(
                "{:<16}  {:<28}  {:<10}  {:<10}  Source",
                "Host / IP", "Epson Model", "Status", "Cloud Code"
            );
            println!("{}", "-".repeat(92));
            for p in found {
                let status = if p.online { "Online" } else { "CUPS only" };
                let cloud = p.cloud_series_code.as_deref().unwrap_or("-");
                println!(
                    "{:<16}  {:<28}  {:<10}  {:<10}  {}",
                    p.host,
                    p.model_name,
                    status,
                    cloud,
                    p.source.badge_de()
                );
            }
            Ok(())
        }
        "cloud-models" => {
            println!("{:<14}  {:<18}  Model Series", "Series Code", "Category");
            println!("{}", "-".repeat(78));
            for m in CLOUD_PRINTER_MODELS {
                println!("{:<14}  {:<18}  {}", m.code, m.category, m.label);
            }
            Ok(())
        }
        "list-slots" => {
            let mut host: Option<String> = None;
            let mut show_all = false;
            let mut as_json = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing value for --host")?);
                    }
                    "--all" => show_all = true,
                    "--json" => as_json = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let target_host = resolve_printer_host(host)?;
            let client = SnmpMediaClient::new(target_host);
            let slots = client
                .read_all_slots(!show_all)
                .map_err(|e| e.to_string())?;
            if as_json {
                let entries: Vec<_> = slots
                    .iter()
                    .map(|s| s.to_custom_media_entry(None))
                    .collect();
                let s = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
                println!("{s}");
            } else {
                print_slots_table(&slots, show_all);
            }
            Ok(())
        }
        "sync" => {
            let mut host: Option<String> = None;
            let mut json_path: Option<PathBuf> = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing value for --host")?);
                    }
                    "--json-path" | "--custom-media-json" => {
                        i += 1;
                        let p = args.get(i).ok_or("Missing value for --json-path")?;
                        json_path = Some(PathBuf::from(p));
                    }
                    // Deprecated: PPDs are refreshed by `install-system`.
                    "--regenerate-ppd" => {}
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let target_host = resolve_printer_host(host)?;
            let client = SnmpMediaClient::new(target_host);
            let (written_paths, entries) =
                sync_printer_slots_to_json(&client, json_path.as_deref())
                    .map_err(|e| e.to_string())?;
            for p in &written_paths {
                println!(
                    "Synchronized {} custom media entries into {}",
                    entries.len(),
                    p.display()
                );
            }
            for item in &entries {
                println!(
                    "  Slot {:2} | key={:<32} | ID={:<10} | base={} (sn={}) | duty={:+}% | icc={}",
                    item.slot,
                    item.key,
                    item.user_media_id,
                    item.base_media,
                    item.base_sn_id,
                    item.print_duty,
                    item.icc_profile
                );
            }
            println!("Für Druckertreiber und LumaPrint: epmi install-system");
            Ok(())
        }
        "inspect-emx" => {
            let file_path = args.get(1).ok_or("Usage: epmi inspect-emx <FILE.emx>")?;
            let pkg = parse_emx_file(file_path).map_err(|e| e.to_string())?;
            let info = serde_json::json!({
                "source_path": pkg.source_path,
                "model_name": pkg.model_name,
                "series_name": pkg.series_name,
                "user_media_id": pkg.user_media_id,
                "media_name": pkg.media_name,
                "media_full_name": pkg.media_full_name,
                "vendor": pkg.vendor,
                "base_sn_id": format!("{} ({})", pkg.base_sn_id, pkg.base_media_key()),
                "mac_ppd_id": pkg.mac_ppd_id,
                "win_drv_id": pkg.win_drv_id,
                "media_category": pkg.media_category,
                "platen_gap": pkg.platen_gap,
                "thickness_01mm": pkg.thickness_01mm,
                "thickness_mm": pkg.thickness_mm(),
                "drying_time": pkg.drying_time,
                "paper_width_check": pkg.paper_width_check,
                "cut_sheet_feed_offset": pkg.cut_sheet_feed_offset,
                "roll_paper_feed_offset": pkg.roll_paper_feed_offset,
                "ink_density": pkg.ink_density,
                "print_duty": pkg.print_duty(),
                "icc_profile_name": pkg.icc_profile_name,
                "icc_size_bytes": pkg.icc_profile_bytes.len(),
            });
            let pretty = serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?;
            println!("{pretty}");
            Ok(())
        }
        "import-emx" => {
            let mut emx_file: Option<String> = None;
            let mut slot = 0u8;
            let mut host: Option<String> = None;
            let mut write_printer = false;
            let mut json_path: Option<PathBuf> = None;

            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--slot" => {
                        i += 1;
                        slot = args
                            .get(i)
                            .ok_or("Missing value for --slot")?
                            .parse::<u8>()
                            .map_err(|_| "Invalid --slot number")?;
                    }
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing value for --host")?);
                    }
                    "--write-printer" => write_printer = true,
                    "--json-path" | "--custom-media-json" => {
                        i += 1;
                        let p = args.get(i).ok_or("Missing value for --json-path")?;
                        json_path = Some(PathBuf::from(p));
                    }
                    // Deprecated: PPDs are refreshed by `install-system`.
                    "--regenerate-ppd" => {}
                    other if !other.starts_with('-') && emx_file.is_none() => {
                        emx_file = Some(other.to_string());
                    }
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let file_path = emx_file.ok_or("Usage: epmi import-emx <FILE.emx> [OPTIONS]")?;
            let client_opt = if write_printer {
                let target_host = resolve_printer_host(host)?;
                Some(SnmpMediaClient::new(target_host))
            } else {
                None
            };
            let (_pkg, entry, saved_icc) =
                import_emx_package(&file_path, slot, json_path.as_deref(), client_opt.as_ref())
                    .map_err(|e| e.to_string())?;

            let target_display = json_path.unwrap_or_else(default_user_custom_media_json);
            println!(
                "Imported '{}' (ID={}) into {}",
                entry.name,
                entry.user_media_id,
                target_display.display()
            );
            for icc in saved_icc {
                println!("  Saved ICC profile: {}", icc.display());
            }
            let s = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
            println!("{s}");
            println!("Für Druckertreiber und LumaPrint: epmi install-system");
            Ok(())
        }
        "write-slot" => {
            let mut host: Option<String> = None;
            let mut slot = 0u8;
            let mut name = String::new();
            let mut base_media = "VelvetFineArt".to_string();
            let mut media_id = 0u32;
            let mut platen_gap: Option<u8> = None;
            let mut thickness_mm: Option<f64> = None;
            let mut dry_time = 0u8;
            let mut cut_offset = 0i16;
            let mut roll_offset = 0i16;
            let mut category: Option<u8> = None;
            let mut do_sync = false;

            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing --host")?);
                    }
                    "--slot" => {
                        i += 1;
                        slot = args
                            .get(i)
                            .ok_or("Missing --slot")?
                            .parse::<u8>()
                            .map_err(|_| "Invalid --slot")?;
                    }
                    "--name" => {
                        i += 1;
                        name = args.get(i).cloned().ok_or("Missing --name")?;
                    }
                    "--base-media" => {
                        i += 1;
                        base_media = args.get(i).cloned().ok_or("Missing --base-media")?;
                    }
                    "--media-id" => {
                        i += 1;
                        media_id = args
                            .get(i)
                            .ok_or("Missing --media-id")?
                            .parse::<u32>()
                            .map_err(|_| "Invalid --media-id")?;
                    }
                    "--platen-gap" => {
                        i += 1;
                        platen_gap = Some(
                            args.get(i)
                                .ok_or("Missing --platen-gap")?
                                .parse::<u8>()
                                .map_err(|_| "Invalid --platen-gap")?,
                        );
                    }
                    "--thickness-mm" => {
                        i += 1;
                        thickness_mm = Some(
                            args.get(i)
                                .ok_or("Missing --thickness-mm")?
                                .parse::<f64>()
                                .map_err(|_| "Invalid --thickness-mm")?,
                        );
                    }
                    "--dry-time" => {
                        i += 1;
                        dry_time = args
                            .get(i)
                            .ok_or("Missing --dry-time")?
                            .parse::<u8>()
                            .map_err(|_| "Invalid --dry-time")?;
                    }
                    "--cut-offset" => {
                        i += 1;
                        cut_offset = args
                            .get(i)
                            .ok_or("Missing --cut-offset")?
                            .parse::<i16>()
                            .map_err(|_| "Invalid --cut-offset")?;
                    }
                    "--roll-offset" => {
                        i += 1;
                        roll_offset = args
                            .get(i)
                            .ok_or("Missing --roll-offset")?
                            .parse::<i16>()
                            .map_err(|_| "Invalid --roll-offset")?;
                    }
                    "--category" => {
                        i += 1;
                        category = Some(
                            args.get(i)
                                .ok_or("Missing --category")?
                                .parse::<u8>()
                                .map_err(|_| "Invalid --category")?,
                        );
                    }
                    "--sync" => do_sync = true,
                    // Deprecated: PPDs are refreshed by `install-system`.
                    "--regenerate-ppd" => {}
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            if !(1..=30).contains(&slot) || name.is_empty() {
                return Err(
                    "Usage: epmi write-slot --slot <1..30> --name <NAME> [OPTIONS]".to_string(),
                );
            }

            let base_spec = find_base_media(&base_media).unwrap_or(&BASE_MEDIA_SPECS[5]);
            let t_01mm = thickness_mm
                .map(|mm| (mm * 100.0).round() as u16)
                .unwrap_or(base_spec.default_thickness_01mm);
            let slot_obj = PrinterMediaSlot {
                slot,
                registered: true,
                media_name: name,
                user_media_id: if media_id > 0 {
                    media_id
                } else {
                    910_000_000 + u32::from(slot)
                },
                base_sn_id: base_spec.sn_id,
                media_category: category.unwrap_or(base_spec.default_category),
                platen_gap: platen_gap.unwrap_or(base_spec.default_platen_gap),
                thickness_01mm: t_01mm,
                drying_time: dry_time,
                paper_width_check: 1,
                cut_sheet_feed_offset: cut_offset,
                roll_paper_feed_offset: roll_offset,
                margin_sets: 0,
            };

            let target_host = resolve_printer_host(host)?;
            let client = SnmpMediaClient::new(target_host);
            let verified = client.write_slot(&slot_obj).map_err(|e| e.to_string())?;
            println!(
                "Wrote and verified Slot {}: '{}' (ID={}, BaseSN={}, Gap={}, Thickness={:.2}mm)",
                verified.slot,
                verified.media_name,
                verified.user_media_id,
                verified.base_sn_id,
                verified.platen_gap,
                verified.thickness_mm()
            );
            if do_sync {
                let _ = sync_printer_slots_to_json(&client, None).map_err(|e| e.to_string())?;
            }
            println!("Für Druckertreiber und LumaPrint: epmi install-system");
            Ok(())
        }
        "clear-slot" => {
            let mut host: Option<String> = None;
            let mut slot = 0u8;
            let mut do_sync = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing --host")?);
                    }
                    "--slot" => {
                        i += 1;
                        slot = args
                            .get(i)
                            .ok_or("Missing --slot")?
                            .parse::<u8>()
                            .map_err(|_| "Invalid --slot")?;
                    }
                    "--sync" => do_sync = true,
                    // Deprecated: PPDs are refreshed by `install-system`.
                    "--regenerate-ppd" => {}
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            if !(1..=30).contains(&slot) {
                return Err("Usage: epmi clear-slot --slot <1..30> [--host <IP>]".to_string());
            }
            let target_host = resolve_printer_host(host)?;
            let client = SnmpMediaClient::new(target_host);
            let verified = client.clear_slot(slot).map_err(|e| e.to_string())?;
            println!("Cleared Slot {} (registered={})", slot, verified.registered);
            if do_sync {
                let _ = sync_printer_slots_to_json(&client, None).map_err(|e| e.to_string())?;
            }
            println!("Für Druckertreiber und LumaPrint: epmi install-system");
            Ok(())
        }
        "cloud-list" => {
            let mut printer: Option<String> = None;
            let mut country = "DE".to_string();
            let mut region = 8u32;
            let mut locale = "de".to_string();
            let mut emx_only = false;
            let mut as_json = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--printer" => {
                        i += 1;
                        printer = Some(args.get(i).cloned().ok_or("Missing --printer")?);
                    }
                    "--country" => {
                        i += 1;
                        country = args.get(i).cloned().ok_or("Missing --country")?;
                    }
                    "--region" => {
                        i += 1;
                        region = args
                            .get(i)
                            .ok_or("Missing --region")?
                            .parse::<u32>()
                            .map_err(|_| "Invalid --region")?;
                    }
                    "--locale" => {
                        i += 1;
                        locale = args.get(i).cloned().ok_or("Missing --locale")?;
                    }
                    "--emx-only" => emx_only = true,
                    "--json" => as_json = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let printer_code = match printer {
                Some(p) => p,
                None => discover_epson_printers(&[])
                    .into_iter()
                    .find_map(|p| p.cloud_series_code)
                    .unwrap_or_else(|| "scp9xx".to_string()),
            };
            let records = cloud_list_media(&printer_code, &country, region, &locale)
                .map_err(|e| e.to_string())?;
            if as_json {
                let s = serde_json::to_string_pretty(&records).map_err(|e| e.to_string())?;
                println!("{s}");
                return Ok(());
            }
            println!(
                "{:<11}  {:<13}  {:<15}  {:<11}  {:<32}  EMX File",
                "EMX ID", "Manufacturer", "Category", "Kind", "Paper Name"
            );
            println!("{}", "-".repeat(118));
            for r in records {
                if emx_only && !r.has_downloadable_emx() {
                    continue;
                }
                let emx_str = if r.emx_id.is_empty() {
                    "-"
                } else {
                    r.emx_id.as_str()
                };
                let kind = if r.is_custom_emx() {
                    "Custom/EMX"
                } else if r.has_downloadable_emx() {
                    "Preset/EMX"
                } else {
                    "Built-in"
                };
                println!(
                    "{:<11}  {:<13}  {:<15}  {:<11}  {:<32}  {}",
                    emx_str,
                    r.media_manufacturer_name,
                    r.media_group_name,
                    kind,
                    r.media_type_name,
                    r.file_name
                );
            }
            Ok(())
        }
        "cloud-download" => {
            let mut emx_id: Option<String> = None;
            let mut out_dir = PathBuf::from("./emx");
            let mut printer = "scp9xx".to_string();
            let mut country = "DE".to_string();
            let mut region = 8u32;
            let mut locale = "de".to_string();
            let mut do_import = false;
            let mut slot = 0u8;
            let mut host: Option<String> = None;
            let mut write_printer = false;

            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--out-dir" => {
                        i += 1;
                        let p = args.get(i).ok_or("Missing --out-dir")?;
                        out_dir = PathBuf::from(p);
                    }
                    "--printer" => {
                        i += 1;
                        printer = args.get(i).cloned().ok_or("Missing --printer")?;
                    }
                    "--country" => {
                        i += 1;
                        country = args.get(i).cloned().ok_or("Missing --country")?;
                    }
                    "--region" => {
                        i += 1;
                        region = args
                            .get(i)
                            .ok_or("Missing --region")?
                            .parse::<u32>()
                            .map_err(|_| "Invalid --region")?;
                    }
                    "--locale" => {
                        i += 1;
                        locale = args.get(i).cloned().ok_or("Missing --locale")?;
                    }
                    "--import" => do_import = true,
                    "--slot" => {
                        i += 1;
                        slot = args
                            .get(i)
                            .ok_or("Missing --slot")?
                            .parse::<u8>()
                            .map_err(|_| "Invalid --slot")?;
                    }
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing --host")?);
                    }
                    "--write-printer" => write_printer = true,
                    other if !other.starts_with('-') && emx_id.is_none() => {
                        emx_id = Some(other.to_string());
                    }
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let target_id = emx_id.ok_or("Usage: epmi cloud-download <EMX_ID> [OPTIONS]")?;
            let records =
                cloud_list_media(&printer, &country, region, &locale).map_err(|e| e.to_string())?;
            let rec = records
                .iter()
                .find(|r| r.emx_id == target_id)
                .ok_or_else(|| format!("EMX ID '{target_id}' not found on server"))?;
            let path = cloud_download_emx(rec, &out_dir).map_err(|e| e.to_string())?;
            println!("Downloaded {}", path.display());
            if do_import {
                let client_opt = if write_printer {
                    let target_host = resolve_printer_host(host)?;
                    Some(SnmpMediaClient::new(target_host))
                } else {
                    None
                };
                let (_pkg, entry, _) =
                    import_emx_package(&path, slot, None::<&Path>, client_opt.as_ref())
                        .map_err(|e| e.to_string())?;
                println!("Imported '{}' (ID={})", entry.name, entry.user_media_id);
            }
            Ok(())
        }
        "status" => {
            let host = parse_host_only(&args[1..])?;
            let working = load_working_copy();
            let entries = match read_registered_slots(host) {
                Ok((slots, _model)) => entries_for_slots(&slots, &working),
                Err(e) => {
                    eprintln!("Drucker nicht erreichbar ({e}); zeige die Arbeitskopie.");
                    working
                }
            };
            print_status_table(&entries);
            Ok(())
        }
        "install-system" => {
            let mut host: Option<String> = None;
            let mut printer: Option<String> = None;
            let mut use_cloud = true;
            let mut icc_assignments: Vec<(u8, PathBuf)> = Vec::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = Some(args.get(i).cloned().ok_or("Missing value for --host")?);
                    }
                    "--printer" => {
                        i += 1;
                        printer = Some(args.get(i).cloned().ok_or("Missing value for --printer")?);
                    }
                    "--icc" => {
                        i += 1;
                        let spec = args.get(i).ok_or("Missing value for --icc")?;
                        let (slot, file) = spec
                            .split_once('=')
                            .ok_or("--icc expects <SLOT>=<FILE.icc>")?;
                        let slot: u8 = slot.parse().map_err(|_| "Invalid slot in --icc")?;
                        icc_assignments.push((slot, PathBuf::from(file)));
                    }
                    "--no-cloud" => use_cloud = false,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let working = load_working_copy();
            let (slots, model) = read_registered_slots(host)?;
            let mut entries = entries_for_slots(&slots, &working);
            for (slot, file) in &icc_assignments {
                let entry = entries
                    .iter_mut()
                    .find(|e| e.slot == *slot)
                    .ok_or_else(|| format!("Speicherplatz {slot} ist im Drucker nicht belegt"))?;
                assign_profile(entry, file).map_err(|e| e.to_string())?;
            }
            if use_cloud {
                let series = printer.unwrap_or(model);
                let code = match_printer_to_cloud_series(&series)
                    .map(|m| m.code)
                    .unwrap_or("scp9xx");
                match cloud_list_media(code, "DE", 8, "de")
                    .and_then(|list| fetch_cloud_profiles(&mut entries, &list))
                {
                    Ok(done) => {
                        for name in done {
                            println!("Farbprofil aus der Epson-Cloud übernommen: {name}");
                        }
                    }
                    Err(e) => eprintln!("Epson-Cloud nicht erreichbar ({e}); weiter ohne."),
                }
            }
            save_custom_media_json(&default_user_custom_media_json(), &entries)
                .map_err(|e| e.to_string())?;
            print_status_table(&entries);
            let report = install_system_media(&entries).map_err(|e| e.to_string())?;
            println!("{report}");
            Ok(())
        }
        "remove-system" => {
            let key = args.get(1).ok_or("Usage: epmi remove-system <KEY>")?;
            let report = remove_system_media(key).map_err(|e| e.to_string())?;
            println!("{report}");
            Ok(())
        }
        "export" => {
            let mut slots: Vec<u8> = Vec::new();
            let mut out: Option<PathBuf> = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--slot" => {
                        i += 1;
                        let v = args.get(i).ok_or("Missing value for --slot")?;
                        slots.push(v.parse().map_err(|_| "Invalid slot")?);
                    }
                    "-o" | "--out" => {
                        i += 1;
                        out = Some(PathBuf::from(args.get(i).ok_or("Missing value for -o")?));
                    }
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let out = out.ok_or("Usage: epmi export [--slot <N>]... -o <FILE.epmi>")?;
            let selected: Vec<CustomMediaEntry> = load_working_copy()
                .into_iter()
                .filter(|e| slots.is_empty() || slots.contains(&e.slot))
                .collect();
            if selected.is_empty() {
                return Err(String::from("Keine passenden Papiere in der Arbeitskopie"));
            }
            export_media(&selected, &out).map_err(|e| e.to_string())?;
            println!(
                "{} Papier(e) exportiert nach {}",
                selected.len(),
                out.display()
            );
            Ok(())
        }
        "import" => {
            let file = args.get(1).ok_or("Usage: epmi import <FILE.epmi>")?;
            let imported = import_media(Path::new(file)).map_err(|e| e.to_string())?;
            let count = imported.len();
            let mut working = load_working_copy();
            merge_imported(&mut working, imported);
            save_custom_media_json(&default_user_custom_media_json(), &working)
                .map_err(|e| e.to_string())?;
            println!("{count} Papier(e) importiert. Jetzt: epmi install-system");
            Ok(())
        }
        other => Err(format!("Unknown subcommand '{other}'. Run 'epmi --help'.")),
    }
}

/// Working copy, seeded from the system installation if the user has none yet.
fn load_working_copy() -> Vec<CustomMediaEntry> {
    let working = load_custom_media_json(&default_user_custom_media_json());
    if working.is_empty() {
        load_system_media()
    } else {
        working
    }
}

fn parse_host_only(args: &[String]) -> Result<Option<String>, String> {
    match args {
        [] => Ok(None),
        [flag, host] if flag == "--host" => Ok(Some(host.clone())),
        _ => Err(String::from("Usage: --host <IP>")),
    }
}

/// Registered printer slots and the printer model (for the cloud catalog).
fn read_registered_slots(host: Option<String>) -> Result<(Vec<PrinterMediaSlot>, String), String> {
    let explicit = host.is_some();
    let target_host = resolve_printer_host(host)?;
    let model = if explicit {
        probe_epson_printer_snmp(&target_host, std::time::Duration::from_secs(2))
            .unwrap_or_default()
    } else {
        discover_epson_printers(&[])
            .into_iter()
            .find(|p| p.host == target_host)
            .map(|p| p.model_name)
            .unwrap_or_default()
    };
    let slots = SnmpMediaClient::new(target_host)
        .read_all_slots(true)
        .map_err(|e| e.to_string())?;
    Ok((slots, model))
}

fn print_status_table(entries: &[CustomMediaEntry]) {
    let system = load_system_media();
    println!(
        "Platz  Papier                                     Dieser Rechner                  Farbprofil"
    );
    println!("{}", "-".repeat(118));
    for e in entries {
        let profile = match resolve_profile(e) {
            ProfileSource::Own(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            ProfileSource::BaseOnly(n) => format!("nur Basisprofil ({n})"),
            ProfileSource::Missing(n) => format!("FEHLT: {n}"),
        };
        println!(
            "{:5}  {:<42} {:<31} {}",
            e.slot,
            e.media_name.chars().take(42).collect::<String>(),
            system_state(e, &system).label_de(),
            profile
        );
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Err(err) = run(&args) {
        eprintln!("ERROR: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
