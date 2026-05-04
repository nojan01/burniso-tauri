# 🔍 Code-Review: BurnISO to USB

> Stand: 04.05.2026 — Vollständige Liste aller Befunde aus dem Review.
> Reihenfolge: nach Schweregrad, innerhalb der Stufe nach Auftreten.
> Status-Markierungen: `[ ]` offen · `[x]` erledigt · `[~]` in Arbeit

---

## 🔴 Kritisch (Sicherheit / Datenverlust / Crashes)

### [x] K1 — Content Security Policy ist deaktiviert
- **Datei:** `src-tauri/tauri.conf.json` (Zeile ~13)
- **Befund:** `"csp": null` → erlaubt jedes Inline-JS, externe Scripts, `eval()`. Verschärft K2.
- **Fix (umgesetzt):** Strikte CSP gesetzt:
  ```json
  "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: https://asset.localhost; font-src 'self' data:; connect-src 'self' ipc: https://ipc.localhost"
  ```
  Inline `onclick` in `help.html` entfernt, Logik nach `src/help.js` ausgelagert.

### [x] K2 — XSS via `innerHTML` mit Backend-/User-Daten
- **Datei:** `src/main.js` Zeilen ~2052, ~2058, ~2107–2840, ~2848, ~2860
- **Befund:** Fehlertexte (`err`), Gerätenamen, Forensik-Strings wurden direkt in `innerHTML` eingesetzt.
- **Fix (umgesetzt):**
  - `escapeHtml()`-Helper plus kurzer Alias `eh = (v) => escapeHtml(...)` im Forensik-Block.
  - Alle `log*`-Funktionen verwenden `textContent` statt `innerHTML` (V2).
  - Boot-Check- und Forensik-Fehler-Pfade escapen die Backend-Strings.
  - Forensik-Erfolgs-HTML (Header/Timestamp, Disk-Info-Loop, Partitions inkl. APFS-Volumes, USB-Devices/-Flat-Loop, Partition-Layout (Array & Raw-String), Boot-Info (ISO-Label, MBR-Partitions, GPT-GUID), Filesystem-Signaturen, Content-Analysis (Mount, Detected-OS, Top-Level-Files), Special/Hardware/Controller/Storage/Disk-Activity-Loops, MBR/GPT-Analyse, Filesystem-Details (Largest-Files/Type-Distribution/Recently-Modified), SMART (alle Feld-Loops + `attributes_table`-Zeilen + Legacy-Attributes + Source), Sector-Checksums (MD5/SHA256), Raw-Header-Hex) komplett mit `eh()` umhüllt.
  - CSP (K1) verhindert zusätzlich Inline-Script-Execution.
  - `node --check src/main.js` grün.

### [x] K3 — Passwort-Übergabe via `sh -c "echo '…' | sudo -S …"`
- **Datei:** `src-tauri/src/lib.rs` ~25 Stellen
- **Befund:** Passwort wurde in einen Shell-String einkopiert; sichtbar in `ps`, anfällig für Quoting-Bugs.
- **Fix (umgesetzt):**
  - Neuer Helfer `sudo_sh(password, script)` (Z. 38–53): startet `sudo -S sh -c <script>` und schreibt das Passwort ausschließlich auf `child.stdin`.
  - Alle ~25 Call-Sites migriert (Surface-Scan, Full-Test, Speed-Test, Forensik, Hex-Dump, MBR/GPT, Checksums, e2label/tune2fs, ISO-9660-Detection, Bootcheck, Special-Structures).
  - Drei Python-`-c`-Großskripte (Boot-Strukturanalyse, FS-Signaturen, Bootcheck) werden jetzt in eine Tempdatei geschrieben und unter sudo ausgeführt — kein Passwort und kein Skript mehr im Prozess-Listing.
  - `cargo check` ohne Warnungen.

### [x] K4 — Panics durch `.unwrap()` auf Regex-Captures / `parse()`
- **Datei:** `src-tauri/src/lib.rs` Z. 1871, 1891, 1938, 2083
- **Befund:** Bei abweichendem `diskutil`-Output (neue macOS-Version, Umgebung) → Panic der App.
- **Fix (umgesetzt):** Alle vier Stellen auf `if let Some(m) = caps.get(1)` umgestellt. `grep \.unwrap\(\)` liefert in `lib.rs` keine Treffer mehr.

