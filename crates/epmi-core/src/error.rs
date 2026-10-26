use std::io;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, EpmiError>;

#[derive(Debug, Error)]
pub enum EpmiError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid slot number {0} (must be 1..=30)")]
    InvalidSlot(u8),

    #[error("SNMP protocol error: {0}")]
    SnmpProtocol(String),

    #[error("Printer NVRAM command returned error code 0x{0:02x}")]
    PrinterExError(u8),

    #[error("Invalid .emx package: {0}")]
    InvalidEmx(String),

    #[error("Cloud server error: {0}")]
    Cloud(String),
}
