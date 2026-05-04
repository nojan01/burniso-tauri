# 🗺️ BurnISO to USB - Roadmap

## Version 1.2.0 (Abgeschlossen)

### 🎨 UX-Verbesserungen

- [x] **Drag & Drop für ISO-Dateien**
  - ISO-Datei direkt ins Fenster ziehen
  - Visuelles Feedback beim Ziehen (Drop-Zone)
  - Automatischer Tab-Wechsel zu "ISO → USB"

- [x] **Geschätzte Restzeit**
  - Anzeige: "~3:45 verbleibend"
  - Basierend auf aktueller Übertragungsgeschwindigkeit
  - Für Brennen, Backup und Diagnose

- [x] **macOS Benachrichtigungen**
  - Notification bei Abschluss von Vorgängen
  - "USB erfolgreich gebrannt! ✓"
  - Auch wenn App im Hintergrund

- [x] **Dock-Fortschritt**
  - macOS Dock-Icon zeigt Fortschrittsbalken
  - Wie bei Downloads im Finder
  - Nutzt Tauri Window setProgressBar API

- [x] **Letzte Dateien merken**
  - Zuletzt verwendete ISO-Dateien speichern
  - Schnellzugriff im Dropdown "Zuletzt verwendet"
  - Zuletzt verwendete Speicherorte für Backups

---

## Version 1.3.0 (Teilweise abgeschlossen)

### 🚀 Funktionale Erweiterungen

- [x] **USB Tools Tab**
  - Neuer "🛠️ USB Tools" Tab
  - Sammelt alle USB-Werkzeuge an einem Ort

- [x] **Bootfähigkeit prüfen**
  - Boot-Status-Analyse (EFI/MBR/Hybrid/El Torito)
  - Zeigt erkannte Boot-Typen und Signaturen
  - Info ob USB-Stick bootfähig ist

- [x] **USB-Stick Formatieren**
  - Wählbare Dateisysteme: FAT32, ExFAT, NTFS, APFS, HFS+, ext2, ext3, ext4
  - NTFS-Formatierung via Paragon NTFS (UFSD_NTFS)
  - ext2/3/4-Formatierung via Paragon extFS
  - Verschlüsselung für APFS und HFS+ (FileVault-kompatibel)
  - Partitionsschema: GPT oder MBR
  - Volume-Name anpassbar

- [x] **Sicheres Löschen**
  - 4 Sicherheitsstufen (0-4)
  - Schnell (1x Nullen) für SSD/Flash
  - Standard (1x Zufall)
  - DoE 3-Pass
  - Gutmann 35-Pass (nur HDD)

- [ ] **ISO-Vorschau**
  - Vor dem Brennen anzeigen:
    - Dateisystem (ISO9660, UDF)
    - Boot-Typ (UEFI/Legacy/Hybrid)
    - OS-Erkennung (Ubuntu, Windows, Fedora, etc.)
    - Enthaltene Dateien (optional)

- [ ] **Fehlerhafte Sektoren behandeln**
  - Im Diagnose-Tab nach Full Test
  - Option: "Low-Level-Format" bei Fehlern
  - Warnung bei zu vielen defekten Sektoren

---

## Version 1.4.0 (Zukunft)

### 🔧 Erweiterte Features

- [ ] **Multiboot USB (Ventoy-ähnlich)**
  - Mehrere ISO-Images auf einem Stick
  - Boot-Menü beim Starten
  - ISOs hinzufügen/entfernen ohne neu zu brennen
  - Persistenz-Support für Linux

- [ ] **Intel Mac Support**
  - Zusätzlicher `x86_64` Build
  - Oder: Universal Binary (arm64 + x86_64)
  - CI/CD Pipeline für beide Architekturen

- [ ] **Windows ISO Optimierung**
  - Automatische NTFS-Formatierung für >4GB install.wim
  - Split install.wim für FAT32 Kompatibilität
  - Rufus-ähnliche Windows-spezifische Optionen

---

## ✅ Abgeschlossen