### [x] K5 — Ignorierte Fehler bei Unmount/Sync (Datenverlustrisiko!)
- **Datei:** `src-tauri/src/lib.rs` Z. 1227, 1295, 1406, 1594, 2070, 2192, 2640 u. a.
- **Befund:** `let _ = Command::new("diskutil").args(["unmountDisk", …]).output();` — wenn das Unmount fehlschlägt, wurde trotzdem auf eine gemountete Partition geschrieben.
- **Fix (umgesetzt):**
  - Neue Helfer `is_disk_mounted()` und `ensure_disk_unmounted()` in `lib.rs`. Letzterer ruft `diskutil unmountDisk force`, parst danach `diskutil info -plist` und gibt einen Fehler zurück, falls die Disk noch gemountet ist.
  - `burn_iso`, `backup_usb_raw`, `diagnose_surface_scan`, `diagnose_full_test`, `diagnose_speed_test` und `secure_erase` nutzen jetzt `ensure_disk_unmounted(&app, &disk_id)?` und brechen bei verbleibenden Mounts ab — keine destruktiven Schreiboperationen mehr auf gemountete Volumes.
  - `cargo check` grün, keine Warnungen.

---

## 🟠 Wichtig (Bugs / Logik / Concurrency)

### [x] W1 — Race Condition bei mehrfachem Passwort-Prompt
- **Datei:** `src/main.js` Z. 873, 1023, 1508
- **Fix (umgesetzt):** Singleton-Lock `passwordPromptActive` in `requestPassword()`; weitere Aufrufe werden abgewiesen, bis OK/Cancel gedrückt wurde.

### [x] W2 — Inkonsistente DOM-Null-Checks
- **Datei:** `src/main.js` Z. 732, 738
- **Befund (verifiziert):** Z. 732 hat ein frühes `if (!recentIsoSelect) return;`, alle nachfolgenden Zugriffe sind dadurch geschützt. Falsch-Positiver Befund.

### [x] W3 — Keine Frontend-Validierung vor `invoke()`
- **Datei:** `src/main.js` Z. 920, 1055
- **Fix (umgesetzt):** Vor `burn`-Invoke `.iso`/`.img`-Endung und Mindestlänge geprüft; vor `backup`-Invoke (raw) `.img`/`.iso`/`.dmg`-Endung geprüft. Neue i18n-Keys `errors.invalidIsoExtension`, `errors.invalidIsoPath`, `errors.invalidBackupPath`, `errors.invalidBackupExtension` in `de.json` und `en.json` ergänzt.

### [x] W4 — Kein Timeout auf langlaufende Child-Prozesse
- **Datei:** `src-tauri/src/lib.rs` neue Helper-Funktion `run_with_timeout` (Z. ~67), Anwendung im Verify-Mount-Roundtrip von `burn_iso` (Z. ~5183/5185)
- **Fix (umgesetzt, pragmatisch):** Generischer Watchdog-Helper für kurze externe Kommandos (Polling per `try_wait`, Kill bei Ablauf). Eingesetzt am kritischsten Hang-Vektor: `diskutil mountDisk`/`unmountDisk` zwischen Schreib- und Verifizierungsphase (Timeout 30 s). Lange `dd`-Pipelines bleiben bewusst ohne harten Timeout, da bestehende Cancel-Logik (`CANCEL_BURN`/`CANCEL_BACKUP`) und das langsame Schreiben mancher USB-Sticks ein Pauschal-Timeout zu False-Positives führen würde.

