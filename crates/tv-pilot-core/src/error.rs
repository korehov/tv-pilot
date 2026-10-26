use std::net::Ipv4Addr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TvError {
    #[error(
        "Forbidden non-private IP address: {0}. TV Pilot strictly operates inside RFC 1918 home LAN (192.168.x.x, 172.16-31.x.x, 10.x.x.x) or 127.0.0.1 on-device."
    )]
    NonPrivateIpForbidden(Ipv4Addr),

    #[error("Invalid IPv4 address syntax: '{0}'")]
    InvalidIpAddress(String),

    #[error("Invalid Android package or component name: '{0}'")]
    InvalidPackageName(String),

    #[error(
        "CRITICAL SAFETY BLOCK: Package '{0}' is protected. Disabling or removing it causes Google TV black screen (FallbackHome)."
    )]
    CriticalSystemPackageProtected(String),

    #[error("Unsafe file extension blocked in torrent stream: '{0}'")]
    UnsafeFileBlocked(String),

    #[error("TorrServe UseDisk=true is forbidden to protect TV eMMC flash memory")]
    DiskCacheForbidden,

    #[error("ADB command failed: {0}")]
    AdbCommandFailed(String),

    #[error("TorrServe HTTP API error: {0}")]
    TorrServeApi(#[from] reqwest::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("No suitable video stream found for query: '{0}'")]
    NoStreamFound(String),
}

pub type Result<T> = std::result::Result<T, TvError>;
