# 🗺️ BurnISO to USB - Roadmap

## Version 1.2.0 (In Arbeit)

### 🎨 UX-Verbesserungen

- [x] **Drag & Drop für ISO-Dateien**
  - ISO-Datei direkt ins Fenster ziehen
  - Visuelles Feedback beim Ziehen (Drop-Zone)
  - Automatischer Tab-Wechsel zu "ISO → USB"

- [ ] **Geschätzte Restzeit**
  - Anzeige: "Noch ca. 3:45 verbleibend"
  - Basierend auf aktueller Übertragungsgeschwindigkeit
  - Für Brennen, Backup und Diagnose

- [ ] **macOS Benachrichtigungen**
  - Notification bei Abschluss von Vorgängen
  - "USB erfolgreich gebrannt! ✓"
  - Auch wenn App im Hintergrund

- [ ] **Dock-Fortschritt**
  - macOS Dock-Icon zeigt Fortschrittsbalken
  - Wie bei Downloads im Finder
  - Erfordert `tauri-plugin-dock` oder native API

- [ ] **Letzte Dateien merken**
  - Zuletzt verwendete ISO-Dateien speichern
  - Schnellzugriff im Menü "Ablage → Zuletzt verwendet"
  - Zuletzt verwendete Speicherorte für Backups

---

## Version 1.3.0 (Geplant)

### 🚀 Funktionale Erweiterungen

- [ ] **Bootfähigkeit prüfen**
  - Nach dem Brennen automatisch prüfen
  - EFI/MBR-Partitionstabelle analysieren
  - Warnung wenn nicht bootfähig

- [ ] **ISO-Vorschau**
  - Vor dem Brennen anzeigen:
    - Dateisystem (ISO9660, UDF)
    - Boot-Typ (UEFI/Legacy/Hybrid)
    - OS-Erkennung (Ubuntu, Windows, Fedora, etc.)
    - Enthaltene Dateien (optional)

- [ ] **USB-Stick Formatieren**
  - Neuer Tab oder Button im Diagnose-Tab
  - Wählbare Dateisysteme: FAT32, ExFAT, APFS, HFS+
  - Optionen: Schnellformat, Sicher löschen
  - Partitionsschema: GPT oder MBR

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
| 🔴 Hoch | Drag & Drop | 🟢 Gering | 1.2.0 |
| 🔴 Hoch | Geschätzte Restzeit | 🟢 Gering | 1.2.0 |
| 🟡 Mittel | Benachrichtigungen | 🟢 Gering | 1.2.0 |
| 🟡 Mittel | Dock-Fortschritt | 🟡 Mittel | 1.2.0 |
| 🟡 Mittel | Letzte Dateien | 🟢 Gering | 1.2.0 |
| 🟡 Mittel | USB Formatieren | 🟡 Mittel | 1.3.0 |
| 🟡 Mittel | Bootfähigkeit prüfen | 🟡 Mittel | 1.3.0 |
| 🟢 Niedrig | ISO-Vorschau | 🟡 Mittel | 1.3.0 |
| 🟢 Niedrig | Intel Mac Build | 🟢 Gering | 1.4.0 |
| 🟢 Niedrig | Multiboot | 🔴 Hoch | 1.4.0 |