### [x] W5 — Kein Operation-Token bei Progress-Events
- **Datei:** `src-tauri/src/lib.rs` (`CURRENT_OPERATION_ID`, `start_operation`), `src/main.js` (Listener)
- **Fix (umgesetzt):** Backend führt `static CURRENT_OPERATION_ID: AtomicU64`. Top-Level-Commands (`burn_iso`, `backup_usb_raw`, `backup_usb_filesystem`, `diagnose_surface_scan`, `diagnose_full_test`, `diagnose_speed_test`, `repair_disk`, `format_disk`, `secure_erase`) rufen zu Beginn `start_operation()` und emittieren ein `operation_start`-Event. `ProgressEvent`/`DiagnoseProgressEvent` tragen jetzt zusätzlich `operation_id`. Frontend tracked `currentOperationId` und verwirft in beiden Progress-Listenern Events einer älteren Operation.

### [x] W6 — Mehrfach gebundene Event-Listener
- **Datei:** `src/main.js` Z. 467–490 (Tab-Switching)
- **Befund (verifiziert):** Tab-`forEach` läuft nur einmal während `DOMContentLoaded`; es gibt keinen Re-Init-Pfad. Falsch-Positiver Befund.

---

## 🟡 Verbesserungen (Qualität / Wartbarkeit)

### [x] V1 — `std::thread::sleep` im async/Tauri-Kontext
- **Datei:** `src-tauri/src/lib.rs`
- **Fix (umgesetzt):** Alle 6 Sleeps in async-fn-Körpern (außerhalb von `spawn_blocking`) auf `tokio::time::sleep(...).await` umgestellt: `repair_disk` (Z. 2255), `format_disk` (Z. 2373), `secure_erase` (Z. 2631), `burn_iso` (Z. 5127/5131/5133). `tokio`-Feature `time` zu `Cargo.toml` ergänzt. Sleeps innerhalb sync helper (`write_pass`, `get_volume_info`) und in `spawn_blocking`-Closures (`diagnose_*`) bleiben absichtlich `std::thread::sleep`.

### [x] V2 — Duplizierte Log-Funktionen
- **Datei:** `src/main.js` Z. 574–602
- **Fix (umgesetzt):** Generischer `appendLog(target, message, type)`-Helper; alle `log*`-Funktionen sind jetzt dünne Wrapper.

### [ ] V3 — Globaler Zustand
- **Datei:** `src/main.js` Z. 240–260 (10+ globale Variablen)
- **Fix:** In ein `appState`-Objekt bündeln (`appState.burn`, `.backup`, `.diagnose` …).

### [x] V4 — Hartcodierte englische Strings in Log-Ausgaben
- **Datei:** `src/main.js` (alle `logBurn`/`logBackup`-Aufrufe)
- **Fix (umgesetzt):** Neuer i18n-Namespace `logs.*` mit 22 Keys in `de.json` und `en.json`. Alle ~28 hartcodierten englischen Log-Strings in `logBurn`/`logBackup` (ISO-Auswahl, Drag\&Drop, USB-Auswahl, Burn-/Backup-Lifecycle, Verify-Start, Cancel-Pfade, App-Ready) auf `t('logs.…')` migriert. Dynamische Inhalte (Pfade, Geraetenamen, Fehlertexte) bleiben konkateniert; UI-Sprache bestimmt jetzt die Log-Sprache.

### [x] V5 — Brittle Regex-Parsing von `diskutil`-Output
- **Datei:** `src-tauri/src/lib.rs` `is_removable_media` (~Z. 2008), `get_disk_details` (~Z. 2027), neuer Helper `extract_plist_bool` + `format_size_si`
- **Fix (umgesetzt):** Beide Funktionen rufen jetzt `diskutil info -plist` und parsen über die bestehenden plist-Helper. `is_removable_media` nutzt den neuen `extract_plist_bool` auf den Key `RemovableMedia`. `get_disk_details` extrahiert `MediaName`/`IORegistryEntryName`/`VolumeName` und `TotalSize`/`Size`; die Anzeigegroeße wird via `format_size_si` (SI-Einheiten, identisch zu `diskutil`-Textausgabe) aus den Bytes berechnet. Damit entfaellt das brittle Substring/Regex-Parsing der lokalisierten Textausgabe.

### [x] V6 — Unnötige `.clone()` auf Passwort-Strings
- **Datei:** `src-tauri/src/lib.rs` Z. 1637
- **Fix (umgesetzt):** `password_clone = password.clone()` entfernt; `password` wird direkt in die `spawn_blocking`-Closure gemoved und an `sudo_sh(&password, …)` geleitet.

