// burnISOtoUSB - Tauri Backend
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri::menu::{Menu, MenuItem, Submenu, PredefinedMenuItem, AboutMetadata};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiskInfo {
    pub id: String,
    pub name: String,
    pub size: String,
    pub bytes: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VolumeInfo {
    pub identifier: String,
    pub mount_point: String,
    pub filesystem: String,
    pub name: String,
    pub bytes: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgressEvent {
    pub percent: u32,
    pub status: String,
    pub operation: String,
}

/// Detected filesystem information from raw device reading
#[derive(Debug, Clone)]
struct DetectedFilesystem {
    name: String,
    label: Option<String>,
    used_bytes: Option<u64>,
    total_bytes: Option<u64>,
}

/// Detect filesystem by reading raw device signatures
/// This works even for filesystems macOS doesn't natively support
fn detect_filesystem_from_device(disk_id: &str) -> Option<DetectedFilesystem> {
    let device_path = format!("/dev/r{}", disk_id); // Use raw device for direct access
    
    let mut file = File::open(&device_path).ok()?;
    let mut buffer = vec![0u8; 131072]; // 128KB buffer for various superblocks
    
    file.read_exact(&mut buffer).ok()?;
    
    // Check for various filesystem signatures
    
    // 1. NTFS: "NTFS    " at offset 3
    if buffer.len() > 10 && &buffer[3..11] == b"NTFS    " {
        let label = extract_ntfs_label(&buffer);
        let (total, used) = extract_ntfs_size(&buffer);
        return Some(DetectedFilesystem {
            name: "NTFS".to_string(),
            label,
            used_bytes: used,
            total_bytes: total,
        });
    }
    
    // 2. EXT2/3/4: Magic number 0xEF53 at offset 1080 (0x438)
    if buffer.len() > 1082 && buffer[0x438] == 0x53 && buffer[0x439] == 0xEF {
        let (fs_type, label, total, used) = extract_ext_info(&buffer);
        return Some(DetectedFilesystem {
            name: fs_type,
            label,
            used_bytes: used,
            total_bytes: total,
        });
    }
    
    // 3. FAT32: "FAT32   " at offset 82
    if buffer.len() > 90 && &buffer[82..90] == b"FAT32   " {
        let label = extract_fat_label(&buffer, 71);
        return Some(DetectedFilesystem {
            name: "FAT32".to_string(),
            label,
            used_bytes: None,
            total_bytes: None,
        });
    }
    
    // 4. FAT16: "FAT16   " or "FAT12   " at offset 54
    if buffer.len() > 62 {
        if &buffer[54..62] == b"FAT16   " {
            let label = extract_fat_label(&buffer, 43);
            return Some(DetectedFilesystem {
                name: "FAT16".to_string(),
                label,
                used_bytes: None,
                total_bytes: None,
            });
        }
        if &buffer[54..62] == b"FAT12   " {
            let label = extract_fat_label(&buffer, 43);
            return Some(DetectedFilesystem {
                name: "FAT12".to_string(),
                label,
                used_bytes: None,
                total_bytes: None,
            });
        }
    }
    
    // 5. exFAT: "EXFAT   " at offset 3
    if buffer.len() > 11 && &buffer[3..11] == b"EXFAT   " {
        return Some(DetectedFilesystem {
            name: "exFAT".to_string(),
            label: None,
            used_bytes: None,
            total_bytes: None,
        });
    }
    
    // 6. ISO 9660: "CD001" at offset 32769 (0x8001) - need to read more
    if let Ok(mut f) = File::open(&device_path) {
        let mut iso_buf = vec![0u8; 6];
        if f.seek(SeekFrom::Start(0x8001)).is_ok() && f.read_exact(&mut iso_buf).is_ok() {
            if &iso_buf[0..5] == b"CD001" {
                return Some(DetectedFilesystem {
                    name: "ISO 9660".to_string(),
                    label: extract_iso_label(&device_path),
                    used_bytes: None,
                    total_bytes: None,
                });
            }
        }
    }
    
    // 7. Btrfs: "_BHRfS_M" at offset 0x10040
    if let Ok(mut f) = File::open(&device_path) {
        let mut btrfs_buf = vec![0u8; 8];
        if f.seek(SeekFrom::Start(0x10040)).is_ok() && f.read_exact(&mut btrfs_buf).is_ok() {
            if &btrfs_buf == b"_BHRfS_M" {
                return Some(DetectedFilesystem {
                    name: "Btrfs".to_string(),
                    label: None,
                    used_bytes: None,
                    total_bytes: None,
                });
            }
        }
    }
    
    // 8. XFS: "XFSB" at offset 0
    if buffer.len() > 4 && &buffer[0..4] == b"XFSB" {
        return Some(DetectedFilesystem {
            name: "XFS".to_string(),
            label: extract_xfs_label(&buffer),
            used_bytes: None,
            total_bytes: None,
        });
    }
    
    None
}

fn extract_ntfs_label(_buffer: &[u8]) -> Option<String> {
    // NTFS volume label is in the $Volume file, not easily accessible from boot sector
    // We'd need to parse the MFT which is complex - return None for now
    None
}

fn extract_ntfs_size(buffer: &[u8]) -> (Option<u64>, Option<u64>) {
    if buffer.len() < 0x30 {
        return (None, None);
    }
    // Bytes per sector at offset 0x0B (2 bytes, little-endian)
    let bytes_per_sector = u16::from_le_bytes([buffer[0x0B], buffer[0x0C]]) as u64;
    // Sectors per cluster at offset 0x0D (1 byte) - not needed for total size calc
    let _sectors_per_cluster = buffer[0x0D] as u64;
    // Total sectors at offset 0x28 (8 bytes, little-endian)
    let total_sectors = u64::from_le_bytes([
        buffer[0x28], buffer[0x29], buffer[0x2A], buffer[0x2B],
        buffer[0x2C], buffer[0x2D], buffer[0x2E], buffer[0x2F],
    ]);
    
    let total_bytes = total_sectors * bytes_per_sector;
    // Used bytes would require reading $Bitmap - return None
    (Some(total_bytes), None)
}

fn extract_ext_info(buffer: &[u8]) -> (String, Option<String>, Option<u64>, Option<u64>) {
    let superblock_offset = 0x400; // 1024 bytes
    
    // Determine EXT version from feature flags
    let fs_type = if buffer.len() > superblock_offset + 0x60 {
        let incompat_features = u32::from_le_bytes([
            buffer[superblock_offset + 0x60],
            buffer[superblock_offset + 0x61],
            buffer[superblock_offset + 0x62],
            buffer[superblock_offset + 0x63],
        ]);
        // INCOMPAT_EXTENTS = 0x40 indicates EXT4
        if incompat_features & 0x40 != 0 {
            "EXT4"
        } else if buffer.len() > superblock_offset + 0xE0 {
            // Check for journal (EXT3)
            let compat_features = u32::from_le_bytes([
                buffer[superblock_offset + 0x5C],
                buffer[superblock_offset + 0x5D],
                buffer[superblock_offset + 0x5E],
                buffer[superblock_offset + 0x5F],
            ]);
            if compat_features & 0x04 != 0 { "EXT3" } else { "EXT2" }
        } else {
            "EXT2"
        }
    } else {
        "EXT"
    };
    
    // Extract volume label (16 bytes at offset 0x78 in superblock)
    let label = if buffer.len() > superblock_offset + 0x88 {
        let label_bytes = &buffer[superblock_offset + 0x78..superblock_offset + 0x88];
        let label_str: String = label_bytes.iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if label_str.is_empty() { None } else { Some(label_str) }
    } else {
        None
    };
    
    // Calculate size
    let (total, used) = if buffer.len() > superblock_offset + 0x28 {
        let block_count = u32::from_le_bytes([
            buffer[superblock_offset + 0x04],
            buffer[superblock_offset + 0x05],
            buffer[superblock_offset + 0x06],
            buffer[superblock_offset + 0x07],
        ]) as u64;
        let free_blocks = u32::from_le_bytes([
            buffer[superblock_offset + 0x0C],
            buffer[superblock_offset + 0x0D],
            buffer[superblock_offset + 0x0E],
            buffer[superblock_offset + 0x0F],
        ]) as u64;
        let log_block_size = u32::from_le_bytes([
            buffer[superblock_offset + 0x18],
            buffer[superblock_offset + 0x19],
            buffer[superblock_offset + 0x1A],
            buffer[superblock_offset + 0x1B],
        ]);
        let block_size = 1024u64 << log_block_size;
        
        let total_bytes = block_count * block_size;
        let used_bytes = (block_count - free_blocks) * block_size;
        (Some(total_bytes), Some(used_bytes))
    } else {
        (None, None)
    };
    
    (fs_type.to_string(), label, total, used)
}

fn extract_fat_label(buffer: &[u8], offset: usize) -> Option<String> {
    if buffer.len() > offset + 11 {
        let label_bytes = &buffer[offset..offset + 11];
        let label: String = label_bytes.iter()
            .map(|&b| b as char)
            .collect::<String>()
            .trim()
            .to_string();
        if label.is_empty() || label == "NO NAME" { None } else { Some(label) }
    } else {
        None
    }
}

fn extract_iso_label(device_path: &str) -> Option<String> {
    // ISO 9660 volume label is at offset 32808 (0x8028), 32 bytes
    let mut file = File::open(device_path).ok()?;
    file.seek(SeekFrom::Start(0x8028)).ok()?;
    let mut label_buf = vec![0u8; 32];
    file.read_exact(&mut label_buf).ok()?;
    let label: String = label_buf.iter()
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string();
    if label.is_empty() { None } else { Some(label) }
}

fn extract_xfs_label(buffer: &[u8]) -> Option<String> {
    // XFS label is at offset 0x6C, 12 bytes
    if buffer.len() > 0x6C + 12 {
        let label_bytes = &buffer[0x6C..0x6C + 12];
        let label: String = label_bytes.iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if label.is_empty() { None } else { Some(label) }
    } else {
        None
    }
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    
    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WindowState {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}

fn get_window_state_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
    PathBuf::from(home)
        .join("Library/Application Support/com.burniso.usb")
        .join("window_state.json")
}

#[tauri::command]
fn get_window_state() -> Option<WindowState> {
    let path = get_window_state_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str::<WindowState>(&content) {
                return Some(state);
            }
        }
    }
    None
}

