use std::time::Duration;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{TvConfig, validate_lan_ip};
use crate::error::{Result, TvError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentFileStat {
    pub id: usize,
    pub path: String,
    pub length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentEntry {
    pub hash: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub torrent_size: u64,
    #[serde(default)]
    pub stat: i32,
    #[serde(default)]
    pub stat_string: String,
    #[serde(default)]
    pub file_stats: Option<Vec<TorrentFileStat>>,
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TorrServerDataWrapper {
    #[serde(rename = "TorrServer")]
    torr_server: Option<TorrServerFilesNode>,
    movie: Option<LampaMovieMeta>,
}

#[derive(Debug, Deserialize)]
struct TorrServerFilesNode {
    #[serde(rename = "Files", default)]
    files: Vec<TorrentFileStat>,
}

#[derive(Debug, Deserialize)]
struct LampaMovieMeta {
    title: Option<String>,
    original_title: Option<String>,
    runtime: Option<u32>,
}

impl TorrentEntry {
    /// Возвращает список файлов внутри торрента (из `file_stats`, `data.TorrServer.Files` или синтезирует основной видеопоток).
    pub fn extract_files(&self) -> Vec<TorrentFileStat> {
        if let Some(files) = &self.file_stats
            && !files.is_empty()
        {
            return files.clone();
        }

        if let Some(data_raw) = &self.data
            && let Ok(parsed) = serde_json::from_str::<TorrServerDataWrapper>(data_raw)
            && let Some(ts) = parsed.torr_server
            && !ts.files.is_empty()
        {
            return ts.files;
        }

        if self.torrent_size > 0 {
            vec![TorrentFileStat {
                id: 1,
                path: format!("{}.mkv", self.display_title()),
                length: self.torrent_size,
            }]
        } else {
            Vec::new()
        }
    }

    /// Человекочитаемое название торрента (обогащается из метаданных Lampa, если заголовок обрезан).
    pub fn display_title(&self) -> String {
        if let Some(data_raw) = &self.data
            && let Ok(parsed) = serde_json::from_str::<TorrServerDataWrapper>(data_raw)
            && let Some(movie) = parsed.movie
        {
            match (movie.title, movie.original_title) {
                (Some(ru), Some(orig)) if !ru.is_empty() && !orig.is_empty() && ru != orig => {
                    return format!("[LAMPA] {} / {}", ru, orig);
                }
                (Some(ru), _) if !ru.is_empty() => return format!("[LAMPA] {}", ru),
                (_, Some(orig)) if !orig.is_empty() => return format!("[LAMPA] {}", orig),
                _ => {}
            }
        }
        if self.title.is_empty() {
            self.hash.clone()
        } else {
            self.title.clone()
        }
    }

    /// Длительность фильма в минутах (если известна из метаданных Lampa TMDB).
    pub fn runtime_minutes(&self) -> Option<u32> {
        let data_raw = self.data.as_ref()?;
        let parsed = serde_json::from_str::<TorrServerDataWrapper>(data_raw).ok()?;
        let runtime = parsed.movie?.runtime?;
        if runtime > 0 { Some(runtime) } else { None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrServeSettings {
    #[serde(rename = "CacheSize", default)]
    pub cache_size: u64,
    #[serde(rename = "ReaderReadAHead", alias = "ReaderReadAhead", default)]
    pub reader_read_ahead: u32,
    #[serde(rename = "PreloadCache", default)]
    pub preload_cache: u32,
    #[serde(rename = "UseDisk", default)]
    pub use_disk: bool,
    #[serde(rename = "ForceEncrypt", default)]
    pub force_encrypt: bool,
    #[serde(rename = "ConnectionsLimit", default)]
    pub connections_limit: u32,
}

#[derive(Debug, Clone)]
pub struct TorrServeClient {
    http: Client,
    base_url: String,
}

impl TorrServeClient {
    pub fn from_config(config: &TvConfig, force_on_device: bool) -> Result<Self> {
        let target_ip = if force_on_device {
            "127.0.0.1"
        } else {
            config.tv_ip.trim()
        };
        let ip = validate_lan_ip(target_ip)?;
        let http = Client::builder()
            .timeout(Duration::from_secs(6))
            .build()?;

        Ok(Self {
            http,
            base_url: format!("http://{}:{}", ip, config.torrserve_port),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Проверяет доступность локального сервера TorrServe (`GET /echo` -> `MatriX.147`).
    pub async fn echo(&self) -> Result<String> {
        let url = format!("{}/echo", self.base_url);
        let resp = self.http.get(&url).send().await?.error_for_status()?;
        Ok(resp.text().await?.trim().to_string())
    }

    /// Получает список всех торрентов в памяти локального TorrServe (`POST /torrents {"action":"list"}`).
    pub async fn list_torrents(&self) -> Result<Vec<TorrentEntry>> {
        let url = format!("{}/torrents", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&json!({ "action": "list" }))
            .send()
            .await?
            .error_for_status()?;

        let text = resp.text().await?;
        if text.trim().is_empty() || text.trim() == "null" {
            return Ok(Vec::new());
        }
        let list: Vec<TorrentEntry> = serde_json::from_str(&text)?;
        Ok(list)
    }

    /// Получает текущие настройки TorrServe (`POST /settings {"action":"get"}`).
    pub async fn get_settings(&self) -> Result<TorrServeSettings> {
        let url = format!("{}/settings", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&json!({ "action": "get" }))
            .send()
            .await?
            .error_for_status()?;

        let settings: TorrServeSettings = resp.json().await?;
        Ok(settings)
    }

    /// Применяет безопасный профиль кэша для 2 ГБ ОЗУ (`UseDisk: false`, `CacheSize: 64 МБ`).
    /// Жёсткий инвариант: никогда не включает `UseDisk: true`, чтобы не изнашивать eMMC память ТВ.
    pub async fn apply_safe_ram_cache(&self, safe_cache_bytes: u64) -> Result<TorrServeSettings> {
        let current = self.get_settings().await?;
        if !current.use_disk && current.cache_size == safe_cache_bytes {
            return Ok(current);
        }

        let url = format!("{}/settings", self.base_url);
        let current_resp = self
            .http
            .post(&url)
            .json(&json!({ "action": "get" }))
            .send()
            .await?
            .error_for_status()?;

        let mut raw_sets: Value = current_resp.json().await?;
        if let Some(obj) = raw_sets.as_object_mut() {
            obj.insert("UseDisk".to_string(), Value::Bool(false));
            obj.insert("CacheSize".to_string(), json!(safe_cache_bytes));
            obj.insert("ReaderReadAHead".to_string(), json!(90));
            obj.insert("PreloadCache".to_string(), json!(50));
            obj.insert("ForceEncrypt".to_string(), Value::Bool(true));
            obj.insert("ConnectionsLimit".to_string(), json!(25));
        }

        self.http
            .post(&url)
            .json(&json!({
                "action": "set",
                "sets": raw_sets,
            }))
            .send()
            .await?
            .error_for_status()?;

        let updated = self.get_settings().await?;
        if updated.use_disk {
            return Err(TvError::DiskCacheForbidden);
        }
        Ok(updated)
    }

    /// Формирует прямую ссылку на поток для Just Player / внешнего плеера.
    pub fn build_stream_url(&self, hash: &str, file_id: usize, file_path: &str) -> String {
        let filename = file_path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("movie.mkv");
        let encoded_name = urlencoding::encode(filename);
        format!(
            "{}/stream/{}?link={}&index={}&play",
            self.base_url, encoded_name, hash, file_id
        )
    }
}