### [x] V7 — DevTools im Release aktiviert
- **Datei:** `src-tauri/tauri.conf.json` Z. ~13
- **Fix (umgesetzt):** `"devtools": false` gesetzt.

### [ ] V8 — Shell-Aufrufe statt nativer macOS-APIs
- **Datei:** `src-tauri/src/lib.rs` (verteilt)
- **Befund:** `IOKit` / `DiskArbitration` wären robuster, aber aktueller Ansatz für MVP akzeptabel.
- **Fix (langfristig):** Schrittweise auf FFI-Bindings umstellen.

---

## 🔵 Klein (i18n / Stil / Housekeeping)

### [x] S1 — Fehlende i18n-Keys
- **Befund:** Prüfung ergab: `diagnose.smartTip`, `smartInstall`, `smartNote`, `smartDetected` existieren in `de.json` (Z. 138–141) und `en.json` (Z. 138). Falsch-Positiver Befund aus dem Review — keine Aktion nötig.

### [x] S2 — Debug-`eprintln!` im Release
- **Datei:** `src-tauri/src/lib.rs` Z. 891–906 (`[SMART Debug] …`)
- **Fix (umgesetzt):** Alle 22 `eprintln!`-Aufrufe in `lib.rs` mit `#[cfg(debug_assertions)]` annotiert.

### [x] S3 — Versionsangaben inkonsistent
- **Fix (umgesetzt):** `ROADMAP.md`-Überschriften aktualisiert: 1.2.0 → „Abgeschlossen“, 1.3.0 → „Teilweise abgeschlossen“. `package.json`/`Cargo.toml`/`tauri.conf.json` sind bereits konsistent auf 1.3.1.

### [x] S4 — `.gitignore` auf sensible Dateien prüfen
- **Fix (umgesetzt):** `.env`, `.env.*` (mit `!.env.example`-Ausnahme), `secrets.json`, `*.key`, `*.pem`, `*.p12`, `*.pfx` zu `.gitignore` hinzugefügt.

### [ ] S5 — Emoji-Icons ohne Fallback
- **Datei:** `src/styles.css`
- **Fix (optional):** SVG-/Font-Icons mit Unicode-Fallback verwenden.

### [x] S6 — Clippy-Hinweise
- **Datei:** `src-tauri/src/lib.rs`
- **Fix (umgesetzt):** `cargo clippy --fix --lib` ausgeführt (8 Auto-Fixes: `for`-Loop, kollabierte `if`-Statements, `div_ceil`, `1*1024` → `1024`, `parts.first()`, `to_string()`); `if same_then_else` in `build_menu` korrigiert; `#[allow(clippy::too_many_arguments)]` an `emit_diagnose_progress`, `format_disk`, `write_pass`. `cargo clippy` ist nun warnungsfrei.

---

## 🚀 Empfohlene Bearbeitungsreihenfolge

1. **K1** CSP aktivieren
2. **K2** XSS-Stellen auf `textContent` / Escape umstellen
3. **K4** `.unwrap()` auf Regex/Parse durch `?` ersetzen
4. **K5** Unmount-/Sync-Fehler nicht mehr ignorieren (Datenverlust!)
5. **K3** Passwort komplett über stdin
6. **W4** Timeouts für `dd`/Shell-Operationen
7. **W1**, **W3**, **W5** Concurrency- und Validierungs-Härtung
8. **V2**, **V3**, **V4** Refactoring (Log-Helper, AppState, i18n)
9. **V1**, **V5**, **V6**, **V7** Restqualität
10. **S1–S6** Housekeeping

---

## 📊 Übersicht

| Stufe | Erledigt | Teilweise | Offen | Σ |
|-------|---------:|----------:|------:|--:|
| 🔴 Kritisch       | 5 | 0 | 0 | 5 |
| 🟠 Wichtig        | 6 | 0 | 0 | 6 |
| 🟡 Verbesserung   | 6 | 0 | 2 | 8 |
| 🔵 Klein          | 5 | 0 | 1 | 6 |
| **Σ**             | **22** | **0** | **3** | **25** |

