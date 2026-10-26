use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use epmi_core::{
    BASE_MEDIA_SPECS, DEFAULT_PRINTER_HOST, PrinterMediaSlot, SnmpMediaClient,
    cloud_download_emx, cloud_list_media, default_user_custom_media_json, find_base_media,
    import_emx_package, parse_emx_file, platen_gap_label, sync_printer_slots_to_json,
    try_regenerate_driver_ppds,
};

fn print_usage() {
    eprintln!(
        "\
epmi 0.1.0 - Media Management CLI for Epson SureColor SC-P900 / SC-P700

USAGE:
  epmi <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  list-slots [--host <IP>] [--all] [--json]
      Query custom media slots 1..30 from the printer via SNMPv1.

  sync [--host <IP>] [--json-path <PATH>] [--regenerate-ppd]
      Read active NVRAM slots 1..30 from the printer and write custom_media.json.

  inspect-emx <FILE.emx>
      Inspect XML, CPB, and ICC metadata inside an .emx archive.

  import-emx <FILE.emx> [--slot <1..30>] [--host <IP>] [--write-printer] [--json-path <PATH>] [--regenerate-ppd]
      Extract ICC profile, optionally write slot to printer NVRAM, and update custom_media.json.

  write-slot --slot <1..30> --name <NAME> [--host <IP>] [--base-media <KEY|SN_ID>]
             [--media-id <U32>] [--platen-gap <U8>] [--thickness-mm <F64>]
             [--dry-time <U8>] [--cut-offset <I16>] [--roll-offset <I16>] [--sync]
      Write custom paper parameters directly into printer NVRAM slot 1..30.

  clear-slot --slot <1..30> [--host <IP>] [--sync]
      Clear/reset printer NVRAM slot 1..30.

  cloud-list [--printer scp9xx|scp7xx] [--country DE] [--region 8] [--locale de] [--emx-only] [--json]
      Query downloadable media definitions from the LFP Cloud Content Server.

  cloud-download <EMX_ID> [--out-dir <DIR>] [--import] [--slot <1..30>] [--host <IP>] [--write-printer]
      Download an .emx package by EMX ID (and optionally import it)."
    );
}

