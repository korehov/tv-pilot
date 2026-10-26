use serde::{Deserialize, Serialize};

use crate::security::{FileSafetyVerdict, inspect_torrent_file_safety};
use crate::torrserve::{TorrentEntry, TorrentFileStat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TvRamProfile {
    /// Телевизоры с 1.5–2 ГБ ОЗУ (например, TCL R51MT05 / Realtek RTD2851M): идеал 6–18 ГБ, потолок 28 ГБ.
    Light2Gb,
    /// Приставки и ТВ с 3 ГБ ОЗУ: идеал 8–30 ГБ, потолок 45 ГБ.
    Standard3Gb,
    /// Устройства с 4+ ГБ ОЗУ: идеал 15–60 ГБ, потолок 90 ГБ.
    Ultra4GbPlus,
}

impl TvRamProfile {
    pub fn from_mem_total_mb(mem_total_mb: u64) -> Self {
        if mem_total_mb <= 2200 {
            Self::Light2Gb
        } else if mem_total_mb <= 3500 {
            Self::Standard3Gb
        } else {
            Self::Ultra4GbPlus
        }
    }

    pub fn ideal_size_range_gb(self) -> (f64, f64) {
        match self {
            Self::Light2Gb => (6.0, 18.0),
            Self::Standard3Gb => (8.0, 30.0),
            Self::Ultra4GbPlus => (15.0, 60.0),
        }
    }

    pub fn max_safe_size_gb(self) -> f64 {
        match self {
            Self::Light2Gb => 28.0,
            Self::Standard3Gb => 45.0,
            Self::Ultra4GbPlus => 90.0,
        }
    }

    pub fn human_label(self) -> &'static str {
        match self {
            Self::Light2Gb => "Light 2GB RAM (потолок 28 ГБ, оптимум 6–18 ГБ)",
            Self::Standard3Gb => "Standard 3GB RAM (потолок 45 ГБ, оптимум 8–30 ГБ)",
            Self::Ultra4GbPlus => "Ultra 4GB+ RAM (потолок 90 ГБ, оптимум 15–60 ГБ)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamVerdict {
    /// ✅ Оптимально: пойдёт плавно без лагов и перегрузки CPU
    Optimal,
    /// 🟡 Допустимо: воспроизведение возможно, но размер ниже или выше золотой середины
    Acceptable,
    /// ❌ Тяжёлый Remux: превышает потолок памяти/CPU ТВ (вызовет фризы и 100%+ CPU)
    TooHeavyRemux,
    /// 🛑 Заблокировано: файл не является видео или содержит опасное расширение (.apk, .exe)
    BlockedUnsafe,
}

impl StreamVerdict {
    pub fn human_badge(self) -> &'static str {
        match self {
            Self::Optimal => "✅ Оптимально (без лагов)",
            Self::Acceptable => "🟡 Допустимо",
            Self::TooHeavyRemux => "❌ Тяжёлый Remux (риск лагов)",
            Self::BlockedUnsafe => "🛑 Заблокирован (не видео)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredTorrentStream {
    pub hash: String,
    pub title: String,
    pub file_id: usize,
    pub file_path: String,
    pub size_bytes: u64,
    pub size_gb: f64,
    pub estimated_bitrate_mbps: f64,
    pub score: u32,
    pub verdict: StreamVerdict,
    pub reason: String,
}

pub struct StreamScorer {
    profile: TvRamProfile,
}

impl StreamScorer {
    pub fn new(profile: TvRamProfile) -> Self {
        Self { profile }
    }

    pub fn profile(&self) -> TvRamProfile {
        self.profile
    }

    /// Оценивает конкретный видеофайл внутри торрента с учётом профиля ОЗУ телевизора.
    pub fn score_file(
        &self,
        hash: &str,
        title: &str,
        file: &TorrentFileStat,
        runtime_minutes: Option<u32>,
    ) -> ScoredTorrentStream {
        let size_gb = (file.length as f64) / (1024.0 * 1024.0 * 1024.0);
        let duration_mins = runtime_minutes.unwrap_or(110).max(30) as f64;
        let duration_secs = duration_mins * 60.0;
        let estimated_bitrate_mbps = ((file.length as f64) * 8.0) / (duration_secs * 1_000_000.0);

        match inspect_torrent_file_safety(&file.path) {
            FileSafetyVerdict::BlockedDangerousExtension(ext) => {
                return ScoredTorrentStream {
                    hash: hash.to_string(),
                    title: title.to_string(),
                    file_id: file.id,
                    file_path: file.path.clone(),
                    size_bytes: file.length,
                    size_gb,
                    estimated_bitrate_mbps,
                    score: 0,
                    verdict: StreamVerdict::BlockedUnsafe,
                    reason: format!("Заблокировано опасное расширение файла (.{ext})"),
                };
            }
            FileSafetyVerdict::NotVideoExtension(ext) => {
                return ScoredTorrentStream {
                    hash: hash.to_string(),
                    title: title.to_string(),
                    file_id: file.id,
                    file_path: file.path.clone(),
                    size_bytes: file.length,
                    size_gb,
                    estimated_bitrate_mbps,
                    score: 0,
                    verdict: StreamVerdict::BlockedUnsafe,
                    reason: format!("Не видеофайл (.{ext})"),
                };
            }
            FileSafetyVerdict::SafeVideo => {}
        }

        let (ideal_min, ideal_max) = self.profile.ideal_size_range_gb();
        let max_safe = self.profile.max_safe_size_gb();

        // Свыше потолка (например >28 ГБ для 2 ГБ ТВ) — жёсткая отбраковка TooHeavyRemux (0 баллов)
        if size_gb > max_safe {
            return ScoredTorrentStream {
                hash: hash.to_string(),
                title: title.to_string(),
                file_id: file.id,
                file_path: file.path.clone(),
                size_bytes: file.length,
                size_gb,
                estimated_bitrate_mbps,
                score: 0,
                verdict: StreamVerdict::TooHeavyRemux,
                reason: format!(
                    "{:.1} ГБ (~{:.1} Мбит/с) превышает безопасный потолок {:.0} ГБ для 2 ГБ ОЗУ",
                    size_gb, estimated_bitrate_mbps, max_safe
                ),
            };
        }

        let mut score: u32 = 40; // Базовый балл за валидный видеофайл
        let verdict;
        let reason;

        if (ideal_min..=ideal_max).contains(&size_gb) {
            score += 40;
            verdict = StreamVerdict::Optimal;
            reason = format!(
                "Золотая середина {:.1} ГБ (~{:.1} Мбит/с) — плавный аппаратный декодинг",
                size_gb, estimated_bitrate_mbps
            );
        } else if (2.0..ideal_min).contains(&size_gb) || (ideal_max..=max_safe).contains(&size_gb) {
            score += 20;
            verdict = StreamVerdict::Acceptable;
            reason = format!(
                "Допустимый размер {:.1} ГБ (~{:.1} Мбит/с)",
                size_gb, estimated_bitrate_mbps
            );
        } else {
            score += 10;
            verdict = StreamVerdict::Acceptable;
            reason = format!(
                "Компактный поток {:.2} ГБ (~{:.1} Мбит/с, возможна серия или рип)",
                size_gb, estimated_bitrate_mbps
            );
        }

        let combined_lower = format!("{} {}", title, file.path).to_lowercase();
        if combined_lower.contains("web-dl") || combined_lower.contains("webrip") {
            score += 10;
        }
        if combined_lower.contains("1080p")
            || combined_lower.contains("hevc")
            || combined_lower.contains("h.265")
            || combined_lower.contains("avc")
        {
            score += 10;
        }

        ScoredTorrentStream {
            hash: hash.to_string(),
            title: title.to_string(),
            file_id: file.id,
            file_path: file.path.clone(),
            size_bytes: file.length,
            size_gb,
            estimated_bitrate_mbps,
            score: score.min(100),
            verdict,
            reason,
        }
    }

    /// Выбирает лучший видеофайл внутри одного торрента (для сериалов или многофайловых раздач).
    pub fn score_torrent(&self, entry: &TorrentEntry) -> Option<ScoredTorrentStream> {
        let files = entry.extract_files();
        let display_title = entry.display_title();
        let runtime = entry.runtime_minutes();

        files
            .iter()
            .map(|f| self.score_file(&entry.hash, &display_title, f, runtime))
            .max_by(|a, b| {
                a.score
                    .cmp(&b.score)
                    .then_with(|| a.size_bytes.cmp(&b.size_bytes))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_terminator_131gb_remux_on_2gb_tv() {
        let scorer = StreamScorer::new(TvRamProfile::Light2Gb);
        let remux_file = TorrentFileStat {
            id: 1,
            path: "The.Terminator.1984.UHD.BluRay.2160p.Remux.mkv".to_string(),
            length: 141_689_763_896, // 131.96 GiB (реальный кейс с ТВ 172.30.0.176)
        };
        let scored = scorer.score_file(
            "203d9093be91c28af05b1590dafd73909c40e8f5",
            "[LAMPA] Терминатор",
            &remux_file,
            Some(108),
        );
        assert_eq!(scored.score, 0);
        assert_eq!(scored.verdict, StreamVerdict::TooHeavyRemux);
        assert!(scored.size_gb > 130.0);
    }

    #[test]
    fn rewards_optimal_9gb_webdl_on_2gb_tv() {
        let scorer = StreamScorer::new(TvRamProfile::Light2Gb);
        let optimal_file = TorrentFileStat {
            id: 1,
            path: "The.End.of.Oak.Street.2026.1080p.WEB.H.264.mkv".to_string(),
            length: 9_039_672_222, // ~8.42 GiB
        };
        let scored = scorer.score_file(
            "10add5afb218ac06a01a5c50f2cc4f107fca028c",
            "[NUM] The End of Oak Street (2026) WEB-DL 1080p",
            &optimal_file,
            Some(105),
        );
        assert_eq!(scored.verdict, StreamVerdict::Optimal);
        assert!(scored.score >= 80);
    }
}
