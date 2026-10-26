pub mod cloud;
pub mod emx;
pub mod error;
pub mod media;
pub mod snmp;
pub mod sync;

pub use cloud::{CloudMediaRecord, cloud_download_emx, cloud_list_media};
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
    sync_printer_slots_to_json, try_regenerate_driver_ppds,
};