fn print_slots_table(slots: &[PrinterMediaSlot], show_all: bool) {
    let rows: Vec<&PrinterMediaSlot> = slots
        .iter()
        .filter(|s| show_all || s.registered)
        .collect();
    if rows.is_empty() {
        println!("No registered custom media slots found on printer.");
        return;
    }
    println!(
        "{:>4}  {:>5}  {:>12}  {:>6}  {:<24}  {:>7}  {:>3}  {:>6}  {:>7}  Media Name (Printer LCD)",
        "Slot", "UI ID", "32-bit MA_ID", "BaseSN", "Platen Gap", "Thick", "Dry", "CutOff", "RollOff"
    );
    println!("{}", "-".repeat(118));
    for s in rows {
        let base_key = s.base_media_key();
        let gap_str = platen_gap_label(s.platen_gap);
        let name_str = if s.registered {
            s.media_name.as_str()
        } else {
            "(Unregistered)"
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
        "list-slots" => {
            let mut host = DEFAULT_PRINTER_HOST.to_string();
            let mut show_all = false;
            let mut as_json = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = args.get(i).cloned().ok_or("Missing value for --host")?;
                    }
                    "--all" => show_all = true,
                    "--json" => as_json = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let client = SnmpMediaClient::new(host);
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
            let mut host = DEFAULT_PRINTER_HOST.to_string();
            let mut json_path: Option<PathBuf> = None;
            let mut regen_ppd = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = args.get(i).cloned().ok_or("Missing value for --host")?;
                    }
                    "--json-path" | "--custom-media-json" => {
                        i += 1;
                        let p = args.get(i).ok_or("Missing value for --json-path")?;
                        json_path = Some(PathBuf::from(p));
                    }
                    "--regenerate-ppd" => regen_ppd = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let client = SnmpMediaClient::new(host);
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
                    "  Slot {:2} | key={:<32} | MA_ID={:<10} | base={} (sn={}) | duty={:+}% | icc={}",
                    item.slot,
                    item.key,
                    item.user_media_id,
                    item.base_media,
                    item.base_sn_id,
                    item.print_duty,
                    item.icc_profile
                );
            }
            if regen_ppd && let Some(msg) = try_regenerate_driver_ppds() {
                println!("{msg}");
            }
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
            let mut host = DEFAULT_PRINTER_HOST.to_string();
            let mut write_printer = false;
            let mut json_path: Option<PathBuf> = None;
            let mut regen_ppd = false;

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
                        host = args.get(i).cloned().ok_or("Missing value for --host")?;
                    }
                    "--write-printer" => write_printer = true,
                    "--json-path" | "--custom-media-json" => {
                        i += 1;
                        let p = args.get(i).ok_or("Missing value for --json-path")?;
                        json_path = Some(PathBuf::from(p));
                    }
                    "--regenerate-ppd" => regen_ppd = true,
                    other if !other.starts_with('-') && emx_file.is_none() => {
                        emx_file = Some(other.to_string());
                    }
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let file_path = emx_file.ok_or("Usage: epmi import-emx <FILE.emx> [OPTIONS]")?;
            let client = SnmpMediaClient::new(host);
            let (_pkg, entry, saved_icc) = import_emx_package(
                &file_path,
                slot,
                json_path.as_deref(),
                if write_printer { Some(&client) } else { None },
            )
            .map_err(|e| e.to_string())?;

            let target_display = json_path
                .unwrap_or_else(default_user_custom_media_json);
            println!(
                "Imported '{}' (MA_ID={}) into {}",
                entry.name,
                entry.user_media_id,
                target_display.display()
            );
            for icc in saved_icc {
                println!("  Saved ICC profile: {}", icc.display());
            }
            let s = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
            println!("{s}");
            if regen_ppd && let Some(msg) = try_regenerate_driver_ppds() {
                println!("{msg}");
            }
            Ok(())
        }
        "write-slot" => {
            let mut host = DEFAULT_PRINTER_HOST.to_string();
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
            let mut regen_ppd = false;

            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = args.get(i).cloned().ok_or("Missing --host")?;
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
                    "--regenerate-ppd" => regen_ppd = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            if !(1..=30).contains(&slot) || name.is_empty() {
                return Err("Usage: epmi write-slot --slot <1..30> --name <NAME> [OPTIONS]".to_string());
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

            let client = SnmpMediaClient::new(host);
            let verified = client.write_slot(&slot_obj).map_err(|e| e.to_string())?;
            println!(
                "Wrote and verified Slot {}: '{}' (MA_ID={}, BaseSN={}, Gap={}, Thickness={:.2}mm)",
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
            if regen_ppd && let Some(msg) = try_regenerate_driver_ppds() {
                println!("{msg}");
            }
            Ok(())
        }
        "clear-slot" => {
            let mut host = DEFAULT_PRINTER_HOST.to_string();
            let mut slot = 0u8;
            let mut do_sync = false;
            let mut regen_ppd = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--host" => {
                        i += 1;
                        host = args.get(i).cloned().ok_or("Missing --host")?;
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
                    "--regenerate-ppd" => regen_ppd = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            if !(1..=30).contains(&slot) {
                return Err("Usage: epmi clear-slot --slot <1..30> [--host <IP>]".to_string());
            }
            let client = SnmpMediaClient::new(host);
            let verified = client.clear_slot(slot).map_err(|e| e.to_string())?;
            println!(
                "Cleared Slot {} (registered={})",
                slot, verified.registered
            );
            if do_sync {
                let _ = sync_printer_slots_to_json(&client, None).map_err(|e| e.to_string())?;
            }
            if regen_ppd && let Some(msg) = try_regenerate_driver_ppds() {
                println!("{msg}");
            }
            Ok(())
        }
        "cloud-list" => {
            let mut printer = "scp9xx".to_string();
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
                    "--emx-only" => emx_only = true,
                    "--json" => as_json = true,
                    other => return Err(format!("Unknown option: {other}")),
                }
                i += 1;
            }
            let records = cloud_list_media(&printer, &country, region, &locale)
                .map_err(|e| e.to_string())?;
            if as_json {
                let s = serde_json::to_string_pretty(&records).map_err(|e| e.to_string())?;
                println!("{s}");
                return Ok(());
            }
            println!(
                "{:<11}  {:<11}  {:<15}  {:<11}  {:<32}  EMX File",
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
                    "{:<11}  {:<11}  {:<15}  {:<11}  {:<32}  {}",
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
            let mut host = DEFAULT_PRINTER_HOST.to_string();
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
                        host = args.get(i).cloned().ok_or("Missing --host")?;
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
            let records = cloud_list_media(&printer, &country, region, &locale)
                .map_err(|e| e.to_string())?;
            let rec = records
                .iter()
                .find(|r| r.emx_id == target_id)
                .ok_or_else(|| format!("EMX ID '{target_id}' not found on server"))?;
            let path = cloud_download_emx(rec, &out_dir).map_err(|e| e.to_string())?;
            println!("Downloaded {}", path.display());
            if do_import {
                let client = SnmpMediaClient::new(host);
                let (_pkg, entry, _) = import_emx_package(
                    &path,
                    slot,
                    None::<&Path>,
                    if write_printer { Some(&client) } else { None },
                )
                .map_err(|e| e.to_string())?;
                println!(
                    "Imported '{}' (MA_ID={})",
                    entry.name, entry.user_media_id
                );
            }
            Ok(())
        }
        other => Err(format!("Unknown subcommand '{other}'. Run 'epmi --help'.")),
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