### Version 1.4.0 (04.05.2026)
- [x] **Code-Review Härtung (siehe `CODE_REVIEW.md`)**
  - **Sicherheit (kritisch):** Strikte CSP, XSS-Schutz via `escapeHtml`, Passwort nicht mehr in Shell-Strings (`sudo_sh`-Helper, `child.stdin`), `unwrap()`-Pfade auf saubere Fehlerpropagation umgestellt, `ensure_disk_unmounted` vor Schreibzugriffen.
  - **Concurrency/UI:** Singleton-Lock für Passwort-Modal, Frontend-Validierung von ISO-/Backup-Pfaden, `operation_id` an `progress`/`diagnose_progress`-Events (verspätete Events einer abgebrochenen Operation werden verworfen), Watchdog-Timeout auf `diskutil`-Mount-Roundtrip im Verify-Pfad.
  - **Robustheit:** `diskutil … -plist` statt brittlem Text-Scraping in `is_removable_media`/`get_disk_details`, `tokio::time::sleep` in async-Kontexten, unnötige Passwort-Clones entfernt, DevTools im Release deaktiviert.
  - **i18n:** Log-Ausgaben (`logBurn`/`logBackup`) in beide Sprachen lokalisiert (`logs.*`-Namespace), Disk-Listing-Meldungen folgen UI-Sprache.
  - **Housekeeping:** `.gitignore` für sensitive Dateien, Debug-`eprintln!` nur noch unter `#[cfg(debug_assertions)]`, `cargo clippy` warnungsfrei.

### Version 1.3.1 (20.12.2024)
- [x] **Erweiterte Forensik-Analyse**
  - **Paragon NTFS für Windows-Filesysteme:**
    - Vollständiger Lese-/Schreibzugriff auf NTFS-Partitionen
    - Automatische Erkennung wenn Paragon NTFS installiert ist
    - Kommerzielle Lösung (empfohlen für NTFS-Analyse)
  - **Paragon extFS für Linux-Filesysteme:**
    - Vollständiger Lese-/Schreibzugriff auf ext2/ext3/ext4-Partitionen
    - Automatische Erkennung wenn Paragon extFS installiert ist
    - Kommerzielle Lösung (empfohlen für Linux-Partitionsanalyse)
  - **Nativ unterstützte Filesysteme:**
    - NTFS: Paragon NTFS (Lesen/Schreiben) oder Nur-Lesen (nativ macOS)
    - ext2/3/4: Paragon extFS (Lesen/Schreiben)
    - exFAT: Nativ macOS (volle Unterstützung)
    - FAT32: Nativ macOS (volle Unterstützung)
    - APFS/HFS+: Nativ macOS (volle Unterstützung)

### Version 1.1.0 (18.12.2024)
- [x] USB Diagnose Tab
- [x] Surface Scan (nicht-destruktiv)
- [x] Volltest (destruktiv, 2 Pattern)
- [x] Geschwindigkeitstest
- [x] S.M.A.R.T. Status Anzeige
- [x] Echtzeit-Statistiken
- [x] Optimierte Blockgröße (64MB)

### Version 1.0.0 (17.12.2024)
- [x] ISO auf USB brennen
- [x] USB Backup erstellen (Raw + Filesystem)
- [x] Verifizierung nach dem Brennen
- [x] Mehrsprachig (DE/EN)
- [x] Dark/Light Mode
- [x] Tastenkürzel
- [x] Fensterposition speichern

---

## 📊 Prioritäten

| Priorität | Feature | Aufwand | Version |
|-----------|---------|---------|---------|
| 🔴 Hoch | Drag & Drop | 🟢 Gering | 1.2.0 ✅ |
| 🔴 Hoch | Geschätzte Restzeit | 🟢 Gering | 1.2.0 ✅ |
| 🟡 Mittel | Benachrichtigungen | 🟢 Gering | 1.2.0 ✅ |
| 🟡 Mittel | Dock-Fortschritt | 🟡 Mittel | 1.2.0 ✅ |
| 🟡 Mittel | Letzte Dateien | 🟢 Gering | 1.2.0 ✅ |
| 🟡 Mittel | USB Tools Tab | 🟡 Mittel | 1.3.0 ✅ |
| 🟡 Mittel | USB Formatieren | 🟡 Mittel | 1.3.0 ✅ |
| 🟡 Mittel | Sicheres Löschen | 🟡 Mittel | 1.3.0 ✅ |
| 🟡 Mittel | Bootfähigkeit prüfen | 🟡 Mittel | 1.3.0 ✅ |
| 🟢 Niedrig | ISO-Vorschau | 🟡 Mittel | 1.3.0 |
| 🟢 Niedrig | Intel Mac Build | 🟢 Gering | 1.4.0 |
| 🟢 Niedrig | Multiboot | 🔴 Hoch | 1.4.0 |
