pub mod cloud;
pub mod discovery;
pub mod emx;
pub mod error;
pub mod media;
pub mod snmp;
pub mod sync;
pub mod system;

pub use cloud::{
    CLOUD_PRINTER_MODELS, CloudMediaRecord, CloudPrinterModel, cloud_download_emx,
    cloud_list_media, match_printer_to_cloud_series,
};
pub use discovery::{
    DiscoveredEpsonPrinter, DiscoverySource, discover_cups_epson_printers, discover_epson_printers,
    extract_host_from_cups_uri, is_epson_printer_identifier, probe_epson_printer_snmp,
};
pub use emx::{EmxPackage, parse_emx_bytes, parse_emx_file};
pub use error::{EpmiError, Result};
pub use media::{
    BASE_MEDIA_SPECS, BaseMediaSpec, CustomMediaEntry, PrinterMediaSlot, find_base_media,
    find_base_media_by_sn_id, media_category_label, platen_gap_label, sanitize_ppd_media_key,
};
pub use snmp::{
    DEFAULT_PRINTER_HOST, DEFAULT_SNMP_PORT, SlotSummaryEntry, SnmpMediaClient, decode_slot_tlv,
    encode_slot_write_packets,
};
pub use sync::{
    default_icc_output_dirs, default_user_custom_media_json, discover_sync_targets,
    import_emx_package, load_custom_media_json, merge_slots_with_existing, save_custom_media_json,
    sync_printer_slots_to_json, user_icc_dir,
};
pub use system::{
    BUILTIN_ICC_DIR, MEDIA_INSTALL_HELPER, ProfileSource, SYSTEM_CUSTOM_ICC_DIR,
    SYSTEM_CUSTOM_MEDIA_JSON, SystemState, assign_profile, entries_for_slots, export_media,
    fetch_cloud_profiles, import_media, install_system_media, load_system_media, merge_imported,
    remove_system_media, resolve_profile, system_state,
};
