---
name: tv-media-control
description: >-
  Use this skill when working with Google TV ADB control, TorrServe MatriX API (:8090),
  stream bitrate/size scoring for Realtek RTD2851M (2 GB RAM), waking the TV from
  screensaver (dreamx), or launching movies in Just Player, Lampa, or NUM.
---

# TV Media Control & Stream Scoring Runbook

Используй этот навык для настройки и отладки воспроизведения видео без лагов на тестовом телевизоре `172.30.0.176` и любых других ТВ в домашнем Wi-Fi.

## 1. Паспорт тестового телевизора (`172.30.0.176`)
- **Модель:** TCL Smart TV Pro, шасси `R51MT05` (`BeyondTV4`), Android 11 (SDK 30).
- **Чипсет и память:** 4-ядерный **Realtek RTD2851M**, **2 ГБ RAM** (`MemTotal ~1.43–1.8 ГБ`, `zram0: 1 ГБ`).
- **Путь к ADB на ПК:** `adb` (или `%LOCALAPPDATA%\Microsoft\WinGet\Packages\Google.PlatformTools_Microsoft.Winget.Source_8wekyb3d8bbwe\platform-tools\adb.exe`).

## 2. Авто-профилирование и скоринг торрентов (`StreamScorer`)
Причина лагов на 2 ГБ ТВ (кейс «Терминатор 1»): торрент `203d9093be91c28af05b1590dafd73909c40e8f5` весил **131.96 ГБ (174.93 Мбит/с)** и загрузил `torrserver` на **126% CPU**.

### Профили ТВ по объёму ОЗУ (`MemTotal`):
- **`Light2Gb` (`MemTotal <= 2200 МБ`):**
  - Идеальный размер фильма: **`6.0 ..= 18.0 ГБ`** (`+40` баллов)
  - Допустимый размер: **`2.0 .. 6.0 ГБ`** или **`18.0 ..= 28.0 ГБ`** (`+20` баллов)
  - Свыше **`28.0 ГБ`** (тяжёлые 4K Blu-Ray Remux): **отбраковка (`0` баллов, статус `TooHeavyRemux`)**
- **`Standard3Gb` (`2200..=3500 МБ`):** идеал `8.0 ..= 30.0 ГБ`, потолок `45.0 ГБ`.
- **`Ultra4GbPlus` (`> 3500 МБ`):** идеал `15.0 ..= 60.0 ГБ`, потолок `90.0 ГБ`.

## 3. API локального `TorrServe MatriX.147` (`http://172.30.0.176:8090`)
- **Проверка жизни:** `GET /echo` -> `MatriX.147`
- **Список торрентов в базе ТВ:**
  ```json
  POST /torrents
  {"action": "list"}
  ```
  Возвращает массив объектов с полями `hash`, `title`, `torrent_size`, `data` (строка JSON с `TorrServer.Files: [{id, path, length}]`).
- **Прямая ссылка на видеопоток для плеера:**
  `http://172.30.0.176:8090/stream/<url_encoded_filename>?link=<hash>&index=<file_id>&play`

## 4. Пробуждение из заставки (`dreamx`) и запуск плеера
Если на ТВ активен скринсейвер (`com.google.android.apps.tv.dreamx`), обычный `am start` не выведет картинку.
Строгая последовательность:
1. Проверить `dumpsys window windows | grep -E 'mCurrentFocus|mFocusedApp'`.
2. Если в фокусе `dreamx` (или экран выключен):
   - `adb shell input keyevent 224` (`KEYCODE_WAKEUP`)
   - пауза 400 мс
   - `adb shell input keyevent 4` (`KEYCODE_BACK`)
3. Запустить поток в **Just Player**:
   ```text
   adb shell am start -a android.intent.action.VIEW -d "<STREAM_URL>" -t "video/*" -n net.gtvbox.videoplayer/.PlayerActivity --es title "<TITLE>"
   ```
