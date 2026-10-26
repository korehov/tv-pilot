---
name: tv-pilot-scaffold
description: >-
  Use this skill when creating, scaffolding, building, or extending the TV Pilot
  Rust Cargo Workspace (tv-pilot-core and tv-pilot-cli) in E:\Project\TV Pilot.
---

# TV Pilot Rust Workspace Architecture & Scaffolding

Используй этот навык при генерации или доработке кода проекта `TV Pilot` в `E:\Project\TV Pilot`.

## 1. Структура Cargo Workspace

```text
E:\Project\TV Pilot\
├── AGENTS.md
├── Cargo.toml                # Workspace (resolver = "2", edition = "2024")
├── tv-pilot.toml             # Конфиг профиля ТВ (172.30.0.176)
├── .agents/
│   ├── rules/lan-and-safety.md
│   └── skills/
│       ├── tv-pilot-scaffold/SKILL.md
│       ├── tv-media-control/SKILL.md
│       └── tv-security-audit/SKILL.md
└── crates/
    ├── tv-pilot-core/        # Чистое кроссплатформенное ядро (ПК, macOS, Android)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── config.rs     # Конфиг + проверка RFC 1918 (LAN Only Guard)
    │       ├── error.rs      # TvError (thiserror)
    │       ├── adb.rs        # ADB-транспорт, wake_up(), launch_stream(), auto_profile()
    │       ├── torrserve.rs  # Клиент TorrServe MatriX API (:8090)
    │       ├── media.rs      # StreamScorer (выбор потока 6-18 ГБ без лагов)
    │       ├── security.rs   # Аудит APK, разрешений, wg0 VPN, фильтр файлов торрента
    │       └── optimizer.rs  # Мониторинг RAM/CPU, очистка фона (am kill-all), scan_lan()
    └── tv-pilot-cli/         # Консольный клиент (поддержка флага --json)
        ├── Cargo.toml
        └── src/
            └── main.rs       # CLI команды: status, security-audit, torrents, watch, boost, scan-lan
```

## 2. Зависимости (`Cargo.toml`)
- `tokio = { version = "1", features = ["full"] }`
- `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }`
- `serde = { version = "1.0", features = ["derive"] }`
- `serde_json = "1.0"`
- `toml = "0.8"`
- `thiserror = "2.0"`
- `anyhow = "1.0"`
- `clap = { version = "4.5", features = ["derive"] }`
- `urlencoding = "2.1"`

## 3. Команды `tv-pilot-cli`
Каждая команда должна поддерживать глобальный флаг `--json` для машинного чтения:
1. `tv-pilot status` — связь по ADB, модель ТВ, авто-профиль ОЗУ (`MemAvailable` / `MemTotal`), статус TorrServe (`MatriX.147`), активное окно.
2. `tv-pilot security-audit` — отчёт безопасности (сторонние APK против белого списка, опасные разрешения, статус `wg0`, проверка настроек TorrServe).
3. `tv-pilot torrents` — список торрентов в памяти локального TorrServe с оценкой `StreamScorer` (`✅ Оптимально` / `❌ Тяжёлый Remux`).
4. `tv-pilot watch [QUERY_OR_HASH]` — будит ТВ, чистит ОЗУ, проверяет файлы торрента, выбирает лучший поток и запускает в `Just Player`.
5. `tv-pilot boost` — освобождение фоновой ОЗУ (`am kill-all`) + установка безопасного RAM-кэша TorrServe (`64 МБ`, `UseDisk: false`).
6. `tv-pilot scan-lan` — асинхронный поиск телевизоров в локальной подсети `/24` по портам `5555` и `8090`.

## 4. Шаги верификации
1. Запусти модульные тесты: `cargo test --workspace`
2. Проверь линтером: `cargo clippy --workspace -- -D warnings`
3. Проверь вживую на тестовом ТВ `172.30.0.176`:
   - `cargo run -- status`
   - `cargo run -- security-audit`
   - `cargo run -- torrents`