#[tauri::command]
fn save_window_state(width: u32, height: u32, x: i32, y: i32) -> Result<(), String> {
    let path = get_window_state_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let state = WindowState { width, height, x, y };
    let content = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

static CANCEL_BURN: AtomicBool = AtomicBool::new(false);
static CANCEL_BACKUP: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn list_disks() -> Result<Vec<DiskInfo>, String> {
    // "external physical" zeigt nur echte physische externe Geräte (keine Disk-Images)
    let output = Command::new("diskutil").args(["list", "external", "physical"]).output()
        .map_err(|e| format!("diskutil Fehler: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut disks: Vec<DiskInfo> = Vec::new();
    for line in stdout.lines() {
        if line.starts_with("/dev/disk") {
            if let Some(caps) = regex_lite::Regex::new(r"/dev/(disk\d+)")
                .ok().and_then(|re| re.captures(line)) {
                let disk_id = caps.get(1).unwrap().as_str().to_string();
                if !disks.iter().any(|d| d.id == disk_id) {
                    if let Ok(info) = get_disk_details(&disk_id) {
                        disks.push(info);
                    }
                }
            }
        }
    }
    Ok(disks)
}

fn get_disk_details(disk_id: &str) -> Result<DiskInfo, String> {
    let output = Command::new("diskutil").args(["info", disk_id]).output()
        .map_err(|e| format!("diskutil info Fehler: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut name = "Unknown Device".to_string();
    let mut size = "Unknown Size".to_string();
    for line in stdout.lines() {
        if line.contains("Device / Media Name:") {
            if let Some(val) = line.split(':').nth(1) {
                name = val.trim().to_string();
            }
        } else if line.contains("Disk Size:") || line.contains("Total Size:") {
            if let Some(caps) = regex_lite::Regex::new(r"([\d.]+\s*[KMGT]?B)")
                .ok().and_then(|re| re.captures(line)) {
                size = caps.get(1).unwrap().as_str().to_string();
            }
        }
    }
    let plist_output = Command::new("diskutil").args(["info", "-plist", disk_id]).output().ok();
    let bytes = plist_output.and_then(|o| {
        let plist_str = String::from_utf8_lossy(&o.stdout);
        extract_plist_value(&plist_str, "TotalSize").or_else(|| extract_plist_value(&plist_str, "Size"))
    });
    Ok(DiskInfo { id: disk_id.to_string(), name, size, bytes })
}

fn extract_plist_value(plist: &str, key: &str) -> Option<u64> {
    let key_pattern = format!("<key>{}</key>", key);
    let mut found_key = false;
    for line in plist.lines() {
        if found_key {
            if let Some(start) = line.find("<integer>") {
                if let Some(end) = line.find("</integer>") {
                    return line[start + 9..end].parse().ok();
                }
            }
            found_key = false;
        }
        if line.contains(&key_pattern) {
            found_key = true;
        }
    }
    None
}

fn extract_plist_string(plist: &str, key: &str) -> Option<String> {
    let key_pattern = format!("<key>{}</key>", key);
    let mut found_key = false;
    for line in plist.lines() {
        if found_key {
            if let (Some(start), Some(end)) = (line.find("<string>"), line.find("</string>")) {
                let val = line[start + 8..end].to_string();
                if !val.is_empty() {
                    return Some(val);
                }
            }
            found_key = false;
        }
        if line.contains(&key_pattern) {
            found_key = true;
        }
    }
    None
}

#[tauri::command]
fn get_disk_info(disk_id: String) -> Result<String, String> {
    let output = Command::new("diskutil").args(["info", &disk_id]).output()
        .map_err(|e| format!("diskutil Fehler: {}", e))?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
fn get_volume_info(disk_id: String) -> Result<Option<VolumeInfo>, String> {
    let supported_fs = ["APFS", "Apple_APFS", "HFS+", "Mac OS Extended", "FAT32", "ExFAT", "Apple_HFS"];
    let iso_fs = ["ISO 9660", "cd9660", "ISO9660", "ISO", "UDF"];
    
    // Hilfsfunktion um Partition/Disk zu prüfen (macOS-native Erkennung)
    let check_disk = |part_id: &str| -> Option<VolumeInfo> {
        let o = Command::new("diskutil").args(["info", "-plist", part_id]).output().ok()?;
        let plist = String::from_utf8_lossy(&o.stdout);
        let mount = extract_plist_string(&plist, "MountPoint");
        let fs = extract_plist_string(&plist, "FilesystemName")
            .or_else(|| extract_plist_string(&plist, "FilesystemUserVisibleName"))
            .or_else(|| extract_plist_string(&plist, "Content")).unwrap_or_default();
        
        if let Some(ref mp) = mount {
            if !mp.is_empty() && std::path::Path::new(mp).exists() {
                let is_iso = iso_fs.iter().any(|s| fs.contains(s));
                if is_iso || supported_fs.iter().any(|s| fs.contains(s)) {
                    let display_fs = if is_iso { format!("ISO:{}", fs) } else { fs };
                    return Some(VolumeInfo {
                        identifier: part_id.to_string(),
                        mount_point: mp.clone(),
                        filesystem: display_fs,
                        name: extract_plist_string(&plist, "VolumeName").unwrap_or_else(|| "USB-Volume".to_string()),
                        bytes: extract_plist_value(&plist, "TotalSize"),
                    });
                }
            }
        }
        None
    };
    
    // Hilfsfunktion für raw filesystem detection (für nicht-gemountete Partitionen)
    let check_disk_raw = |part_id: &str| -> Option<VolumeInfo> {
        if let Some(detected) = detect_filesystem_from_device(part_id) {
            // Get size from diskutil even if filesystem is not mounted
            let o = Command::new("diskutil").args(["info", "-plist", part_id]).output().ok()?;
            let plist = String::from_utf8_lossy(&o.stdout);
            let bytes = detected.total_bytes.or_else(|| extract_plist_value(&plist, "TotalSize"));
            
            // Build filesystem display string with usage info
            let fs_display = if let (Some(used), Some(total)) = (detected.used_bytes, detected.total_bytes) {
                format!("{} ({} / {} belegt)", detected.name, format_bytes(used), format_bytes(total))
            } else if let Some(total) = detected.total_bytes {
                format!("{} ({})", detected.name, format_bytes(total))
            } else {
                detected.name.clone()
            };
            
            let name = detected.label.unwrap_or_else(|| {
                extract_plist_string(&plist, "VolumeName")
                    .unwrap_or_else(|| format!("{} Volume", detected.name))
            });
            
            return Some(VolumeInfo {
                identifier: part_id.to_string(),
                mount_point: String::new(), // Not mounted
                filesystem: fs_display,
                name,
                bytes,
            });
        }
        None
    };
    
    // Zuerst Partitionen prüfen (diskXsY)
    let output = Command::new("diskutil").args(["list", &disk_id]).output()
        .map_err(|e| format!("diskutil Fehler: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(caps) = regex_lite::Regex::new(r"(disk\d+s\d+)").ok().and_then(|re| re.captures(line)) {
            let part_id = caps.get(1).unwrap().as_str();
            // Try macOS native first
            if let Some(info) = check_disk(part_id) {
                return Ok(Some(info));
            }
            // Then try raw detection for unsupported filesystems
            if let Some(info) = check_disk_raw(part_id) {
                return Ok(Some(info));
            }
        }
    }
    
    // Falls keine Partition gefunden, die Hauptdisk selbst prüfen
    if let Some(info) = check_disk(&disk_id) {
        return Ok(Some(info));
    }
    // Try raw detection on main disk
    if let Some(info) = check_disk_raw(&disk_id) {
        return Ok(Some(info));
    }
    
    Ok(None)
}

#[tauri::command]
fn cancel_burn() {
    CANCEL_BURN.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn cancel_backup() {
    CANCEL_BACKUP.store(true, Ordering::SeqCst);
}

fn emit_progress(app: &AppHandle, percent: u32, status: &str, operation: &str) {
    let _ = app.emit("progress", ProgressEvent {
        percent,
        status: status.to_string(),
        operation: operation.to_string(),
    });
}

#[tauri::command]
async fn burn_iso(app: AppHandle, iso_path: String, disk_id: String, password: String, verify: bool, eject: bool) -> Result<String, String> {
    CANCEL_BURN.store(false, Ordering::SeqCst);
    let iso_size = std::fs::metadata(&iso_path).map_err(|e| format!("ISO nicht gefunden: {}", e))?.len();
    
    let _ = app.emit("burn_phase", "writing");
    emit_progress(&app, 0, "Vorbereitung...", "burn");
    
    let disk_path = format!("/dev/{}", disk_id);
    let rdisk_path = format!("/dev/r{}", disk_id);
    
    emit_progress(&app, 0, "Unmount Disk...", "burn");
    let _ = Command::new("diskutil").args(["unmountDisk", &disk_path]).output();
    
    emit_progress(&app, 0, "Schreibe ISO auf USB...", "burn");
    
    let python_script = format!(
        r#"import os, sys
iso_path = "{}"
disk_path = "{}"
buffer_size = 1024 * 1024
total_size = {}
copied = 0
try:
    with open(iso_path, 'rb') as src:
        fd = os.open(disk_path, os.O_WRONLY)
        with os.fdopen(fd, 'wb', buffering=0) as dst:
            while True:
                chunk = src.read(buffer_size)
                if not chunk: break
                dst.write(chunk)
                copied += len(chunk)
                print(f"BYTES:{{copied}}", flush=True)
            dst.flush()
            os.fsync(dst.fileno())
except OSError as exc:
    print(f"ERROR: {{exc}}", file=sys.stderr)
    sys.exit(1)
print("WRITE_SUCCESS", flush=True)"#, iso_path.replace('"', r#"\""#), rdisk_path, iso_size);

    let mut child = Command::new("sudo").args(["-S", "python3", "-c", &python_script])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|e| format!("Fehler beim Starten: {}", e))?;
    
    if let Some(ref mut stdin) = child.stdin {
        writeln!(stdin, "{}", password).ok();
    }
    
    let stdout = child.stdout.take().ok_or("Kein stdout")?;
    let reader = BufReader::new(stdout);
    let mut write_success = false;
    
    for line in reader.lines().map_while(Result::ok) {
        if CANCEL_BURN.load(Ordering::SeqCst) {
            let _ = child.kill();
            return Err("Brennvorgang abgebrochen".to_string());
        }
        if let Some(stripped) = line.strip_prefix("BYTES:") {
            if let Ok(bytes) = stripped.parse::<u64>() {
                let percent = ((bytes as f64 / iso_size as f64) * 100.0) as u32;
                emit_progress(&app, percent.min(100), &format!("SCHREIBEN: {}%", percent.min(100)), "burn");
            }
        } else if line.contains("WRITE_SUCCESS") {
            write_success = true;
        }
    }
    
    let status = child.wait().map_err(|e| format!("Prozess Fehler: {}", e))?;
    
    if !status.success() || !write_success {
        let _ = app.emit("burn_phase", "error");
        return Err("Brennvorgang fehlgeschlagen".to_string());
    }
    
    if verify {
        let _ = app.emit("burn_phase", "verifying");
        emit_progress(&app, 0, "Synchronisiere Daten...", "burn");
        
        // Wichtig: Cache leeren und Disk neu einbinden für zuverlässige Verifizierung
        let _ = Command::new("sync").output();
        std::thread::sleep(std::time::Duration::from_secs(2));
        
        // Disk kurz einhängen und wieder aushängen, um gepufferte Daten zu schreiben
        let _ = Command::new("diskutil").args(["mountDisk", &disk_path]).output();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = Command::new("diskutil").args(["unmountDisk", &disk_path]).output();
        std::thread::sleep(std::time::Duration::from_millis(500));
        
        emit_progress(&app, 0, "VERIFIZIEREN: 0%", "burn");
        
        let verify_script = format!(
            r#"import os, sys
iso_path = "{}"
disk_path = "{}"
buffer_size = 1024 * 1024
total_size = {}
verified = 0
errors = 0
try:
    with open(iso_path, 'rb') as iso_file:
        fd = os.open(disk_path, os.O_RDONLY)
        with os.fdopen(fd, 'rb', buffering=0) as disk_file:
            while verified < total_size:
                iso_chunk = iso_file.read(buffer_size)
                if not iso_chunk: break
                disk_chunk = disk_file.read(len(iso_chunk))
                if iso_chunk != disk_chunk:
                    errors += 1
                    print(f"MISMATCH:{{verified}}", flush=True)
                verified += len(iso_chunk)
                print(f"VERIFY:{{verified}}:{{errors}}", flush=True)
except OSError as exc:
    print(f"ERROR: {{exc}}", file=sys.stderr)
    sys.exit(1)
if errors == 0:
    print("VERIFY_SUCCESS", flush=True)
else:
    print(f"VERIFY_FAILED:{{errors}}", flush=True)
    sys.exit(1)"#, iso_path.replace('"', r#"\""#), rdisk_path, iso_size);

        let mut verify_child = Command::new("sudo").args(["-S", "python3", "-c", &verify_script])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
            .map_err(|e| format!("Verifizierung Fehler: {}", e))?;
        
        if let Some(ref mut stdin) = verify_child.stdin {
            writeln!(stdin, "{}", password).ok();
        }
        
        let verify_stdout = verify_child.stdout.take().ok_or("Kein stdout")?;
        let verify_reader = BufReader::new(verify_stdout);
        let mut verify_success = false;
        let mut verify_errors = 0u32;
        
        for line in verify_reader.lines().map_while(Result::ok) {
            if CANCEL_BURN.load(Ordering::SeqCst) {
                let _ = verify_child.kill();
                return Err("Verifizierung abgebrochen".to_string());
            }
            if let Some(stripped) = line.strip_prefix("VERIFY:") {
                let parts: Vec<&str> = stripped.split(':').collect();
                if let (Some(bytes_str), Some(err_str)) = (parts.first(), parts.get(1)) {
                    if let (Ok(bytes), Ok(errs)) = (bytes_str.parse::<u64>(), err_str.parse::<u32>()) {
                        let percent = ((bytes as f64 / iso_size as f64) * 100.0) as u32;
                        let status_msg = if errs > 0 {
                            format!("VERIFIZIEREN: {}% ({} Fehler)", percent.min(100), errs)
                        } else {
                            format!("VERIFIZIEREN: {}%", percent.min(100))
                        };
                        emit_progress(&app, percent.min(100), &status_msg, "burn");
                    }
                }
            } else if line.contains("VERIFY_SUCCESS") {
                verify_success = true;
            } else if let Some(stripped) = line.strip_prefix("VERIFY_FAILED:") {
                verify_errors = stripped.parse().unwrap_or(1);
            }
        }
        
        let _ = verify_child.wait();
        
        if !verify_success || verify_errors > 0 {
            let _ = app.emit("burn_phase", "error");
            emit_progress(&app, 100, &format!("FEHLER: {} Blöcke stimmen nicht überein!", verify_errors), "burn");
            if eject {
                let _ = Command::new("diskutil").args(["eject", &disk_path]).output();
            }
            return Err(format!("Verifizierung fehlgeschlagen: {} fehlerhafte Blöcke", verify_errors));
        }
    }
    
    let _ = app.emit("burn_phase", "success");
    emit_progress(&app, 100, "Fertig!", "burn");
    
    if eject {
        let _ = Command::new("diskutil").args(["eject", &disk_path]).output();
    } else {
        let _ = Command::new("diskutil").args(["mountDisk", &disk_path]).output();
    }
    
    if verify {
        Ok("ISO erfolgreich auf USB geschrieben und verifiziert".to_string())
    } else {
        Ok("ISO erfolgreich auf USB geschrieben".to_string())
    }
}

#[tauri::command]
async fn backup_usb_raw(app: AppHandle, disk_id: String, destination: String, disk_size: u64, password: String) -> Result<String, String> {
    CANCEL_BACKUP.store(false, Ordering::SeqCst);
    let disk_path = format!("/dev/{}", disk_id);
    let rdisk_path = format!("/dev/r{}", disk_id);
    emit_progress(&app, 0, "Unmount Disk...", "backup");
    let _ = Command::new("diskutil").args(["unmountDisk", &disk_path]).output();
    emit_progress(&app, 0, "Lese USB-Daten...", "backup");
    
    let python_script = format!(
        r#"import os, sys
raw_path = "{}"
out_path = "{}"
total_size = {}
buffer_size = 1024 * 1024
copied = 0
try:
    fd = os.open(raw_path, os.O_RDONLY)
except OSError as exc:
    print(f"ERROR: {{exc}}", file=sys.stderr)
    sys.exit(1)
try:
    with os.fdopen(fd, 'rb', buffering=0) as src, open(out_path, 'wb') as dst:
        remaining = total_size
        while remaining > 0:
            to_read = min(buffer_size, remaining)
            chunk = src.read(to_read)
            if not chunk: break
            dst.write(chunk)
            copied += len(chunk)
            remaining -= len(chunk)
            print(f"BYTES:{{copied}}", flush=True)
        dst.flush()
        os.fsync(dst.fileno())
except OSError as exc:
    print(f"ERROR: {{exc}}", file=sys.stderr)
    sys.exit(1)
print("SUCCESS", flush=True)"#, rdisk_path, destination.replace('"', r#"\""#), disk_size);

    let mut child = Command::new("sudo").args(["-S", "python3", "-c", &python_script])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|e| format!("Fehler beim Starten: {}", e))?;
    
    if let Some(ref mut stdin) = child.stdin {
        writeln!(stdin, "{}", password).ok();
    }
    
    let stdout = child.stdout.take().ok_or("Kein stdout")?;
    let reader = BufReader::new(stdout);
    
    for line in reader.lines().map_while(Result::ok) {
        if CANCEL_BACKUP.load(Ordering::SeqCst) {
            let _ = child.kill();
            return Err("Sicherung abgebrochen".to_string());
        }
        if let Some(stripped) = line.strip_prefix("BYTES:") {
            if let Ok(bytes) = stripped.parse::<u64>() {
                let percent = ((bytes as f64 / disk_size as f64) * 100.0) as u32;
                emit_progress(&app, percent.min(100), &format!("{}% gesichert", percent), "backup");
            }
        } else if line.contains("SUCCESS") {
            emit_progress(&app, 100, "Sicherung fertig!", "backup");
        }
    }
    
    let status = child.wait().map_err(|e| format!("Prozess Fehler: {}", e))?;
    let _ = Command::new("diskutil").args(["mountDisk", &disk_path]).output();
    
    if status.success() {
        Ok("USB-Stick erfolgreich gesichert".to_string())
    } else {
        Err("Sicherung fehlgeschlagen".to_string())
    }
}

#[tauri::command]
async fn backup_usb_filesystem(app: AppHandle, mount_point: String, destination: String, volume_name: String) -> Result<String, String> {
    CANCEL_BACKUP.store(false, Ordering::SeqCst);
    emit_progress(&app, 0, "Erstelle komprimiertes Image...", "backup");
    
    let mut child = Command::new("hdiutil")
        .args(["create", "-puppetstrings", "-format", "UDZO", "-volname", &volume_name, "-srcfolder", &mount_point, &destination])
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
        .map_err(|e| format!("hdiutil Fehler: {}", e))?;
    
    let stdout = child.stdout.take().ok_or("Kein stdout")?;
    let reader = BufReader::new(stdout);
    
    for line in reader.lines().map_while(Result::ok) {
        if CANCEL_BACKUP.load(Ordering::SeqCst) {
            let _ = child.kill();
            return Err("Sicherung abgebrochen".to_string());
        }
        if let Some(stripped) = line.strip_prefix("PERCENT:") {
            if let Ok(percent) = stripped.trim().parse::<f64>() {
                emit_progress(&app, percent as u32, &format!("{}% erstellt", percent as u32), "backup");
            }
        }
    }
    
    let status = child.wait().map_err(|e| format!("Prozess Fehler: {}", e))?;
    
    if status.success() {
        emit_progress(&app, 100, "Sicherung fertig!", "backup");
        Ok("Dateibasierte Sicherung abgeschlossen".to_string())
    } else {
        Err("hdiutil Sicherung fehlgeschlagen".to_string())
    }
}

// ========== Menu Building ==========

fn build_menu(app_handle: &AppHandle, lang: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (about_label, about_comments, hide_label, hide_others_label, show_all_label, quit_label) = if lang == "en" {
        ("About BurnISO to USB", "Burn ISO to USB & Backup USB", "Hide BurnISO to USB", "Hide Others", "Show All", "Quit BurnISO to USB")
    } else {
        ("Über BurnISO to USB", "ISO auf USB brennen & USB sichern", "BurnISO to USB ausblenden", "Andere ausblenden", "Alle einblenden", "BurnISO to USB beenden")
    };
    
    let (file_menu_label, select_iso_label, select_destination_label, refresh_label, close_label) = if lang == "en" {
        ("File", "Open ISO File...", "Choose Destination...", "Refresh USB Devices", "Close Window")
    } else {
        ("Ablage", "ISO-Datei öffnen...", "Speicherort wählen...", "USB-Geräte aktualisieren", "Fenster schließen")
    };
    
    let (action_menu_label, start_burn_label, start_backup_label, cancel_label) = if lang == "en" {
        ("Action", "Burn ISO to USB", "Backup USB", "Cancel Operation")
    } else {
        ("Aktion", "ISO auf USB brennen", "USB sichern", "Vorgang abbrechen")
    };
    
    let (window_menu_label, minimize_label, fullscreen_label) = if lang == "en" {
        ("Window", "Minimize", "Fullscreen")
    } else {
        ("Fenster", "Im Dock ablegen", "Vollbild")
    };
    
    let help_menu_label = if lang == "en" { "Help" } else { "Hilfe" };
    
    let about_metadata = AboutMetadata {
        name: Some("BurnISO to USB".to_string()),
        version: Some("1.0.0".to_string()),
        copyright: Some("© 2025 Norbert Jander".to_string()),
        comments: Some(about_comments.to_string()),
        ..Default::default()
    };
    
    // App-Menü
    let about = PredefinedMenuItem::about(app_handle, Some(about_label), Some(about_metadata))?;
    let separator = PredefinedMenuItem::separator(app_handle)?;
    let hide = PredefinedMenuItem::hide(app_handle, Some(hide_label))?;
    let hide_others = PredefinedMenuItem::hide_others(app_handle, Some(hide_others_label))?;
    let show_all = PredefinedMenuItem::show_all(app_handle, Some(show_all_label))?;
    let quit = PredefinedMenuItem::quit(app_handle, Some(quit_label))?;
    
    let app_menu = Submenu::with_items(
        app_handle,
        "BurnISO to USB",
        true,
        &[&about, &separator, &hide, &hide_others, &show_all, &PredefinedMenuItem::separator(app_handle)?, &quit],
    )?;
    
    // Ablage-Menü
    let select_iso = MenuItem::with_id(app_handle, "select_iso", select_iso_label, true, Some("CmdOrCtrl+O"))?;
    let select_destination = MenuItem::with_id(app_handle, "select_destination", select_destination_label, true, Some("CmdOrCtrl+S"))?;
    let refresh = MenuItem::with_id(app_handle, "refresh", refresh_label, true, Some("CmdOrCtrl+R"))?;
    let close = PredefinedMenuItem::close_window(app_handle, Some(close_label))?;
    
    let file_menu = Submenu::with_items(
        app_handle,
        file_menu_label,
        true,
        &[&select_iso, &select_destination, &PredefinedMenuItem::separator(app_handle)?, &refresh, &PredefinedMenuItem::separator(app_handle)?, &close],
    )?;
    
    // Aktion-Menü
    let tab_burn = MenuItem::with_id(app_handle, "tab_burn", "ISO → USB", true, Some("CmdOrCtrl+1"))?;
    let tab_backup = MenuItem::with_id(app_handle, "tab_backup", "USB → ISO", true, Some("CmdOrCtrl+2"))?;
    let start_burn = MenuItem::with_id(app_handle, "start_burn", start_burn_label, true, Some("CmdOrCtrl+B"))?;
    let start_backup = MenuItem::with_id(app_handle, "start_backup", start_backup_label, true, Some("CmdOrCtrl+Shift+B"))?;
    let cancel_action = MenuItem::with_id(app_handle, "cancel_action", cancel_label, true, Some("CmdOrCtrl+."))?;
    
    let action_menu = Submenu::with_items(
        app_handle,
        action_menu_label,
        true,
        &[&tab_burn, &tab_backup, &PredefinedMenuItem::separator(app_handle)?, &start_burn, &start_backup, &PredefinedMenuItem::separator(app_handle)?, &cancel_action],
    )?;
    
    // Fenster-Menü
    let minimize = PredefinedMenuItem::minimize(app_handle, Some(minimize_label))?;
    let fullscreen = PredefinedMenuItem::fullscreen(app_handle, Some(fullscreen_label))?;
    
    let theme_dark_label = if lang == "en" { "🌙 Dark Mode" } else { "🌙 Dunkles Design" };
    let theme_light_label = if lang == "en" { "☀️ Light Mode" } else { "☀️ Helles Design" };
    let theme_dark = MenuItem::with_id(app_handle, "theme_dark", theme_dark_label, true, Some("CmdOrCtrl+Shift+D"))?;
    let theme_light = MenuItem::with_id(app_handle, "theme_light", theme_light_label, true, Some("CmdOrCtrl+Shift+L"))?;
    
    let window_menu = Submenu::with_items(
        app_handle,
        window_menu_label,
        true,
        &[&minimize, &fullscreen, &PredefinedMenuItem::separator(app_handle)?, &theme_dark, &theme_light],
    )?;
    
    // Hilfe-Menü
    let help_label = if lang == "en" { "Help" } else { "Hilfe" };
    let github = MenuItem::with_id(app_handle, "github", "GitHub Repository", true, None::<&str>)?;
    let help_item = MenuItem::with_id(app_handle, "help", help_label, true, Some("CmdOrCtrl+?"))?;
    let lang_german = MenuItem::with_id(app_handle, "lang_de", "🇩🇪 Deutsch", true, None::<&str>)?;
    let lang_english = MenuItem::with_id(app_handle, "lang_en", "🇬🇧 English", true, None::<&str>)?;
    
    let help_menu = Submenu::with_items(
        app_handle,
        help_menu_label,
        true,
        &[&help_item, &PredefinedMenuItem::separator(app_handle)?, &github, &PredefinedMenuItem::separator(app_handle)?, &lang_german, &lang_english],
    )?;
    
    let menu = Menu::with_items(
        app_handle,
        &[&app_menu, &file_menu, &action_menu, &window_menu, &help_menu],
    )?;
    
    app_handle.set_menu(menu)?;
    
    Ok(())
}

#[tauri::command]
fn set_menu_language(app_handle: AppHandle, lang: String) -> Result<(), String> {
    build_menu(&app_handle, &lang).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            list_disks,
            get_disk_info,
            get_volume_info,
            burn_iso,
            backup_usb_raw,
            backup_usb_filesystem,
            cancel_burn,
            cancel_backup,
            get_window_state,
            save_window_state,
            set_menu_language
        ])
        .setup(|app| {
            let app_handle = app.handle();
            
            // Fensterposition wiederherstellen
            if let Some(window) = app.get_webview_window("main") {
                if let Some(state) = get_window_state() {
                    if state.width >= 700 && state.height >= 700 {
                        let _ = window.set_size(tauri::LogicalSize::new(state.width as f64, state.height as f64));
                    }
                    if state.x > -2000 && state.x < 5000 && state.y > -200 && state.y < 3000 {
                        let _ = window.set_position(tauri::LogicalPosition::new(state.x as f64, state.y as f64));
                    }
                }
            }
            
            // Menü erstellen (Deutsch als Standard)
            build_menu(app_handle, "de")?;
            
            // Menü-Events
            let app_handle_clone = app_handle.clone();
            app.on_menu_event(move |app, event| {
                let id = event.id().as_ref();
                if let Some(window) = app.get_webview_window("main") {
                    match id {
                        "refresh" => { let _ = window.emit("menu-action", "refresh"); }
                        "select_iso" => { let _ = window.emit("menu-action", "select_iso"); }
                        "select_destination" => { let _ = window.emit("menu-action", "select_destination"); }
                        "tab_burn" => { let _ = window.emit("menu-action", "tab_burn"); }
                        "tab_backup" => { let _ = window.emit("menu-action", "tab_backup"); }
                        "start_burn" => { let _ = window.emit("menu-action", "start_burn"); }
                        "start_backup" => { let _ = window.emit("menu-action", "start_backup"); }
                        "cancel_action" => { let _ = window.emit("menu-action", "cancel_action"); }
                        "lang_de" => {
                            let _ = build_menu(&app_handle_clone, "de");
                            let _ = window.emit("menu-action", "lang_de");
                        }
                        "lang_en" => {
                            let _ = build_menu(&app_handle_clone, "en");
                            let _ = window.emit("menu-action", "lang_en");
                        }
                        "theme_dark" => {
                            let _ = window.emit("menu-action", "theme_dark");
                        }
                        "theme_light" => {
                            let _ = window.emit("menu-action", "theme_light");
                        }
                        "help" => {
                            let _ = window.emit("menu-action", "help");
                        }
                        "github" => {
                            let _ = Command::new("open")
                                .arg("https://github.com/nojan01/burniso-tauri")
                                .spawn();
                        }
                        _ => {}
                    }
                }
            });
            
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