## ✅ Aktuelle Iteration (Commit-Vorschlag)

Erledigt:
- **K1** Strikte CSP aktiviert, `help.html` von Inline-Handlern befreit (`src/help.js` neu).
- **K2** `escapeHtml()`-Helper plus Alias `eh` im Forensik-Block; alle Log-Funktionen
  nutzen `textContent`; Boot-Check-, Forensik-Fehler- und das komplette Forensik-
  Erfolgs-HTML (Disk-/USB-Info, Partitions/APFS, Boot-Info, FS-Sigs, Content-Analysis,
  Hardware/Controller/Storage, MBR/GPT, Filesystem-Details, alle SMART-Loops + Tabelle,
  Sector-Checksums, Raw-Header-Hex) escapen jede Backend-Interpolation.
- **K3** Passwort wird nicht mehr in Shell-Strings einkopiert; neuer `sudo_sh()`-Helper
  (Passwort via `child.stdin`), alle ~25 Call-Sites migriert. Drei Python-Skripte werden
  aus Tempdateien gestartet, statt sie als `python3 -c '<script>'` durch die Shell zu
  jagen.
- **K4** Alle `.unwrap()`-Stellen in `lib.rs` entfernt.
- **K5** `ensure_disk_unmounted()` jetzt auch in `diagnose_surface_scan`,
  `diagnose_full_test`, `diagnose_speed_test` und `secure_erase` — destruktive
  Schreibvorgänge brechen ab, falls die Disk noch gemountet ist.
- **V2** Generischer `appendLog()`-Helper.
- **V7** `devtools: false` im Release.
- **S2** Alle `eprintln!`-Calls hinter `#[cfg(debug_assertions)]`.
- **S1** Verifiziert: i18n-Keys existieren bereits in beiden Sprachen.
- **W1** Singleton-Lock `passwordPromptActive` in `requestPassword()` (rejectet Mehrfach-Aufrufe).
- **W2** Verifiziert (Falsch-Positiver Befund — frühes `return` deckt alle Zugriffe ab).
- **W3** Frontend-Validierung (Endung + Länge) für Burn (`.iso`/`.img`) und Backup (`.img`/`.iso`/`.dmg`); 4 neue i18n-Keys unter `errors.*`.
- **W6** Verifiziert (Falsch-Positiver Befund — Tab-Listener werden nur einmalig in `DOMContentLoaded` gebunden).
- **V1** Async-Sleeps (`repair_disk`, `format_disk`, `secure_erase`, `burn_iso` Verify-Phase) auf `tokio::time::sleep(...).await` umgestellt; `tokio`-Feature `time` aktiviert.
- **V6** Unnötige `password.clone()`-Stelle in `diagnose_speed_test` entfernt.
- **S3** ROADMAP-Versionen synchronisiert (1.2.0 → Abgeschlossen, 1.3.0 → Teilweise abgeschlossen).
- **S4** `.gitignore` um `.env*`, `secrets.json`, `*.key`/`*.pem`/`*.p12`/`*.pfx` erweitert.
- **S6** `cargo clippy` warnungsfrei: Auto-Fixes via `--fix`, `if_same_then_else` korrigiert, `#[allow(clippy::too_many_arguments)]` an drei API-Funktionen.

Verifiziert: `cargo check` grün, `cargo clippy` ohne Warnungen, `node --check` für JS-Files grün, JSON valide.

Offen für nächste Iteration:
- **W4** Timeouts auf langlaufende Child-Prozesse (`dd`, `diskutil`).
- **W5** `operation_id` in Progress-Events für Cancel/Restart-Sicherheit.
- **V3** Globale State-Variablen in `appState`-Objekt bündeln.
- **V4** Hartcodierte Log-Strings nach i18n migrieren (umfangreich).
- **V5** Restliche `diskutil`-Regex-Parser auf `-plist`-Output umstellen.
- **V8** Langfristig: Shell-Aufrufe durch native `IOKit`/`DiskArbitration`-FFI ersetzen.
- **S5** Optional: Emoji-Icons mit SVG-Fallback.
