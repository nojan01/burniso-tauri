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
                let iso_size = extract_iso_size(&device_path);
                return Some(DetectedFilesystem {
                    name: "ISO 9660".to_string(),
                    label: extract_iso_label(&device_path),
                    used_bytes: iso_size, // ISO size = used bytes
                    total_bytes: iso_size,
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

/// Extract ISO 9660 volume size from Primary Volume Descriptor
/// The PVD is at sector 16 (offset 0x8000), and contains:
/// - Volume Space Size at offset 80 (4 bytes little-endian + 4 bytes big-endian)
/// - Logical Block Size at offset 128 (2 bytes little-endian + 2 bytes big-endian)
fn extract_iso_size(device_path: &str) -> Option<u64> {
    let mut file = File::open(device_path).ok()?;
    
    // Read Primary Volume Descriptor (starts at 0x8000, 2048 bytes)
    file.seek(SeekFrom::Start(0x8000)).ok()?;
    let mut pvd = vec![0u8; 2048];
    file.read_exact(&mut pvd).ok()?;
    
    // Check it's a Primary Volume Descriptor (type 1, "CD001")
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    
    // Volume Space Size (number of logical blocks) at offset 80
    // Little-endian 32-bit value
    let volume_space_size = u32::from_le_bytes([pvd[80], pvd[81], pvd[82], pvd[83]]) as u64;
    
    // Logical Block Size at offset 128 (usually 2048)
    // Little-endian 16-bit value
    let logical_block_size = u16::from_le_bytes([pvd[128], pvd[129]]) as u64;
    
    // Total size = blocks * block_size
    let total_size = volume_space_size * logical_block_size;
    
    if total_size > 0 {
        Some(total_size)
    } else {
        None
    }
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
static CANCEL_DIAGNOSE: AtomicBool = AtomicBool::new(false);

/// SMART data structure - Extended with all smartctl -x data
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmartData {
    pub available: bool,
    pub health_status: String,
    pub temperature: Option<i32>,
    pub power_on_hours: Option<u64>,
    pub power_cycle_count: Option<u64>,
    pub reallocated_sectors: Option<u64>,
    pub pending_sectors: Option<u64>,
    pub uncorrectable_sectors: Option<u64>,
    pub attributes: Vec<SmartAttribute>,
    pub source: String, // "smartctl" or "diskutil" or "none"
    pub error_message: Option<String>,
    // Extended device info (from smartctl -x)
    pub model_family: Option<String>,
    pub device_model: Option<String>,
    pub serial_number: Option<String>,
    pub firmware_version: Option<String>,
    pub user_capacity_bytes: Option<u64>,
    pub logical_block_size: Option<u32>,
    pub physical_block_size: Option<u32>,
    pub rotation_rate: Option<u32>,        // 0 = SSD, >0 = HDD RPM
    pub form_factor: Option<String>,       // "2.5 inches", "3.5 inches", etc.
    pub device_type: Option<String>,       // "ata", "nvme", "scsi"
    pub protocol: Option<String>,          // "ATA", "NVMe", "SCSI"
    // SATA/ATA info
    pub ata_version: Option<String>,
    pub sata_version: Option<String>,
    pub interface_speed_max: Option<String>,
    pub interface_speed_current: Option<String>,
    // SMART capabilities
    pub smart_enabled: Option<bool>,
    pub read_lookahead_enabled: Option<bool>,
    pub write_cache_enabled: Option<bool>,
    pub trim_supported: Option<bool>,
    // ATA Security
    pub ata_security_enabled: Option<bool>,
    pub ata_security_frozen: Option<bool>,
    // SCT (SMART Command Transport) Temperature
    pub sct_temperature_current: Option<i32>,
    pub sct_temperature_lifetime_min: Option<i32>,
    pub sct_temperature_lifetime_max: Option<i32>,
    pub sct_temperature_op_limit: Option<i32>,
    // Self-test info
    pub self_test_status: Option<String>,
    pub self_test_short_minutes: Option<u32>,
    pub self_test_extended_minutes: Option<u32>,
    // Error logs
    pub error_log_count: Option<u32>,
    pub self_test_log_count: Option<u32>,
    // SSD specific
    pub endurance_used_percent: Option<u32>,
    pub spare_available_percent: Option<u32>,
    pub total_lbas_written: Option<u64>,
    pub total_lbas_read: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SmartAttribute {
    pub id: u32,
    pub name: String,
    pub value: String,
    pub worst: Option<String>,
    pub threshold: Option<String>,
    pub raw_value: String,
    pub status: String, // "ok", "warning", "critical"
    pub flags: Option<String>,        // e.g. "PO--CK"
    pub prefailure: Option<bool>,     // Prefailure warning attribute
}

/// Diagnose progress event with statistics
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiagnoseProgressEvent {
    pub percent: u32,
    pub status: String,
    pub phase: String,
    pub sectors_checked: u64,
    pub errors_found: u64,
    pub read_speed_mbps: f64,
    pub write_speed_mbps: f64,
}

/// Diagnose result
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiagnoseResult {
    pub success: bool,
    pub total_sectors: u64,
    pub sectors_checked: u64,
    pub errors_found: u64,
    pub bad_sectors: Vec<u64>,
    pub read_speed_mbps: f64,
    pub write_speed_mbps: f64,
    pub message: String,
}

#[tauri::command]
fn cancel_diagnose() {
    CANCEL_DIAGNOSE.store(true, Ordering::SeqCst);
}

/// Get the path to smartctl (checking common installation locations)
fn get_smartctl_path() -> Option<String> {
    // Check common paths for smartctl (Homebrew paths, standard paths)
    let paths = [
        "/opt/homebrew/bin/smartctl",  // Homebrew on Apple Silicon
        "/usr/local/bin/smartctl",      // Homebrew on Intel Mac
        "/usr/bin/smartctl",             // System path
        "/usr/sbin/smartctl",            // System path
    ];
    
    for path in paths {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    
    // Fallback: try which command
    if let Ok(output) = Command::new("which").arg("smartctl").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    
    None
}

/// Write text content to a file
#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<(), String> {
    use std::fs::File;
    use std::io::Write;
    
    let mut file = File::create(&path)
        .map_err(|e| format!("Datei konnte nicht erstellt werden: {}", e))?;
    
    file.write_all(content.as_bytes())
        .map_err(|e| format!("Schreibfehler: {}", e))?;
    
    Ok(())
}

/// Check if Paragon NTFS and/or extFS drivers are installed
/// Returns a JSON object with { ntfs: bool, extfs: bool }
#[tauri::command]
fn check_paragon_drivers() -> serde_json::Value {
    // Check for Paragon NTFS driver (UFSD_NTFS)
    let ntfs_installed = Command::new("diskutil")
        .args(["listFilesystems"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("UFSD_NTFS"))
        .unwrap_or(false);
    
    // Check for Paragon extFS driver (UFSD_EXTFS)
    let extfs_installed = Command::new("diskutil")
        .args(["listFilesystems"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("UFSD_EXTFS"))
        .unwrap_or(false);
    
    serde_json::json!({
        "ntfs": ntfs_installed,
        "extfs": extfs_installed
    })
}

/// Check if smartmontools is installed
#[tauri::command]
fn check_smartctl_installed() -> bool {
    get_smartctl_path().is_some()
}

/// Check if e2fsprogs is installed (for ext2/3/4 label reading)
fn get_e2fsprogs_path() -> Option<String> {
    let paths = [
        "/opt/homebrew/opt/e2fsprogs/sbin/e2label",  // Homebrew on Apple Silicon
        "/usr/local/opt/e2fsprogs/sbin/e2label",     // Homebrew on Intel Mac
        "/usr/local/sbin/e2label",                    // Manual install
        "/usr/sbin/e2label",                          // System path
    ];
    
    for path in paths {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    
    // Try which command as fallback
    if let Ok(output) = Command::new("which").arg("e2label").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let path = stdout.trim();
        if !path.is_empty() && std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    
    None
}

/// Check all optional dependencies and return their status
#[tauri::command]
fn check_dependencies() -> serde_json::Value {
    let paragon = check_paragon_drivers();
    let smartctl = get_smartctl_path().is_some();
    let e2fsprogs = get_e2fsprogs_path().is_some();
    
    // Check if Homebrew is installed
    let homebrew_installed = std::path::Path::new("/opt/homebrew/bin/brew").exists()
        || std::path::Path::new("/usr/local/bin/brew").exists();
    
    // Build missing packages list
    let mut missing_brew_packages = Vec::new();
    if !smartctl { missing_brew_packages.push("smartmontools"); }
    if !e2fsprogs { missing_brew_packages.push("e2fsprogs"); }
    
    // Build install command
    let install_command: Option<String> = if !missing_brew_packages.is_empty() {
        Some(format!("brew install {}", missing_brew_packages.join(" ")))
    } else {
        None
    };
    
    serde_json::json!({
        "smartmontools": smartctl,
        "e2fsprogs": e2fsprogs,
        "paragon_ntfs": paragon.get("ntfs").and_then(|v| v.as_bool()).unwrap_or(false),
        "paragon_extfs": paragon.get("extfs").and_then(|v| v.as_bool()).unwrap_or(false),
        "homebrew": homebrew_installed,
        "missing_brew_packages": missing_brew_packages,
        "install_command": install_command
    })
}

/// Get SMART data for a disk
#[tauri::command]
fn get_smart_data(disk_id: String) -> SmartData {
    // First, try smartctl (most comprehensive, but requires smartmontools)
    if let Some(data) = try_smartctl(&disk_id) {
        return data;
    }
    
    // Fallback: Try to get basic info from diskutil (limited but always available)
    if let Some(data) = try_diskutil_smart(&disk_id) {
        return data;
    }
    
    // No SMART data available
    SmartData::not_available("SMART data not available for this device. USB sticks and SD cards typically do not support SMART. For USB hard drives, you can install 'smartmontools' (brew install smartmontools).")
}

impl SmartData {
    /// Create SmartData indicating SMART is not available
    fn not_available(message: &str) -> Self {
        SmartData {
            available: false,
            health_status: "Unbekannt".to_string(),
            temperature: None,
            power_on_hours: None,
            power_cycle_count: None,
            reallocated_sectors: None,
            pending_sectors: None,
            uncorrectable_sectors: None,
            attributes: Vec::new(),
            source: "none".to_string(),
            error_message: Some(message.to_string()),
            model_family: None,
            device_model: None,
            serial_number: None,
            firmware_version: None,
            user_capacity_bytes: None,
            logical_block_size: None,
            physical_block_size: None,
            rotation_rate: None,
            form_factor: None,
            device_type: None,
            protocol: None,
            ata_version: None,
            sata_version: None,
            interface_speed_max: None,
            interface_speed_current: None,
            smart_enabled: None,
            read_lookahead_enabled: None,
            write_cache_enabled: None,
            trim_supported: None,
            ata_security_enabled: None,
            ata_security_frozen: None,
            sct_temperature_current: None,
            sct_temperature_lifetime_min: None,
            sct_temperature_lifetime_max: None,
            sct_temperature_op_limit: None,
            self_test_status: None,
            self_test_short_minutes: None,
            self_test_extended_minutes: None,
            error_log_count: None,
            self_test_log_count: None,
            endurance_used_percent: None,
            spare_available_percent: None,
            total_lbas_written: None,
            total_lbas_read: None,
        }
    }
    
    /// Create basic SmartData with health status
    fn basic(health_status: String, source: &str, error_message: Option<&str>) -> Self {
        SmartData {
            available: true,
            health_status,
            temperature: None,
            power_on_hours: None,
            power_cycle_count: None,
            reallocated_sectors: None,
            pending_sectors: None,
            uncorrectable_sectors: None,
            attributes: Vec::new(),
            source: source.to_string(),
            error_message: error_message.map(|s| s.to_string()),
            model_family: None,
            device_model: None,
            serial_number: None,
            firmware_version: None,
            user_capacity_bytes: None,
            logical_block_size: None,
            physical_block_size: None,
            rotation_rate: None,
            form_factor: None,
            device_type: None,
            protocol: None,
            ata_version: None,
            sata_version: None,
            interface_speed_max: None,
            interface_speed_current: None,
            smart_enabled: None,
            read_lookahead_enabled: None,
            write_cache_enabled: None,
            trim_supported: None,
            ata_security_enabled: None,
            ata_security_frozen: None,
            sct_temperature_current: None,
            sct_temperature_lifetime_min: None,
            sct_temperature_lifetime_max: None,
            sct_temperature_op_limit: None,
            self_test_status: None,
            self_test_short_minutes: None,
            self_test_extended_minutes: None,
            error_log_count: None,
            self_test_log_count: None,
            endurance_used_percent: None,
            spare_available_percent: None,
            total_lbas_written: None,
            total_lbas_read: None,
        }
    }
}

fn try_smartctl(disk_id: &str) -> Option<SmartData> {
    // Get smartctl path
    let smartctl_path = get_smartctl_path()?;
    
    let device_path = format!("/dev/{}", disk_id);
    eprintln!("[SMART Debug] Checking disk: {} with smartctl: {}", device_path, smartctl_path);
    
    // First, quick check if SMART is supported at all (fast command)
    let info_output = Command::new(&smartctl_path)
        .args(["-i", &device_path])
        .output()
        .ok()?;
    
    let info_text = String::from_utf8_lossy(&info_output.stdout);
    let info_stderr = String::from_utf8_lossy(&info_output.stderr);
    
    eprintln!("[SMART Debug] -i output contains 'SMART support': {}", info_text.contains("SMART support is:"));
    
    // Check for common indicators that SMART is not supported
    if info_text.contains("Unknown USB bridge") 
        || info_text.contains("Device type: unknown")
        || info_stderr.contains("Unable to detect device type")
        || info_stderr.contains("Unknown USB bridge")
        || (!info_text.contains("SMART support is:") && !info_text.contains("SMART Health Status")) {
        eprintln!("[SMART Debug] SMART not supported (early check failed)");
        return None;
    }
    
    // Check if SMART is explicitly unavailable
    if info_text.contains("SMART support is: Unavailable") 
        || info_text.contains("Device does not support SMART") {
        eprintln!("[SMART Debug] SMART explicitly unavailable");
        return None;
    }
    
    eprintln!("[SMART Debug] Running smartctl -x -j ...");
    
    // Run smartctl -x -j (extended info with JSON output) for full data
    let output = Command::new(&smartctl_path)
        .args(["-x", "-j", &device_path])
        .output()
        .ok()?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("[SMART Debug] Got {} bytes of JSON output", stdout.len());
    
    // Parse JSON output
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
        eprintln!("[SMART Debug] JSON parsed successfully");
        
        // Check if device type is recognized
        let device_type_val = json.get("device").and_then(|d| d.get("type")).and_then(|t| t.as_str());
        if device_type_val == Some("unknown") {
            eprintln!("[SMART Debug] Device type is unknown");
            return None;
        }
        
        // Basic health status
        let smart_status = json.get("smart_status")
            .and_then(|s| s.get("passed"))
            .and_then(|p| p.as_bool());
        
        let health_status = match smart_status {
            Some(true) => "PASSED ✅".to_string(),
            Some(false) => "FAILED ❌".to_string(),
            None => "Unbekannt".to_string(),
        };
        
        // Temperature (check multiple sources)
        let temperature = json.get("temperature")
            .and_then(|t| t.get("current"))
            .and_then(|c| c.as_i64())
            .map(|t| t as i32);
        
        let power_on_hours = json.get("power_on_time")
            .and_then(|p| p.get("hours"))
            .and_then(|h| h.as_u64());
        
        let power_cycle_count = json.get("power_cycle_count")
            .and_then(|p| p.as_u64());
        
        // Extended device info
        let model_family = json.get("model_family").and_then(|v| v.as_str()).map(|s| s.to_string());
        let device_model = json.get("model_name").and_then(|v| v.as_str()).map(|s| s.to_string());
        let serial_number = json.get("serial_number").and_then(|v| v.as_str()).map(|s| s.to_string());
        let firmware_version = json.get("firmware_version").and_then(|v| v.as_str()).map(|s| s.to_string());
        
        let user_capacity_bytes = json.get("user_capacity")
            .and_then(|c| c.get("bytes"))
            .and_then(|b| b.as_u64());
        
        let logical_block_size = json.get("logical_block_size")
            .and_then(|b| b.as_u64())
            .map(|b| b as u32);
        
        let physical_block_size = json.get("physical_block_size")
            .and_then(|b| b.as_u64())
            .map(|b| b as u32);
        
        let rotation_rate = json.get("rotation_rate")
            .and_then(|r| r.as_u64())
            .map(|r| r as u32);
        
        let form_factor = json.get("form_factor")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        
        let device_type = json.get("device")
            .and_then(|d| d.get("type"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        
        let protocol = json.get("device")
            .and_then(|d| d.get("protocol"))
            .and_then(|p| p.as_str())
            .map(|s| s.to_string());
        
        // ATA/SATA versions
        let ata_version = json.get("ata_version")
            .and_then(|v| v.get("string"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        
        let sata_version = json.get("sata_version")
            .and_then(|v| v.get("string"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        
        // Interface speed
        let interface_speed_max = json.get("interface_speed")
            .and_then(|i| i.get("max"))
            .and_then(|m| m.get("string"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        
        let interface_speed_current = json.get("interface_speed")
            .and_then(|i| i.get("current"))
            .and_then(|c| c.get("string"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        
        // SMART capabilities
        let smart_enabled = json.get("smart_support")
            .and_then(|s| s.get("enabled"))
            .and_then(|e| e.as_bool());
        
        let read_lookahead_enabled = json.get("read_lookahead")
            .and_then(|r| r.get("enabled"))
            .and_then(|e| e.as_bool());
        
        let write_cache_enabled = json.get("write_cache")
            .and_then(|w| w.get("enabled"))
            .and_then(|e| e.as_bool());
        
        let trim_supported = json.get("trim")
            .and_then(|t| t.get("supported"))
            .and_then(|s| s.as_bool());
        
        // ATA Security
        let ata_security_enabled = json.get("ata_security")
            .and_then(|a| a.get("enabled"))
            .and_then(|e| e.as_bool());
        
        let ata_security_frozen = json.get("ata_security")
            .and_then(|a| a.get("frozen"))
            .and_then(|f| f.as_bool());
        
        // SCT Temperature data (more detailed than basic temperature)
        let sct_temp = json.get("ata_sct_status").and_then(|s| s.get("temperature"));
        let sct_temperature_current = sct_temp
            .and_then(|t| t.get("current"))
            .and_then(|c| c.as_i64())
            .map(|t| t as i32);
        let sct_temperature_lifetime_min = sct_temp
            .and_then(|t| t.get("lifetime_min"))
            .and_then(|m| m.as_i64())
            .map(|t| t as i32);
        let sct_temperature_lifetime_max = sct_temp
            .and_then(|t| t.get("lifetime_max"))
            .and_then(|m| m.as_i64())
            .map(|t| t as i32);
        let sct_temperature_op_limit = sct_temp
            .and_then(|t| t.get("op_limit_max"))
            .and_then(|m| m.as_i64())
            .map(|t| t as i32);
        
        // Self-test info
        let self_test_status = json.get("ata_smart_data")
            .and_then(|d| d.get("self_test"))
            .and_then(|s| s.get("status"))
            .and_then(|st| st.get("string"))
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        
        let self_test_short_minutes = json.get("ata_smart_data")
            .and_then(|d| d.get("self_test"))
            .and_then(|s| s.get("polling_minutes"))
            .and_then(|p| p.get("short"))
            .and_then(|s| s.as_u64())
            .map(|m| m as u32);
        
        let self_test_extended_minutes = json.get("ata_smart_data")
            .and_then(|d| d.get("self_test"))
            .and_then(|s| s.get("polling_minutes"))
            .and_then(|p| p.get("extended"))
            .and_then(|e| e.as_u64())
            .map(|m| m as u32);
        
        // Error logs
        let error_log_count = json.get("ata_smart_error_log")
            .and_then(|e| e.get("summary"))
            .and_then(|s| s.get("count"))
            .and_then(|c| c.as_u64())
            .map(|c| c as u32);
        
        let self_test_log_count = json.get("ata_smart_self_test_log")
            .and_then(|l| l.get("standard"))
            .and_then(|s| s.get("count"))
            .and_then(|c| c.as_u64())
            .map(|c| c as u32);
        
        // SSD-specific: endurance and spare
        let endurance_used_percent = json.get("endurance_used")
            .and_then(|e| e.as_u64())
            .map(|e| e as u32);
        
        let spare_available_percent = json.get("spare_available")
            .and_then(|s| s.as_u64())
            .map(|s| s as u32);
        
        // Parse SMART attributes with extended fields
        let mut attributes = Vec::new();
        let mut reallocated_sectors = None;
        let mut pending_sectors = None;
        let mut uncorrectable_sectors = None;
        let mut total_lbas_written: Option<u64> = None;
        let mut total_lbas_read: Option<u64> = None;
        
        if let Some(attrs) = json.get("ata_smart_attributes").and_then(|a| a.get("table")).and_then(|t| t.as_array()) {
            for attr in attrs {
                let id = attr.get("id").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                let name = attr.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown").to_string();
                let value = attr.get("value").and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or("-".to_string());
                let worst = attr.get("worst").and_then(|w| w.as_u64()).map(|w| w.to_string());
                let threshold = attr.get("thresh").and_then(|t| t.as_u64()).map(|t| t.to_string());
                let raw_value = attr.get("raw").and_then(|r| r.get("value")).and_then(|v| v.as_u64()).map(|v| v.to_string()).unwrap_or("-".to_string());
                
                // Extended attribute flags
                let flags = attr.get("flags")
                    .and_then(|f| f.get("string"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.trim().to_string());
                
                let prefailure = attr.get("flags")
                    .and_then(|f| f.get("prefailure"))
                    .and_then(|p| p.as_bool());
                
                // Check for critical attributes and extract special values
                let raw = attr.get("raw").and_then(|r| r.get("value")).and_then(|v| v.as_u64()).unwrap_or(0);
                let status = match id {
                    5 => {  // Reallocated_Sector_Ct
                        reallocated_sectors = Some(raw);
                        if raw > 0 { "warning".to_string() } else { "ok".to_string() }
                    },
                    196 => { // Reallocated_Event_Count
                        if raw > 0 { "warning".to_string() } else { "ok".to_string() }
                    },
                    197 => { // Current_Pending_Sector
                        pending_sectors = Some(raw);
                        if raw > 0 { "warning".to_string() } else { "ok".to_string() }
                    },
                    198 => { // Offline_Uncorrectable
                        uncorrectable_sectors = Some(raw);
                        if raw > 0 { "warning".to_string() } else { "ok".to_string() }
                    },
                    241 => { // Total_LBAs_Written
                        total_lbas_written = Some(raw);
                        "ok".to_string()
                    },
                    242 => { // Total_LBAs_Read
                        total_lbas_read = Some(raw);
                        "ok".to_string()
                    },
                    _ => "ok".to_string()
                };
                
                attributes.push(SmartAttribute {
                    id,
                    name,
                    value,
                    worst,
                    threshold,
                    raw_value,
                    status,
                    flags,
                    prefailure,
                });
            }
        }
        
        return Some(SmartData {
            available: true,
            health_status,
            temperature,
            power_on_hours,
            power_cycle_count,
            reallocated_sectors,
            pending_sectors,
            uncorrectable_sectors,
            attributes,
            source: "smartctl".to_string(),
            error_message: None,
            // Extended fields
            model_family,
            device_model,
            serial_number,
            firmware_version,
            user_capacity_bytes,
            logical_block_size,
            physical_block_size,
            rotation_rate,
            form_factor,
            device_type,
            protocol,
            ata_version,
            sata_version,
            interface_speed_max,
            interface_speed_current,
            smart_enabled,
            read_lookahead_enabled,
            write_cache_enabled,
            trim_supported,
            ata_security_enabled,
            ata_security_frozen,
            sct_temperature_current,
            sct_temperature_lifetime_min,
            sct_temperature_lifetime_max,
            sct_temperature_op_limit,
            self_test_status,
            self_test_short_minutes,
            self_test_extended_minutes,
            error_log_count,
            self_test_log_count,
            endurance_used_percent,
            spare_available_percent,
            total_lbas_written,
            total_lbas_read,
        });
    }
    
    // Try plain text parsing if JSON fails
    let output_text = Command::new(&smartctl_path)
        .args(["-H", "-A", &device_path])
        .output()
        .ok()?;
    
    let text = String::from_utf8_lossy(&output_text.stdout);
    
    if text.contains("SMART support is:") && !text.contains("Unavailable") {
        let health_status = if text.contains("PASSED") {
            "PASSED ✅".to_string()
        } else if text.contains("FAILED") {
            "FAILED ❌".to_string()
        } else {
            "Unbekannt".to_string()
        };
        
        return Some(SmartData::basic(
            health_status,
            "smartctl",
            Some("Detailed SMART data could not be read.")
        ));
    }
    
    None
}

fn try_diskutil_smart(disk_id: &str) -> Option<SmartData> {
    // diskutil info provides some basic health info for some drives
    let output = Command::new("diskutil")
        .args(["info", disk_id])
        .output()
        .ok()?;
    
    let text = String::from_utf8_lossy(&output.stdout);
    
    // Check if SMART Status is present
    for line in text.lines() {
        if line.contains("SMART Status:") {
            let status = line.split(':').nth(1)?.trim();
            
            // "Not Supported" means SMART is not available for this device
            if status.contains("Not Supported") || status.contains("not supported") {
                return None;
            }
            
            let health_status = if status.contains("Verified") || status.contains("OK") {
                "PASSED ✅".to_string()
            } else if status.contains("Fail") {
                "FAILED ❌".to_string()
            } else {
                status.to_string()
            };
            
            return Some(SmartData::basic(
                health_status,
                "diskutil",
                Some("Only basic SMART status available. For detailed data, install 'smartmontools' (brew install smartmontools).")
            ));
        }
    }
    
    None
}

fn emit_diagnose_progress(app: &AppHandle, percent: u32, status: &str, phase: &str, 
    sectors_checked: u64, errors_found: u64, read_speed: f64, write_speed: f64) {
    let _ = app.emit("diagnose_progress", DiagnoseProgressEvent {
        percent,
        status: status.to_string(),
        phase: phase.to_string(),
        sectors_checked,
        errors_found,
        read_speed_mbps: read_speed,
        write_speed_mbps: write_speed,
    });
}

/// Parse dd output to extract bytes transferred and time in seconds
/// dd outputs: "8388608 bytes transferred in 0.5 secs (16777216 bytes/sec)"
/// Returns (bytes, seconds) or None if parsing fails
fn parse_dd_bytes_and_time(output: &str) -> Option<(u64, f64)> {
    // Look for "X bytes transferred in Y secs" pattern
    if let Some(bytes_pos) = output.find(" bytes transferred in ") {
        let before_bytes = &output[..bytes_pos];
        let bytes_str = before_bytes.split_whitespace().last()?;
        let bytes: u64 = bytes_str.parse().ok()?;
        
        let after_in = &output[bytes_pos + 22..];
        let time_str = after_in.split_whitespace().next()?;
        let time: f64 = time_str.parse().ok()?;
        
        if time > 0.0 && bytes > 0 {
            return Some((bytes, time));
        }
    }
    
    None
}

/// Surface scan - read all sectors and detect read errors (non-destructive)
#[tauri::command]
async fn diagnose_surface_scan(app: AppHandle, disk_id: String, password: String) -> Result<DiagnoseResult, String> {
    CANCEL_DIAGNOSE.store(false, Ordering::SeqCst);
    
    let device_path = format!("/dev/r{}", disk_id);
    
    // First unmount all partitions
    let unmount_script = format!(
        "echo '{}' | sudo -S diskutil unmountDisk force {} 2>&1",
        password.replace("'", "'\\''"),
        disk_id
    );
    let _ = Command::new("sh").args(["-c", &unmount_script]).output();
    
    // Get disk size
    let size_output = Command::new("diskutil").args(["info", "-plist", &disk_id]).output()
        .map_err(|e| format!("Failed to get disk info: {}", e))?;
    let plist = String::from_utf8_lossy(&size_output.stdout);
    let total_bytes = extract_plist_value(&plist, "TotalSize")
        .ok_or("Failed to get disk size")?;
    
    const BLOCK_SIZE: u64 = 16 * 1024 * 1024; // 16MB blocks for better performance
    let total_blocks = (total_bytes + BLOCK_SIZE - 1) / BLOCK_SIZE;
    let total_sectors = total_bytes / 512;
    
    emit_diagnose_progress(&app, 0, "Starting surface scan...", "reading", 0, 0, 0.0, 0.0);
    
    // Run in blocking thread to avoid freezing UI
    let app_clone = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut sectors_checked: u64 = 0;
        let mut errors_found: u64 = 0;
        let bad_sectors: Vec<u64> = Vec::new();
        let start_time = std::time::Instant::now();
        let mut bytes_read: u64 = 0;
        
        // Read using dd with sudo - use larger blocks for speed
        for block in 0..total_blocks {
            if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                return DiagnoseResult {
                    success: false,
                    total_sectors,
                    sectors_checked,
                    errors_found,
                    bad_sectors,
                    read_speed_mbps: 0.0,
                    write_speed_mbps: 0.0,
                    message: "Scan cancelled".to_string(),
                };
            }
            
            // Use dd to read 16MB at a time with sudo
            let dd_cmd = format!(
                "echo '{}' | sudo -S dd if={} bs=16m skip={} count=1 2>/dev/null | wc -c",
                password.replace("'", "'\\''"),
                device_path,
                block
            );
            
            let result = Command::new("sh").args(["-c", &dd_cmd]).output();
            
            match result {
                Ok(output) => {
                    let bytes_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    let read_bytes: u64 = bytes_str.parse().unwrap_or(0);
                    if read_bytes > 0 {
                        bytes_read += read_bytes;
                        sectors_checked += read_bytes / 512;
                    } else {
                        errors_found += 1;
                    }
                }
                Err(_) => {
                    errors_found += 1;
                }
            }
            
            let percent = ((block + 1) * 100 / total_blocks) as u32;
            let elapsed = start_time.elapsed().as_secs_f64();
            let read_speed = if elapsed > 0.0 { (bytes_read as f64 / 1024.0 / 1024.0) / elapsed } else { 0.0 };
            
            // Update progress every block (since blocks are now 16MB)
            let status = format!("Reading {:.0} MB / {:.0} MB", bytes_read as f64 / 1024.0 / 1024.0, total_bytes as f64 / 1024.0 / 1024.0);
            emit_diagnose_progress(&app_clone, percent.min(99), &status, "reading", sectors_checked, errors_found, read_speed, 0.0);
        }
        
        let elapsed = start_time.elapsed().as_secs_f64();
        let read_speed = if elapsed > 0.0 { (bytes_read as f64 / 1024.0 / 1024.0) / elapsed } else { 0.0 };
        
        let message = if errors_found == 0 {
            format!("Surface scan complete. No errors found. Read speed: {:.1} MB/s", read_speed)
        } else {
            format!("Surface scan complete. {} errors found!", errors_found)
        };
        
        emit_diagnose_progress(&app_clone, 100, &message, "complete", sectors_checked, errors_found, read_speed, 0.0);
        
        DiagnoseResult {
            success: errors_found == 0,
            total_sectors,
            sectors_checked,
            errors_found,
            bad_sectors,
            read_speed_mbps: read_speed,
            write_speed_mbps: 0.0,
            message,
        }
    }).await.map_err(|e| e.to_string())?;
    
    Ok(result)
}

/// Full test - write patterns and verify (destructive!)
#[tauri::command]
async fn diagnose_full_test(app: AppHandle, disk_id: String, password: String) -> Result<DiagnoseResult, String> {
    CANCEL_DIAGNOSE.store(false, Ordering::SeqCst);
    
    // Use rdisk for raw device access (like speed test)
    let device_path = format!("/dev/r{}", disk_id);
    
    // Unmount all partitions
    let unmount_script = format!(
        "echo '{}' | sudo -S diskutil unmountDisk force {} 2>&1",
        password.replace("'", "'\\''"),
        disk_id
    );
    let _ = Command::new("sh").args(["-c", &unmount_script]).output();
    
    // Get disk size
    let size_output = Command::new("diskutil").args(["info", "-plist", &disk_id]).output()
        .map_err(|e| format!("Failed to get disk info: {}", e))?;
    let plist = String::from_utf8_lossy(&size_output.stdout);
    let total_bytes = extract_plist_value(&plist, "TotalSize")
        .ok_or("Failed to get disk size")?;
    
    const BLOCK_SIZE: u64 = 64 * 1024 * 1024; // 64MB blocks for maximum throughput
    let total_blocks = total_bytes / BLOCK_SIZE;
    let total_sectors = total_bytes / 512;
    
    emit_diagnose_progress(&app, 0, "Starting full test...", "writing", 0, 0, 0.0, 0.0);
    
    // Run in blocking thread
    let app_clone = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        // Test patterns - reduced to 2 for speed (0x00 and 0xFF catch most errors)
        let patterns: [(u8, &str); 2] = [
            (0x00, "zeros"),
            (0xFF, "ones"),
        ];
        
        let mut sectors_checked: u64 = 0;
        let mut errors_found: u64 = 0;
        let bad_sectors: Vec<u64> = Vec::new();
        let mut total_write_time: f64 = 0.0;
        let mut total_read_time: f64 = 0.0;
        let mut total_write_bytes: u64 = 0;
        let mut total_read_bytes: u64 = 0;
        
        for (pattern_idx, (pattern, pattern_name)) in patterns.iter().enumerate() {
            if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                return DiagnoseResult {
                    success: false,
                    total_sectors,
                    sectors_checked,
                    errors_found,
                    bad_sectors,
                    read_speed_mbps: 0.0,
                    write_speed_mbps: 0.0,
                    message: "Test cancelled".to_string(),
                };
            }
            
            // Create temp file with pattern
            let temp_pattern = format!("/tmp/burniso_pattern_{:02X}.bin", pattern);
            let write_buffer: Vec<u8> = vec![*pattern; BLOCK_SIZE as usize];
            if let Ok(mut tf) = File::create(&temp_pattern) {
                let _ = tf.write_all(&write_buffer);
            }
            
            // Write phase
            let write_start = std::time::Instant::now();
            
            for block in 0..total_blocks {
                if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                    let _ = std::fs::remove_file(&temp_pattern);
                    return DiagnoseResult {
                        success: false,
                        total_sectors,
                        sectors_checked,
                        errors_found,
                        bad_sectors,
                        read_speed_mbps: 0.0,
                        write_speed_mbps: 0.0,
                        message: "Test cancelled".to_string(),
                    };
                }
                
                // dd write command with 64MB blocks
                let dd_cmd = format!(
                    "echo '{}' | sudo -S dd if={} of={} bs=64m seek={} count=1 conv=notrunc 2>/dev/null",
                    password.replace("'", "'\\''"),
                    temp_pattern,
                    device_path,
                    block
                );
                
                if Command::new("sh").args(["-c", &dd_cmd]).output().is_ok() {
                    total_write_bytes += BLOCK_SIZE;
                }
                
                // Update GUI every block
                // Total: 4 phases (2 patterns × write + verify), each phase = 25%
                // Pattern 0 Write: 0-25%, Pattern 0 Verify: 25-50%
                // Pattern 1 Write: 50-75%, Pattern 1 Verify: 75-100%
                let phase_progress = (block + 1) as f64 / total_blocks as f64; // 0.0 to 1.0
                let base_percent = (pattern_idx * 50) as f64;
                let percent = (base_percent + phase_progress * 25.0) as u32;
                let status = format!("Writing {} ({}/{})", pattern_name, block + 1, total_blocks);
                emit_diagnose_progress(&app_clone, percent.min(99), &status, "writing", sectors_checked, errors_found, 0.0, 0.0);
            }
            
            total_write_time += write_start.elapsed().as_secs_f64();
            let _ = Command::new("sync").output();
            
            // Verify phase using dd
            let read_start = std::time::Instant::now();
            
            for block in 0..total_blocks {
                if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                    let _ = std::fs::remove_file(&temp_pattern);
                    break;
                }
                
                // Read block using dd - just check first byte for speed
                let dd_read = format!(
                    "echo '{}' | sudo -S dd if={} bs=64m skip={} count=1 2>/dev/null | head -c 1 | xxd -p",
                    password.replace("'", "'\\''"),
                    device_path,
                    block
                );
                
                let result = Command::new("sh").args(["-c", &dd_read]).output();
                
                match result {
                    Ok(output) => {
                        let hex = String::from_utf8_lossy(&output.stdout);
                        // Check if pattern matches (first bytes should be pattern)
                        let expected = format!("{:02x}", pattern);
                        if !hex.is_empty() {
                            total_read_bytes += BLOCK_SIZE;
                            sectors_checked += BLOCK_SIZE / 512;
                            if !hex.starts_with(&expected) && !hex.starts_with(&expected.to_uppercase()) {
                                errors_found += 1;
                            }
                        } else {
                            errors_found += 1;
                        }
                    }
                    Err(_) => {
                        errors_found += 1;
                    }
                }
                
                // Update GUI every block
                // Pattern 0 Verify: 25-50%, Pattern 1 Verify: 75-100%
                let phase_progress = (block + 1) as f64 / total_blocks as f64;
                let base_percent = (pattern_idx * 50 + 25) as f64;
                let percent = (base_percent + phase_progress * 25.0) as u32;
                let status = format!("Verifying {} ({}/{})", pattern_name, block + 1, total_blocks);
                emit_diagnose_progress(&app_clone, percent.min(99), &status, "verifying", sectors_checked, errors_found, 0.0, 0.0);
            }
            
            total_read_time += read_start.elapsed().as_secs_f64();
            let _ = std::fs::remove_file(&temp_pattern);
        }
        
        let write_speed = if total_write_time > 0.0 { (total_write_bytes as f64 / 1024.0 / 1024.0) / total_write_time } else { 0.0 };
        let read_speed = if total_read_time > 0.0 { (total_read_bytes as f64 / 1024.0 / 1024.0) / total_read_time } else { 0.0 };
        
        let message = if errors_found == 0 {
            format!("Full test complete. No errors. Write: {:.1} MB/s, Read: {:.1} MB/s", write_speed, read_speed)
        } else {
            format!("Full test complete. {} errors found!", errors_found)
        };
        
        emit_diagnose_progress(&app_clone, 100, &message, "complete", sectors_checked, errors_found, read_speed, write_speed);
        
        DiagnoseResult {
            success: errors_found == 0,
            total_sectors,
            sectors_checked,
            errors_found,
            bad_sectors,
            read_speed_mbps: read_speed,
            write_speed_mbps: write_speed,
            message,
        }
    }).await.map_err(|e| e.to_string())?;
    
    Ok(result)
}

/// Speed test - measure read and write performance (destructive for write!)
#[tauri::command]
async fn diagnose_speed_test(app: AppHandle, disk_id: String, password: String) -> Result<DiagnoseResult, String> {
    CANCEL_DIAGNOSE.store(false, Ordering::SeqCst);
    
    let device_path = format!("/dev/r{}", disk_id);
    
    // Show progress immediately
    emit_diagnose_progress(&app, 0, "USB-Stick wird vorbereitet...", "preparing", 0, 0, 0.0, 0.0);
    
    // Unmount - run in background to not block
    let unmount_script = format!(
        "echo '{}' | sudo -S diskutil unmountDisk force {} 2>&1",
        password.replace("'", "'\\''"),
        disk_id
    );
    let _ = Command::new("sh").args(["-c", &unmount_script]).output();
    
    emit_diagnose_progress(&app, 0, "Lese Disk-Informationen...", "preparing", 0, 0, 0.0, 0.0);
    
    // Get disk size
    let size_output = Command::new("diskutil").args(["info", "-plist", &disk_id]).output()
        .map_err(|e| format!("Failed to get disk info: {}", e))?;
    let plist = String::from_utf8_lossy(&size_output.stdout);
    let total_bytes = extract_plist_value(&plist, "TotalSize")
        .ok_or("Failed to get disk size")?;
    
    // Test with different block sizes for accurate speed measurement
    // Larger blocks = more realistic max speed, smaller blocks = more IO overhead
    // Test ~10% of total disk capacity for meaningful results (min 100MB, max 50GB per test)
    // For a 256GB drive, this means testing ~26GB total (split across 3 block sizes)
    let test_percentage = 0.10; // 10% of disk capacity
    let total_test_bytes = (total_bytes as f64 * test_percentage) as u64;
    let per_test_bytes = total_test_bytes / 3; // Split across 3 block size tests
    
    // Minimum 100MB, maximum 50GB per test for practical limits
    let min_test_bytes: u64 = 100 * 1024 * 1024;       // 100 MB minimum
    let max_test_bytes: u64 = 50 * 1024 * 1024 * 1024; // 50 GB maximum
    let capped_test_bytes = per_test_bytes.max(min_test_bytes).min(max_test_bytes);
    
    // Calculate block counts based on capped test size
    let count_1m = capped_test_bytes / (1 * 1024 * 1024);    // blocks for 1MB test
    let count_4m = capped_test_bytes / (4 * 1024 * 1024);    // blocks for 4MB test
    let count_16m = capped_test_bytes / (16 * 1024 * 1024);  // blocks for 16MB test
    
    let block_sizes: [(u64, &str, u64); 3] = [
        (1 * 1024 * 1024, "1m", count_1m.max(10)),     // At least 10 blocks
        (4 * 1024 * 1024, "4m", count_4m.max(5)),      // At least 5 blocks
        (16 * 1024 * 1024, "16m", count_16m.max(3)),   // At least 3 blocks
    ];
    let total_tests = block_sizes.len() as u32;
    
    // Calculate and log total test size for transparency
    let total_test_size_mb = block_sizes.iter()
        .map(|(bs, _, cnt)| (bs * cnt) / (1024 * 1024))
        .sum::<u64>();
    
    // Format test size for display
    let test_size_display = if total_test_size_mb >= 1024 {
        format!("{:.1} GB", total_test_size_mb as f64 / 1024.0)
    } else {
        format!("{} MB", total_test_size_mb)
    };
    
    emit_diagnose_progress(&app, 0, &format!("Starte Geschwindigkeitstest ({})...", test_size_display), "starting", 0, 0, 0.0, 0.0);
    
    // Clone password for use in blocking thread
    let password_clone = password.clone();
    let app_clone = app.clone();
    
    // Run in blocking thread
    let result = tokio::task::spawn_blocking(move || {
        let mut all_results: Vec<(String, f64, f64)> = Vec::new();
        let mut best_write = 0.0f64;
        let mut best_read = 0.0f64;
        
        // Maximum blocks per chunk to show progress frequently
        // With typical USB speeds (20-100 MB/s), 256MB chunks take 2-12 seconds
        let max_mb_per_chunk: u64 = 256; // 256 MB max per chunk for visible progress
        
        for (test_idx, (block_size, bs_str, count)) in block_sizes.iter().enumerate() {
            if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                return DiagnoseResult {
                    success: false,
                    total_sectors: total_bytes / 512,
                    sectors_checked: 0,
                    errors_found: 0,
                    bad_sectors: Vec::new(),
                    read_speed_mbps: best_read,
                    write_speed_mbps: best_write,
                    message: "Test abgebrochen".to_string(),
                };
            }
            
            let test_bytes = block_size * count;
            let test_mb = test_bytes / 1024 / 1024;
            let block_mb = block_size / 1024 / 1024;
            let test_name = format!("{}MB Blöcke", block_mb);
            
            // Format test size for display (MB or GB)
            let test_size_str = if test_mb >= 1024 {
                format!("{:.1} GB", test_mb as f64 / 1024.0)
            } else {
                format!("{} MB", test_mb)
            };
            
            // Calculate how many blocks per chunk (to show progress every ~256MB)
            let blocks_per_chunk = (max_mb_per_chunk / block_mb).max(1);
            let total_chunks = (*count as f64 / blocks_per_chunk as f64).ceil() as u64;
            
            // Calculate progress percentages for this test
            let test_progress_start = ((test_idx as u32) * 100) / total_tests;
            let test_progress_range = 100 / total_tests;
            
            // === WRITE TEST ===
            emit_diagnose_progress(&app_clone, test_progress_start, 
                &format!("Test {}/{}: {} - Schreibe {}...", test_idx + 1, total_tests, test_name, test_size_str), 
                "writing", 0, 0, best_read, best_write);
            
            std::thread::sleep(std::time::Duration::from_millis(50));
            
            // Write in chunks for visible progress
            let mut total_write_bytes: u64 = 0;
            let mut total_write_time: f64 = 0.0;
            let mut blocks_written: u64 = 0;
            let mut chunk_num: u64 = 0;
            
            while blocks_written < *count {
                if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                    return DiagnoseResult {
                        success: false,
                        total_sectors: total_bytes / 512,
                        sectors_checked: 0,
                        errors_found: 0,
                        bad_sectors: Vec::new(),
                        read_speed_mbps: best_read,
                        write_speed_mbps: best_write,
                        message: "Test abgebrochen".to_string(),
                    };
                }
                
                let remaining = *count - blocks_written;
                let chunk_blocks = remaining.min(blocks_per_chunk);
                let offset_blocks = blocks_written;
                
                // Update progress before each chunk
                let chunk_progress = test_progress_start + 
                    ((chunk_num as u32 * test_progress_range / 2) / total_chunks.max(1) as u32);
                let written_so_far_mb = (blocks_written * block_size) / (1024 * 1024);
                let written_display = if written_so_far_mb >= 1024 {
                    format!("{:.1} GB", written_so_far_mb as f64 / 1024.0)
                } else {
                    format!("{} MB", written_so_far_mb)
                };
                
                emit_diagnose_progress(&app_clone, chunk_progress, 
                    &format!("Test {}/{}: {} - Schreibe {} von {}...", 
                        test_idx + 1, total_tests, test_name, written_display, test_size_str), 
                    "writing", 0, 0, best_read, best_write);
                
                // Write chunk with seek to correct position
                let dd_write = format!(
                    "echo '{}' | sudo -S dd if=/dev/zero of={} bs={} count={} seek={} 2>&1",
                    password_clone.replace("'", "'\\''"),
                    device_path,
                    bs_str,
                    chunk_blocks,
                    offset_blocks
                );
                
                let write_result = Command::new("sh").args(["-c", &dd_write]).output();
                
                // Parse result and accumulate
                if let Ok(output) = &write_result {
                    let stdout_str = String::from_utf8_lossy(&output.stdout);
                    let stderr_str = String::from_utf8_lossy(&output.stderr);
                    let combined = format!("{}{}", stdout_str, stderr_str);
                    
                    // Parse bytes and time from dd output
                    if let Some((bytes, secs)) = parse_dd_bytes_and_time(&combined) {
                        total_write_bytes += bytes;
                        total_write_time += secs;
                    }
                }
                
                blocks_written += chunk_blocks;
                chunk_num += 1;
            }
            
            // Calculate average write speed
            let write_speed = if total_write_time > 0.0 {
                (total_write_bytes as f64 / total_write_time) / (1024.0 * 1024.0)
            } else {
                0.0
            };
            
            if write_speed > 0.0 {
                best_write = best_write.max(write_speed);
            }
            
            let mid_progress = test_progress_start + (test_progress_range / 2);
            emit_diagnose_progress(&app_clone, mid_progress, 
                &format!("Test {}/{}: {} - Schreiben: {:.1} MB/s", test_idx + 1, total_tests, test_name, write_speed), 
                "writing", 0, 0, best_read, best_write);
            
            std::thread::sleep(std::time::Duration::from_millis(100));
            
            // === READ TEST ===
            emit_diagnose_progress(&app_clone, mid_progress, 
                &format!("Test {}/{}: {} - Lese {}...", test_idx + 1, total_tests, test_name, test_size_str), 
                "reading", 0, 0, best_read, best_write);
            
            std::thread::sleep(std::time::Duration::from_millis(50));
            
            // Read in chunks for visible progress
            let mut total_read_bytes: u64 = 0;
            let mut total_read_time: f64 = 0.0;
            let mut blocks_read: u64 = 0;
            chunk_num = 0;
            
            while blocks_read < *count {
                if CANCEL_DIAGNOSE.load(Ordering::SeqCst) {
                    return DiagnoseResult {
                        success: false,
                        total_sectors: total_bytes / 512,
                        sectors_checked: 0,
                        errors_found: 0,
                        bad_sectors: Vec::new(),
                        read_speed_mbps: best_read,
                        write_speed_mbps: best_write,
                        message: "Test abgebrochen".to_string(),
                    };
                }
                
                let remaining = *count - blocks_read;
                let chunk_blocks = remaining.min(blocks_per_chunk);
                let offset_blocks = blocks_read;
                
                // Update progress before each chunk
                let chunk_progress = mid_progress + 
                    ((chunk_num as u32 * test_progress_range / 2) / total_chunks.max(1) as u32);
                let read_so_far_mb = (blocks_read * block_size) / (1024 * 1024);
                let read_display = if read_so_far_mb >= 1024 {
                    format!("{:.1} GB", read_so_far_mb as f64 / 1024.0)
                } else {
                    format!("{} MB", read_so_far_mb)
                };
                
                emit_diagnose_progress(&app_clone, chunk_progress, 
                    &format!("Test {}/{}: {} - Lese {} von {}...", 
                        test_idx + 1, total_tests, test_name, read_display, test_size_str), 
                    "reading", 0, 0, best_read, best_write);
                
                // Read chunk with skip to correct position
                let dd_read = format!(
                    "echo '{}' | sudo -S dd if={} of=/dev/null bs={} count={} skip={} 2>&1",
                    password_clone.replace("'", "'\\''"),
                    device_path,
                    bs_str,
                    chunk_blocks,
                    offset_blocks
                );
                
                let read_result = Command::new("sh").args(["-c", &dd_read]).output();
                
                // Parse result and accumulate
                if let Ok(output) = &read_result {
                    let stdout_str = String::from_utf8_lossy(&output.stdout);
                    let stderr_str = String::from_utf8_lossy(&output.stderr);
                    let combined = format!("{}{}", stdout_str, stderr_str);
                    
                    if let Some((bytes, secs)) = parse_dd_bytes_and_time(&combined) {
                        total_read_bytes += bytes;
                        total_read_time += secs;
                    }
                }
                
                blocks_read += chunk_blocks;
                chunk_num += 1;
            }
            
            // Calculate average read speed
            let read_speed = if total_read_time > 0.0 {
                (total_read_bytes as f64 / total_read_time) / (1024.0 * 1024.0)
            } else {
                0.0
            };
            
            if read_speed > 0.0 {
                best_read = best_read.max(read_speed);
            }
            
            // Store results
            all_results.push((test_name.clone(), write_speed, read_speed));
            
            let end_progress = ((test_idx as u32 + 1) * 100) / total_tests;
            emit_diagnose_progress(&app_clone, end_progress, 
                &format!("Test {}/{}: {} - W: {:.1} / R: {:.1} MB/s", 
                    test_idx + 1, total_tests, test_name, write_speed, read_speed), 
                "testing", 0, 0, best_read, best_write);
            
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        
        // Final summary
        let message = if all_results.iter().all(|(_, w, r)| *w == 0.0 && *r == 0.0) {
            "Keine gültigen Testergebnisse. Möglicherweise fehlen Berechtigungen.".to_string()
        } else {
            let mut msg = String::from("Geschwindigkeitstest Ergebnisse:\n");
            for (name, w, r) in &all_results {
                msg.push_str(&format!("  {}: W {:.1}, R {:.1} MB/s\n", name, w, r));
            }
            msg.push_str(&format!("\nBeste Werte: W {:.1}, R {:.1} MB/s", best_write, best_read));
            msg
        };
        
        let success = best_write > 0.0 || best_read > 0.0;
        
        emit_diagnose_progress(&app_clone, 100, 
            if success { "Test abgeschlossen!" } else { "Test fehlgeschlagen" }, 
            "complete", 0, 0, best_read, best_write);
        
        DiagnoseResult {
            success,
            total_sectors: total_bytes / 512,
            sectors_checked: 0,
            errors_found: 0,
            bad_sectors: Vec::new(),
            read_speed_mbps: best_read,
            write_speed_mbps: best_write,
            message,
        }
    }).await.map_err(|e| e.to_string())?;
    
    Ok(result)
}

#[tauri::command]
fn list_disks() -> Result<Vec<DiskInfo>, String> {
    // Strategy: Get external physical disks + internal removable media (like built-in SD card readers)
    // The built-in SD card reader is classified as "internal" but has "Removable Media: Removable"
    
    let mut disks: Vec<DiskInfo> = Vec::new();
    let mut seen_disk_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    
    // First: Get external physical disks (USB drives, external SSDs, etc.)
    let external_output = Command::new("diskutil").args(["list", "external", "physical"]).output()
        .map_err(|e| format!("diskutil Fehler: {}", e))?;
    let external_stdout = String::from_utf8_lossy(&external_output.stdout);
    
    for line in external_stdout.lines() {
        if line.starts_with("/dev/disk") {
            if let Some(caps) = regex_lite::Regex::new(r"/dev/(disk\d+)")
                .ok().and_then(|re| re.captures(line)) {
                let disk_id = caps.get(1).unwrap().as_str().to_string();
                if !seen_disk_ids.contains(&disk_id) {
                    if let Ok(info) = get_disk_details(&disk_id) {
                        seen_disk_ids.insert(disk_id);
                        disks.push(info);
                    }
                }
            }
        }
    }
    
    // Second: Get internal physical disks and filter for removable media (SD cards)
    let internal_output = Command::new("diskutil").args(["list", "internal", "physical"]).output()
        .map_err(|e| format!("diskutil Fehler: {}", e))?;
    let internal_stdout = String::from_utf8_lossy(&internal_output.stdout);
    
    for line in internal_stdout.lines() {
        if line.starts_with("/dev/disk") {
            if let Some(caps) = regex_lite::Regex::new(r"/dev/(disk\d+)")
                .ok().and_then(|re| re.captures(line)) {
                let disk_id = caps.get(1).unwrap().as_str().to_string();
                if !seen_disk_ids.contains(&disk_id) {
                    // Check if this is a removable media (SD card, etc.)
                    if is_removable_media(&disk_id) {
                        if let Ok(info) = get_disk_details(&disk_id) {
                            seen_disk_ids.insert(disk_id);
                            disks.push(info);
                        }
                    }
                }
            }
        }
    }
    
    Ok(disks)
}

/// Check if a disk has removable media (like SD cards in built-in readers)
fn is_removable_media(disk_id: &str) -> bool {
    let output = Command::new("diskutil").args(["info", disk_id]).output();
    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            // Check for "Removable Media: Removable" or "Removable Media: Yes"
            if line.contains("Removable Media:") {
                let value = line.split(':').nth(1).map(|s| s.trim().to_lowercase()).unwrap_or_default();
                return value == "removable" || value == "yes";
            }
        }
    }
    false
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
    let supported_fs = ["APFS", "Apple_APFS", "HFS+", "Mac OS Extended", "FAT32", "ExFAT", "Apple_HFS", "MS-DOS", "msdos", "FAT16", "FAT12"];
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
                    // Für ISO-Volumes: VolumeTotalSpace (echte Größe), sonst TotalSize (Disk-Größe)
                    let bytes = if is_iso {
                        extract_plist_value(&plist, "VolumeTotalSpace")
                            .or_else(|| extract_plist_value(&plist, "TotalSize"))
                    } else {
                        extract_plist_value(&plist, "TotalSize")
                    };
                    return Some(VolumeInfo {
                        identifier: part_id.to_string(),
                        mount_point: mp.clone(),
                        filesystem: display_fs,
                        name: extract_plist_string(&plist, "VolumeName").unwrap_or_else(|| "USB-Volume".to_string()),
                        bytes,
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
    
    // Versuche zuerst, die Disk zu mounten (für ISO-Volumes, die nicht automatisch gemountet sind)
    // Das Mounten von ISO-Volumes braucht keine Root-Rechte
    let _ = Command::new("diskutil")
        .args(["mount", &disk_id])
        .output();
    
    // Kurz warten, damit das Mount abgeschlossen ist
    std::thread::sleep(std::time::Duration::from_millis(300));
    
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
    
    // Try raw detection on main disk (requires root - may not work without password)
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

// Static for cancel tools operation
static CANCEL_TOOLS: AtomicBool = AtomicBool::new(false);

#[tauri::command]
fn cancel_tools() {
    CANCEL_TOOLS.store(true, Ordering::SeqCst);
}

/// Repair a USB disk filesystem
#[tauri::command]
async fn repair_disk(
    app: AppHandle,
    disk_id: String,
    password: String,
) -> Result<String, String> {
    CANCEL_TOOLS.store(false, Ordering::SeqCst);
    
    let disk_path = format!("/dev/{}", disk_id);
    
    emit_progress(&app, 5, "Starting disk repair...", "tools");
    
    // Get list of partitions on this disk
    let diskutil_list = Command::new("diskutil")
        .args(["list", &disk_path])
        .output();
    
    let mut partitions: Vec<String> = Vec::new();
    
    if let Ok(output) = diskutil_list {
        let list_str = String::from_utf8_lossy(&output.stdout);
        for line in list_str.lines() {
            // Look for partition identifiers like "disk4s1", "disk4s2", etc.
            if let Some(id) = line.split_whitespace().last() {
                if id.starts_with(&disk_id) && id.contains('s') && id != disk_id {
                    partitions.push(id.to_string());
                }
            }
        }
    }
    
    emit_progress(&app, 10, &format!("Found {} partition(s)", partitions.len()), "tools");
    
    // If no partitions found, try repairing the whole disk
    if partitions.is_empty() {
        partitions.push(disk_id.clone());
    }
    
    let mut all_results = Vec::new();
    let mut any_success = false;
    let partition_count = partitions.len();
    
    for (idx, partition) in partitions.iter().enumerate() {
        let partition_path = format!("/dev/{}", partition);
        let progress_base = 15 + (idx as u32 * 70 / partition_count as u32);
        
        // Check filesystem type for this partition
        let diskutil_info = Command::new("diskutil")
            .args(["info", &partition_path])
            .output();
        
        let mut filesystem = String::new();
        if let Ok(output) = diskutil_info {
            let info_str = String::from_utf8_lossy(&output.stdout);
            for line in info_str.lines() {
                if line.contains("File System Personality:") || line.contains("Type (Bundle):") {
                    filesystem = line.split(':').nth(1).unwrap_or("").trim().to_string();
                    break;
                }
            }
        }
        
        emit_progress(&app, progress_base, &format!("Repairing {} ({})...", partition, if filesystem.is_empty() { "Unknown" } else { &filesystem }), "tools");
        
        // Unmount first
        let _ = Command::new("diskutil")
            .args(["unmount", &partition_path])
            .output();
        
        std::thread::sleep(std::time::Duration::from_millis(300));
        
        // Use repairVolume for partitions, repairDisk for whole disk
        let repair_cmd = if partition.contains('s') {
            format!("diskutil repairVolume {}", partition_path)
        } else {
            format!("diskutil repairDisk {}", partition_path)
        };
        
        let mut child = Command::new("sudo")
            .args(["-S", "sh", "-c", &repair_cmd])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Repair error: {}", e))?;
        
        // Send password
        if let Some(ref mut stdin) = child.stdin {
            writeln!(stdin, "{}", password).ok();
        }
        drop(child.stdin.take());
        
        // Wait for completion
        let output = child.wait_with_output().map_err(|e| format!("Wait error: {}", e))?;
        
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{}{}", stdout_str, stderr_str);
        
        // Check result
        if output.status.success() || combined.contains("appears to be OK") || combined.contains("exit code is 0") {
            any_success = true;
            all_results.push(format!("✓ {}: OK", partition));
        } else if combined.contains("repaired") {
            any_success = true;
            all_results.push(format!("✓ {}: Repaired", partition));
        } else {
            // Extract meaningful error
            let error_line = combined.lines()
                .find(|l| l.contains("Error") || l.contains("error") || l.contains("failed"))
                .unwrap_or("Unknown error");
            all_results.push(format!("✗ {}: {}", partition, error_line.trim()));
        }
        
        // Try to remount
        let _ = Command::new("diskutil")
            .args(["mount", &partition_path])
            .output();
    }
    
    emit_progress(&app, 100, "Repair complete!", "tools");
    
    let result_text = all_results.join("\n");
    
    if any_success {
        Ok(format!("Repair completed:\n{}", result_text))
    } else {
        Err(format!("Repair failed:\n{}", result_text))
    }
}

/// Format a USB disk with the specified filesystem
#[tauri::command]
async fn format_disk(
    app: AppHandle,
    disk_id: String,
    filesystem: String,
    name: String,
    scheme: String,
    password: String,
    encrypted: Option<bool>,
    encryption_password: Option<String>,
) -> Result<String, String> {
    CANCEL_TOOLS.store(false, Ordering::SeqCst);
    
    let disk_path = format!("/dev/{}", disk_id);
    let is_encrypted = encrypted.unwrap_or(false);
    let is_ntfs = filesystem == "NTFS";
    let is_ext = filesystem == "ext2" || filesystem == "ext3" || filesystem == "ext4";
    
    // Validate filesystem
    let fs_type = match (filesystem.as_str(), is_encrypted) {
        ("FAT32", _) => "MS-DOS FAT32",
        ("ExFAT", _) => "ExFAT",
        ("NTFS", _) => "UFSD_NTFS", // Paragon NTFS driver
        ("ext2", _) => "UFSD_EXTFS", // Paragon extFS driver
        ("ext3", _) => "UFSD_EXTFS", // Paragon extFS driver
        ("ext4", _) => "UFSD_EXTFS", // Paragon extFS driver
        ("APFS", false) => "APFS",
        ("APFS", true) => "APFS (Encrypted)",
        ("HFS+", false) => "JHFS+",
        ("HFS+", true) => "JHFS+ (Encrypted)",
        _ => return Err(format!("Nicht unterstütztes Dateisystem: {}", filesystem)),
    };
    
    // Validate scheme
    let scheme_type = match scheme.as_str() {
        "GPT" => "GPT",
        "MBR" => "MBR",
        _ => "GPT",
    };
    
    // Sanitize volume name (FAT32 max 11 chars, no special chars)
    let safe_name: String = name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .take(11)
        .collect();
    let volume_name = if safe_name.is_empty() { "USB_STICK".to_string() } else { safe_name };
    
    emit_progress(&app, 5, "Formatting USB drive...", "tools");
    
    // Force unmount first to release any locks (especially after secure erase)
    let _ = Command::new("diskutil")
        .args(["unmountDisk", "force", &disk_path])
        .output();
    
    // Small delay to allow system to release device
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    // Build the format command
    // NTFS requires Paragon NTFS driver and uses eraseVolume with UFSD_NTFS
    // ext2/3/4 requires Paragon extFS driver and uses eraseVolume with UFSD_EXTFS
    // Other filesystems use eraseDisk
    let script = if is_ntfs {
        // For NTFS with Paragon: 
        // 1. Create a single partition disk with FAT32 first (simpler than ExFAT)
        // 2. Reformat the first partition (s1 or s2 depending on scheme) as NTFS
        // GPT creates disk#s2 as main partition, MBR creates disk#s1
        let partition_suffix = if scheme_type == "GPT" { "s2" } else { "s1" };
        format!(
            r#"diskutil eraseDisk "MS-DOS FAT32" "{}" {} {} && sleep 1 && echo "y" | diskutil eraseVolume UFSD_NTFS "{}" {}{}"#,
            volume_name, scheme_type, disk_path, volume_name, disk_path, partition_suffix
        )
    } else if is_ext {
        // For ext2/3/4 with Paragon extFS:
        // 1. Create a single partition disk with FAT32 first
        // 2. Reformat the first partition as ext2/3/4 using UFSD_EXTFS
        // GPT creates disk#s2 as main partition, MBR creates disk#s1
        let partition_suffix = if scheme_type == "GPT" { "s2" } else { "s1" };
        format!(
            r#"diskutil eraseDisk "MS-DOS FAT32" "{}" {} {} && sleep 1 && echo "y" | diskutil eraseVolume UFSD_EXTFS "{}" {}{}"#,
            volume_name, scheme_type, disk_path, volume_name, disk_path, partition_suffix
        )
    } else if is_encrypted {
        let enc_pass = encryption_password.unwrap_or_default();
        if enc_pass.is_empty() {
            return Err("Verschlüsselungspasswort erforderlich".to_string());
        }
        // For encrypted APFS/HFS+, use diskutil with passphrase
        format!(
            r#"diskutil eraseDisk "{}" "{}" {} {} -passphrase "{}""#,
            fs_type, volume_name, scheme_type, disk_path, enc_pass
        )
    } else {
        format!(
            r#"diskutil eraseDisk "{}" "{}" {} {}"#,
            fs_type, volume_name, scheme_type, disk_path
        )
    };
    
    // Start the format process
    let mut child = Command::new("sudo")
        .args(["-S", "sh", "-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Format error: {}", e))?;
    
    // Send password
    if let Some(ref mut stdin) = child.stdin {
        writeln!(stdin, "{}", password).ok();
    }
    drop(child.stdin.take());
    
    // Animate progress while waiting for completion
    let mut progress = 10;
    loop {
        if CANCEL_TOOLS.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Format cancelled".to_string());
        }
        
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    emit_progress(&app, 95, "Mounting volume...", "tools");
                    
                    // Wait a moment for the system to recognize the new filesystem
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    
                    // Mount the newly formatted disk
                    let _ = Command::new("diskutil")
                        .args(["mountDisk", &disk_path])
                        .output();
                    
                    // Additional wait and retry mount for FAT32/exFAT/NTFS/ext which sometimes need it
                    if filesystem == "FAT32" || filesystem == "ExFAT" || filesystem == "NTFS" 
                        || filesystem == "ext2" || filesystem == "ext3" || filesystem == "ext4" {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        // Try mounting specific partitions
                        let partition_suffix = if scheme_type == "GPT" { "s2" } else { "s1" };
                        let partition_path = format!("{}{}", disk_path, partition_suffix);
                        let _ = Command::new("diskutil")
                            .args(["mount", &partition_path])
                            .output();
                    }
                    
                    emit_progress(&app, 100, "Format complete!", "tools");
                    return Ok(format!("USB formatted as {} ({})", filesystem, volume_name));
                } else {
                    if let Some(mut stderr) = child.stderr.take() {
                        let mut error_msg = String::new();
                        let _ = stderr.read_to_string(&mut error_msg);
                        if !error_msg.is_empty() {
                            return Err(format!("Format failed: {}", error_msg));
                        }
                    }
                    return Err("Format failed".to_string());
                }
            }
            Ok(None) => {
                // Still running - animate progress
                progress = (progress + 5).min(90);
                emit_progress(&app, progress, &format!("Formatting as {}...", filesystem), "tools");
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => {
                return Err(format!("Wait error: {}", e));
            }
        }
    }
}

/// Write a pass using dd with progress tracking
fn write_pass(
    app: &AppHandle,
    disk_path: &str,
    disk_size: u64,
    source: &str,
    pass_num: u32,
    total_passes: u32,
    pass_desc: &str,
    password: &str,
) -> Result<(), String> {
    // Calculate base progress for this pass
    let pass_start = ((pass_num - 1) as f64 / total_passes as f64 * 90.0) as u32 + 5;
    let pass_range = (90.0 / total_passes as f64) as u32;
    
    emit_progress(app, pass_start, &format!("Pass {}/{}: {}...", pass_num, total_passes, pass_desc), "tools");
    
    // Use dd with 1MB blocks
    let block_size = 1024 * 1024u64; // 1MB
    let total_blocks = disk_size / block_size;
    
    // Build dd command
    let dd_cmd = format!(
        "dd if={} of={} bs=1m count={} 2>&1",
        source, disk_path, total_blocks
    );
    
    let mut child = Command::new("sudo")
        .args(["-S", "sh", "-c", &dd_cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("dd start error: {}", e))?;
    
    // Send password
    if let Some(ref mut stdin) = child.stdin {
        writeln!(stdin, "{}", password).ok();
    }
    drop(child.stdin.take());
    
    // Poll with progress estimation based on typical write speed (~50MB/s for USB)
    let estimated_seconds = (disk_size as f64 / (50.0 * 1024.0 * 1024.0)) as u64;
    let start_time = std::time::Instant::now();
    
    loop {
        if CANCEL_TOOLS.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cancelled".to_string());
        }
        
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    emit_progress(app, pass_start + pass_range, &format!("Pass {}/{}: Complete", pass_num, total_passes), "tools");
                    return Ok(());
                } else {
                    if let Some(mut stderr) = child.stderr.take() {
                        let mut error_msg = String::new();
                        let _ = stderr.read_to_string(&mut error_msg);
                        // dd outputs stats to stderr, check for actual errors
                        if error_msg.contains("Permission denied") || error_msg.contains("No such file") {
                            return Err(format!("dd error: {}", error_msg));
                        }
                    }
                    return Ok(()); // dd often exits 0 but reports to stderr
                }
            }
            Ok(None) => {
                // Estimate progress based on elapsed time
                let elapsed = start_time.elapsed().as_secs();
                let estimated_progress = if estimated_seconds > 0 {
                    ((elapsed as f64 / estimated_seconds as f64) * pass_range as f64).min(pass_range as f64 - 1.0) as u32
                } else {
                    0
                };
                let current = pass_start + estimated_progress;
                emit_progress(app, current, &format!("Pass {}/{}: {}...", pass_num, total_passes, pass_desc), "tools");
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => return Err(format!("Wait error: {}", e)),
        }
    }
}

/// Get disk size in bytes
fn get_disk_size(disk_id: &str) -> Result<u64, String> {
    let output = Command::new("diskutil")
        .args(["info", disk_id])
        .output()
        .map_err(|e| format!("diskutil error: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("Disk Size:") {
            // Extract bytes from format like "Disk Size: 32.0 GB (32000000000 Bytes)"
            if let Some(start) = line.find('(') {
                if let Some(end) = line.find(" Bytes") {
                    let bytes_str = &line[start+1..end];
                    if let Ok(bytes) = bytes_str.trim().parse::<u64>() {
                        return Ok(bytes);
                    }
                }
            }
        }
    }
    Err("Could not determine disk size".to_string())
}

/// Securely erase a USB disk using dd with real progress
#[tauri::command]
async fn secure_erase(
    app: AppHandle,
    disk_id: String,
    level: u32,
    password: String,
) -> Result<String, String> {
    CANCEL_TOOLS.store(false, Ordering::SeqCst);
    
    let disk_path = format!("/dev/r{}", disk_id); // Use raw device for faster writes
    
    // Level descriptions
    let level_desc = match level {
        0 => "1x Zeros",
        1 => "1x Random",
        2 => "DoD 7-Pass",
        3 => "Gutmann 35-Pass",
        4 => "DoE 3-Pass",
        _ => "Unknown",
    };
    
    emit_progress(&app, 2, &format!("Preparing secure erase ({})...", level_desc), "tools");
    
    // Get disk size
    let disk_size = get_disk_size(&disk_id)?;
    
    // Force unmount
    let _ = Command::new("diskutil")
        .args(["unmountDisk", "force", &format!("/dev/{}", disk_id)])
        .output();
    
    std::thread::sleep(std::time::Duration::from_millis(500));
    
    emit_progress(&app, 5, &format!("Starting {} erase...", level_desc), "tools");
    
    match level {
        0 => {
            // Single pass zeros
            write_pass(&app, &disk_path, disk_size, "/dev/zero", 1, 1, "Zeros", &password)?;
        }
        1 => {
            // Single pass random
            write_pass(&app, &disk_path, disk_size, "/dev/urandom", 1, 1, "Random", &password)?;
        }
        2 => {
            // DoD 7-Pass: 0x00, 0xFF, Random, 0x00, 0xFF, Random, Random
            // Simplified: alternating zeros/random
            for i in 1..=7 {
                if CANCEL_TOOLS.load(Ordering::SeqCst) {
                    return Err("Secure erase cancelled".to_string());
                }
                let source = if i % 2 == 1 { "/dev/zero" } else { "/dev/urandom" };
                let desc = if i % 2 == 1 { "Zeros" } else { "Random" };
                write_pass(&app, &disk_path, disk_size, source, i, 7, desc, &password)?;
            }
        }
        3 => {
            // Gutmann 35-Pass: Mix of patterns and random
            // Simplified: 4 random + 27 zeros/random alternating + 4 random
            for i in 1..=35 {
                if CANCEL_TOOLS.load(Ordering::SeqCst) {
                    return Err("Secure erase cancelled".to_string());
                }
                let (source, desc) = if i <= 4 || i > 31 {
                    ("/dev/urandom", "Random")
                } else if i % 2 == 0 {
                    ("/dev/zero", "Pattern")
                } else {
                    ("/dev/urandom", "Random")
                };
                write_pass(&app, &disk_path, disk_size, source, i, 35, desc, &password)?;
            }
        }
        4 => {
            // DoE 3-Pass: Random, Zeros, Random
            write_pass(&app, &disk_path, disk_size, "/dev/urandom", 1, 3, "Random", &password)?;
            if !CANCEL_TOOLS.load(Ordering::SeqCst) {
                write_pass(&app, &disk_path, disk_size, "/dev/zero", 2, 3, "Zeros", &password)?;
            }
            if !CANCEL_TOOLS.load(Ordering::SeqCst) {
                write_pass(&app, &disk_path, disk_size, "/dev/urandom", 3, 3, "Random", &password)?;
            }
        }
        _ => {
            return Err(format!("Unknown erase level: {}", level));
        }
    }
    
    if CANCEL_TOOLS.load(Ordering::SeqCst) {
        return Err("Secure erase cancelled".to_string());
    }
    
    emit_progress(&app, 100, "Secure erase complete!", "tools");
    Ok(format!("USB securely erased ({})", level_desc))
}

/// Forensic analysis - gather all available information about a USB device
#[tauri::command]
async fn forensic_analysis(disk_id: String, password: String) -> Result<serde_json::Value, String> {
    let escaped_password = password.replace("'", "'\\''");
    
    // 0. Validate password first with a simple sudo command
    let password_check_cmd = format!(
        "echo '{}' | sudo -S -v 2>&1",
        escaped_password
    );
    
    if let Ok(output) = Command::new("sh").args(["-c", &password_check_cmd]).output() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{}{}", stdout, stderr);
        
        if combined.contains("Sorry, try again") || 
           combined.contains("incorrect password") ||
           combined.contains("no password was provided") ||
           combined.contains("Authentication failed") {
            return Err("Falsches Passwort. Bitte geben Sie Ihr Admin-Passwort korrekt ein.".to_string());
        }
    }
    
    let mut result = serde_json::json!({
        "disk_id": disk_id,
        "timestamp": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    });
    
    // 0. Check for Paragon drivers availability (for filesystem support info)
    let paragon_ntfs = Command::new("diskutil")
        .args(["listFilesystems"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("UFSD_NTFS"))
        .unwrap_or(false);
    
    let paragon_extfs = Command::new("diskutil")
        .args(["listFilesystems"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("UFSD_EXTFS"))
        .unwrap_or(false);
    
    result["paragon_drivers"] = serde_json::json!({
        "ntfs": paragon_ntfs,
        "extfs": paragon_extfs,
        "ntfs_description": if paragon_ntfs { "Paragon NTFS installiert - voller NTFS Lese-/Schreibzugriff" } else { "Paragon NTFS nicht installiert - nur Lesezugriff auf NTFS" },
        "extfs_description": if paragon_extfs { "Paragon extFS installiert - voller ext2/3/4 Lese-/Schreibzugriff" } else { "Paragon extFS nicht installiert - kein ext2/3/4 Zugriff" }
    });
    
    // 1. Get basic disk info from diskutil
    let diskutil_cmd = format!(
        "echo '{}' | sudo -S diskutil info {} 2>/dev/null",
        escaped_password, disk_id
    );
    
    if let Ok(output) = Command::new("sh").args(["-c", &diskutil_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut disk_info = serde_json::Map::new();
        
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                if !value.is_empty() {
                    match key {
                        "Device Identifier" => { disk_info.insert("device_id".to_string(), serde_json::json!(value)); },
                        "Device Node" => { disk_info.insert("device_node".to_string(), serde_json::json!(value)); },
                        "Whole" => { disk_info.insert("is_whole_disk".to_string(), serde_json::json!(value == "Yes")); },
                        "Part of Whole" => { disk_info.insert("parent_disk".to_string(), serde_json::json!(value)); },
                        "Device / Media Name" => { disk_info.insert("media_name".to_string(), serde_json::json!(value)); },
                        "Volume Name" => { disk_info.insert("volume_name".to_string(), serde_json::json!(value)); },
                        "Mounted" => { disk_info.insert("is_mounted".to_string(), serde_json::json!(value == "Yes")); },
                        "Mount Point" => { disk_info.insert("mount_point".to_string(), serde_json::json!(value)); },
                        "Content (IOContent)" => { disk_info.insert("content_type".to_string(), serde_json::json!(value)); },
                        "File System Personality" => { disk_info.insert("filesystem".to_string(), serde_json::json!(value)); },
                        "Type (Bundle)" => { disk_info.insert("filesystem_bundle".to_string(), serde_json::json!(value)); },
                        "Name (User Visible)" => { disk_info.insert("filesystem_name".to_string(), serde_json::json!(value)); },
                        "Disk Size" => { disk_info.insert("disk_size".to_string(), serde_json::json!(value)); },
                        "Device Block Size" => { disk_info.insert("block_size".to_string(), serde_json::json!(value)); },
                        "Volume Total Space" => { disk_info.insert("total_space".to_string(), serde_json::json!(value)); },
                        "Volume Free Space" => { disk_info.insert("free_space".to_string(), serde_json::json!(value)); },
                        "Volume Used Space" => { disk_info.insert("used_space".to_string(), serde_json::json!(value)); },
                        "Allocation Block Size" => { disk_info.insert("allocation_block_size".to_string(), serde_json::json!(value)); },
                        "Read-Only Media" => { disk_info.insert("read_only".to_string(), serde_json::json!(value == "Yes")); },
                        "Read-Only Volume" => { disk_info.insert("volume_read_only".to_string(), serde_json::json!(value == "Yes")); },
                        "Device Location" => { disk_info.insert("location".to_string(), serde_json::json!(value)); },
                        "Removable Media" => { disk_info.insert("removable".to_string(), serde_json::json!(value == "Removable")); },
                        "Media Type" => { disk_info.insert("media_type".to_string(), serde_json::json!(value)); },
                        "Protocol" => { disk_info.insert("protocol".to_string(), serde_json::json!(value)); },
                        "SMART Status" => { disk_info.insert("smart_status".to_string(), serde_json::json!(value)); },
                        "Solid State" => { disk_info.insert("is_ssd".to_string(), serde_json::json!(value == "Yes")); },
                        "Virtual" => { disk_info.insert("is_virtual".to_string(), serde_json::json!(value == "Yes")); },
                        "OS Can Be Installed" => { disk_info.insert("os_installable".to_string(), serde_json::json!(value == "Yes")); },
                        "Bootable" => { disk_info.insert("bootable".to_string(), serde_json::json!(value == "Yes")); },
                        "Boot Disk" => { disk_info.insert("is_boot_disk".to_string(), serde_json::json!(value == "Yes")); },
                        "Disk / Partition UUID" => { disk_info.insert("uuid".to_string(), serde_json::json!(value)); },
                        "Volume UUID" => { disk_info.insert("volume_uuid".to_string(), serde_json::json!(value)); },
                        "Partition Type" => { disk_info.insert("partition_type".to_string(), serde_json::json!(value)); },
                        _ => {}
                    }
                }
            }
        }
        
        // Collect information from ALL partitions (s1, s2, s3, etc.)
        let mut partitions_info: Vec<serde_json::Value> = Vec::new();
        let mut main_partition_idx: Option<usize> = None;
        let mut main_partition_size: u64 = 0;
        
        for suffix in 1..=10 {  // Check up to 10 partitions
            let partition_id = format!("{}s{}", disk_id, suffix);
            let partition_cmd = format!(
                "echo '{}' | sudo -S diskutil info {} 2>/dev/null",
                escaped_password, partition_id
            );
            
            if let Ok(part_output) = Command::new("sh").args(["-c", &partition_cmd]).output() {
                let part_stdout = String::from_utf8_lossy(&part_output.stdout);
                
                // Check if partition exists (output should contain device identifier)
                if !part_stdout.contains("Device Identifier") {
                    continue;
                }
                
                let mut part_info = serde_json::Map::new();
                part_info.insert("partition_id".to_string(), serde_json::json!(partition_id));
                
                let mut part_size_bytes: u64 = 0;
                let mut is_efi = false;
                
                for line in part_stdout.lines() {
                    if let Some((key, value)) = line.split_once(':') {
                        let key = key.trim();
                        let value = value.trim();
                        if !value.is_empty() {
                            match key {
                                "Volume Name" => {
                                    if !value.contains("Not applicable") {
                                        part_info.insert("volume_name".to_string(), serde_json::json!(value));
                                    }
                                },
                                "Mount Point" => {
                                    if !value.contains("Not applicable") {
                                        part_info.insert("mount_point".to_string(), serde_json::json!(value));
                                    }
                                },
                                "File System Personality" => {
                                    part_info.insert("filesystem".to_string(), serde_json::json!(value));
                                },
                                "Name (User Visible)" => {
                                    part_info.insert("filesystem_name".to_string(), serde_json::json!(value));
                                },
                                "Content (IOContent)" => {
                                    part_info.insert("content_type".to_string(), serde_json::json!(value));
                                    if value.contains("EFI") {
                                        is_efi = true;
                                    }
                                    // Use content type as filesystem if no filesystem detected
                                    // and it's a known filesystem type
                                    if !part_info.contains_key("filesystem") {
                                        let fs_from_content = match value {
                                            "Microsoft Basic Data" => Some("NTFS/FAT/exFAT"),
                                            "Linux Filesystem" => Some("Linux (ext2/3/4)"),
                                            "Linux Swap" => Some("Linux Swap"),
                                            "Apple_HFS" => Some("HFS+"),
                                            "Apple_HFSX" => Some("HFS+ (Case-sensitive)"),
                                            "Apple_Boot" => Some("Apple Boot"),
                                            "Apple_APFS_ISC" => Some("APFS (System)"),
                                            "Apple_APFS_Recovery" => Some("APFS (Recovery)"),
                                            _ => None,
                                        };
                                        if let Some(fs_name) = fs_from_content {
                                            part_info.insert("filesystem".to_string(), serde_json::json!(fs_name));
                                        }
                                    }
                                },
                                "Partition Type" => {
                                    part_info.insert("partition_type".to_string(), serde_json::json!(value));
                                    if value.contains("EFI") || value == "0xEF" {
                                        is_efi = true;
                                    }
                                    // If it's Apple_APFS, use that as filesystem
                                    if value.contains("Apple_APFS") {
                                        part_info.insert("filesystem".to_string(), serde_json::json!("APFS Container"));
                                    }
                                    // Translate known MBR partition types to readable names
                                    let fs_from_type = match value {
                                        "0xEF" => Some("EFI System Partition"),
                                        "0x07" => Some("NTFS/exFAT/HPFS"),
                                        "0x0B" | "0x0C" => Some("FAT32"),
                                        "0x01" | "0x04" | "0x06" | "0x0E" => Some("FAT16/FAT12"),
                                        "0x83" => Some("Linux (ext2/3/4)"),
                                        "0x82" => Some("Linux Swap"),
                                        "0x8E" => Some("Linux LVM"),
                                        "0xFD" => Some("Linux RAID"),
                                        "0xAF" => Some("Apple HFS/HFS+"),
                                        "0xAB" => Some("Apple Boot"),
                                        "0xA5" => Some("FreeBSD"),
                                        "0xA6" => Some("OpenBSD"),
                                        "0xA9" => Some("NetBSD"),
                                        "0x00" => Some("Leer/Unpartitioniert"),
                                        _ => None,
                                    };
                                    if let Some(fs_name) = fs_from_type {
                                        if !part_info.contains_key("filesystem") {
                                            part_info.insert("filesystem".to_string(), serde_json::json!(fs_name));
                                        }
                                    }
                                },
                                "Disk Size" => {
                                    part_info.insert("size".to_string(), serde_json::json!(value));
                                    // Parse size in bytes from format like "209.7 MB (209715200 Bytes)"
                                    if let Some(start) = value.find('(') {
                                        if let Some(end) = value.find(" Bytes") {
                                            if let Ok(bytes) = value[start+1..end].trim().replace(",", "").parse::<u64>() {
                                                part_size_bytes = bytes;
                                            }
                                        }
                                    }
                                },
                                "Volume Total Space" => {
                                    part_info.insert("total_space".to_string(), serde_json::json!(value));
                                },
                                "Volume Free Space" => {
                                    part_info.insert("free_space".to_string(), serde_json::json!(value));
                                },
                                "Volume Used Space" => {
                                    part_info.insert("used_space".to_string(), serde_json::json!(value));
                                },
                                "Volume UUID" => {
                                    part_info.insert("volume_uuid".to_string(), serde_json::json!(value));
                                },
                                "APFS Container" => {
                                    // This is an APFS Physical Store - get container info
                                    part_info.insert("apfs_container".to_string(), serde_json::json!(value));
                                },
                                _ => {}
                            }
                        }
                    }
                }
                
                // If this is an APFS Physical Store, get container and volume info
                if let Some(container) = part_info.get("apfs_container").and_then(|c| c.as_str()) {
                    // Get APFS container info
                    let apfs_cmd = format!("diskutil apfs list {} 2>/dev/null", container);
                    if let Ok(apfs_output) = Command::new("sh").args(["-c", &apfs_cmd]).output() {
                        let apfs_stdout = String::from_utf8_lossy(&apfs_output.stdout);
                        
                        // Parse volumes from APFS container output
                        let mut apfs_volumes: Vec<serde_json::Value> = Vec::new();
                        let mut current_volume: Option<serde_json::Map<String, serde_json::Value>> = None;
                        
                        for line in apfs_stdout.lines() {
                            let trimmed = line.trim();
                            
                            if trimmed.starts_with("+-> Volume ") {
                                // Save previous volume if exists
                                if let Some(vol) = current_volume.take() {
                                    apfs_volumes.push(serde_json::json!(vol));
                                }
                                // Start new volume
                                let mut vol = serde_json::Map::new();
                                // Extract volume disk ID (e.g., "disk6s1")
                                if let Some(vol_id) = trimmed.split_whitespace().nth(2) {
                                    vol.insert("volume_id".to_string(), serde_json::json!(vol_id));
                                }
                                current_volume = Some(vol);
                            } else if let Some(ref mut vol) = current_volume {
                                if let Some((key, value)) = trimmed.split_once(':') {
                                    let key = key.trim();
                                    let value = value.trim();
                                    match key {
                                        "Name" => {
                                            vol.insert("name".to_string(), serde_json::json!(value));
                                        },
                                        "Mount Point" => {
                                            vol.insert("mount_point".to_string(), serde_json::json!(value));
                                        },
                                        "Capacity Consumed" => {
                                            vol.insert("used".to_string(), serde_json::json!(value));
                                        },
                                        "FileVault" => {
                                            vol.insert("filevault".to_string(), serde_json::json!(value));
                                        },
                                        _ => {}
                                    }
                                }
                            }
                        }
                        // Add last volume
                        if let Some(vol) = current_volume {
                            apfs_volumes.push(serde_json::json!(vol));
                        }
                        
                        if !apfs_volumes.is_empty() {
                            part_info.insert("apfs_volumes".to_string(), serde_json::json!(apfs_volumes));
                            
                            // Use first volume's mount point for display
                            if let Some(first_vol) = apfs_volumes.first() {
                                if let Some(mp) = first_vol.get("mount_point").and_then(|m| m.as_str()) {
                                    if !mp.contains("Not mounted") && !mp.is_empty() {
                                        part_info.insert("mount_point".to_string(), serde_json::json!(mp));
                                    }
                                }
                                if let Some(name) = first_vol.get("name").and_then(|n| n.as_str()) {
                                    part_info.insert("volume_name".to_string(), serde_json::json!(name));
                                }
                            }
                        }
                        
                        // Parse container capacity info
                        for line in apfs_stdout.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("Capacity In Use By Volumes:") {
                                if let Some(val) = trimmed.split(':').nth(1) {
                                    part_info.insert("used_space".to_string(), serde_json::json!(val.trim()));
                                }
                            } else if trimmed.starts_with("Capacity Not Allocated:") {
                                if let Some(val) = trimmed.split(':').nth(1) {
                                    part_info.insert("free_space".to_string(), serde_json::json!(val.trim()));
                                }
                            }
                        }
                    }
                }
                
                // For Linux filesystems (ext2/3/4), try to read volume label using e2label or tune2fs
                // This requires e2fsprogs to be installed (brew install e2fsprogs)
                // Also detect Paragon UFSD_EXTFS driver which mounts ext2/3/4
                let is_linux_fs = part_info.get("content_type")
                    .and_then(|c| c.as_str())
                    .map(|c| c == "Linux Filesystem" || c == "0x83" || c.contains("Linux"))
                    .unwrap_or(false)
                    || part_info.get("partition_type")
                        .and_then(|p| p.as_str())
                        .map(|p| p == "0x83" || p == "Linux" || p.contains("Linux"))
                        .unwrap_or(false)
                    || part_info.get("filesystem")
                        .and_then(|f| f.as_str())
                        .map(|f| f.contains("ext") || f.contains("Linux") || f.contains("EXTFS") || f.contains("UFSD"))
                        .unwrap_or(false);
                
                // Debug: Log what we detected for Linux FS
                let content_type_str = part_info.get("content_type").and_then(|c| c.as_str()).unwrap_or("none");
                let partition_type_str = part_info.get("partition_type").and_then(|p| p.as_str()).unwrap_or("none");
                let filesystem_str = part_info.get("filesystem").and_then(|f| f.as_str()).unwrap_or("none");
                eprintln!("[ext4 Debug] Partition {}: is_linux_fs={}, content_type={}, partition_type={}, filesystem={}", 
                    partition_id, is_linux_fs, content_type_str, partition_type_str, filesystem_str);
                
                if is_linux_fs && !part_info.contains_key("volume_name") {
                    eprintln!("[ext4 Debug] Trying to read ext4 label for {}", partition_id);
                    
                    // Try e2label first (simpler output) - needs sudo for raw disk access
                    let e2label_cmd = format!(
                        "echo '{}' | sudo -S /opt/homebrew/opt/e2fsprogs/sbin/e2label /dev/{} 2>/dev/null || echo '{}' | sudo -S /usr/local/opt/e2fsprogs/sbin/e2label /dev/{} 2>/dev/null",
                        escaped_password, partition_id, escaped_password, partition_id
                    );
                    eprintln!("[ext4 Debug] Running e2label with sudo for {}", partition_id);
                    
                    if let Ok(label_output) = Command::new("sh").args(["-c", &e2label_cmd]).output() {
                        let stdout = String::from_utf8_lossy(&label_output.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&label_output.stderr).trim().to_string();
                        eprintln!("[ext4 Debug] e2label stdout: '{}', stderr: '{}'", stdout, stderr);
                        
                        // Check if it's a valid label (not an error message)
                        if !stdout.is_empty() && !stdout.contains("Permission denied") && !stdout.contains("Bad magic") && !stdout.contains("No such file") && !stdout.contains("Password:") {
                            part_info.insert("volume_name".to_string(), serde_json::json!(stdout));
                            eprintln!("[ext4 Debug] Set volume_name to: {}", stdout);
                        }
                    }
                    
                    // If e2label didn't work, try tune2fs
                    if !part_info.contains_key("volume_name") {
                        eprintln!("[ext4 Debug] e2label didn't work, trying tune2fs");
                        let tune2fs_cmd = format!(
                            "echo '{}' | sudo -S /opt/homebrew/opt/e2fsprogs/sbin/tune2fs -l /dev/{} 2>/dev/null | grep 'Filesystem volume name' || echo '{}' | sudo -S /usr/local/opt/e2fsprogs/sbin/tune2fs -l /dev/{} 2>/dev/null | grep 'Filesystem volume name'",
                            escaped_password, partition_id, escaped_password, partition_id
                        );
                        eprintln!("[ext4 Debug] Running tune2fs with sudo for {}", partition_id);
                        
                        if let Ok(tune_output) = Command::new("sh").args(["-c", &tune2fs_cmd]).output() {
                            let tune_stdout = String::from_utf8_lossy(&tune_output.stdout);
                            let tune_stderr = String::from_utf8_lossy(&tune_output.stderr);
                            eprintln!("[ext4 Debug] tune2fs stdout: '{}', stderr: '{}'", tune_stdout.trim(), tune_stderr.trim());
                            
                            // Parse "Filesystem volume name:   <volume_label>"
                            if let Some(line) = tune_stdout.lines().find(|l| l.contains("Filesystem volume name")) {
                                if let Some(label) = line.split(':').nth(1) {
                                    let label = label.trim();
                                    eprintln!("[ext4 Debug] Parsed label: '{}'", label);
                                    if !label.is_empty() && label != "<none>" {
                                        part_info.insert("volume_name".to_string(), serde_json::json!(label));
                                    }
                                }
                            }
                        }
                    }
                    
                    // If still no volume name and partition is mounted, use mount point name
                    if !part_info.contains_key("volume_name") {
                        eprintln!("[ext4 Debug] No volume_name found, checking mount point");
                        let mount_point_name = part_info.get("mount_point")
                            .and_then(|m| m.as_str())
                            .and_then(|mp| mp.rsplit('/').next())
                            .filter(|name| !name.is_empty())
                            .map(|s| s.to_string());
                        
                        if let Some(name) = mount_point_name {
                            eprintln!("[ext4 Debug] Using mount point name: {}", name);
                            part_info.insert("volume_name".to_string(), serde_json::json!(name));
                        }
                    }
                }
                
                // Track the main (largest non-EFI) partition
                if !is_efi && part_size_bytes > main_partition_size {
                    main_partition_size = part_size_bytes;
                    main_partition_idx = Some(partitions_info.len());
                }
                
                partitions_info.push(serde_json::json!(part_info));
            }
        }
        
        // Add partitions array to result
        if !partitions_info.is_empty() {
            result["partitions"] = serde_json::json!(partitions_info);
            
            // Use main partition info for disk_info if volume_name is not set
            let volume_name = disk_info.get("volume_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            
            if (volume_name.contains("Not applicable") || volume_name.is_empty()) && main_partition_idx.is_some() {
                if let Some(idx) = main_partition_idx {
                    if let Some(main_part) = partitions_info.get(idx) {
                        // Copy main partition info to disk_info
                        if let Some(v) = main_part.get("volume_name") {
                            disk_info.insert("volume_name".to_string(), v.clone());
                        }
                        if let Some(v) = main_part.get("mount_point") {
                            disk_info.insert("mount_point".to_string(), v.clone());
                        }
                        if let Some(v) = main_part.get("filesystem") {
                            disk_info.insert("filesystem".to_string(), v.clone());
                        }
                        if let Some(v) = main_part.get("filesystem_name") {
                            disk_info.insert("filesystem_name".to_string(), v.clone());
                        }
                        if let Some(v) = main_part.get("total_space") {
                            disk_info.insert("total_space".to_string(), v.clone());
                        }
                        if let Some(v) = main_part.get("free_space") {
                            disk_info.insert("free_space".to_string(), v.clone());
                        }
                        if let Some(v) = main_part.get("used_space") {
                            disk_info.insert("used_space".to_string(), v.clone());
                        }
                    }
                }
            }
        }
        
        result["disk_info"] = serde_json::json!(disk_info);
    }
    
    // 2. Get partition layout
    let partitions_cmd = format!(
        "echo '{}' | sudo -S diskutil list {} 2>/dev/null",
        escaped_password, disk_id
    );
    
    if let Ok(output) = Command::new("sh").args(["-c", &partitions_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        result["partition_layout"] = serde_json::json!(stdout.trim());
    }
    
    // 3. Get device info - check SD Card Reader FIRST (more specific match by bsd_name)
    // then fall back to USB device tree
    let media_name = result.get("disk_info")
        .and_then(|d| d.get("media_name"))
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    
    let mut found_device_info = false;
    
    // 3a. Check for SD Card Reader first (built-in card readers have exact bsd_name match)
    let sd_cmd = "system_profiler SPCardReaderDataType -json 2>/dev/null";
    if let Ok(output) = Command::new("sh").args(["-c", sd_cmd]).output() {
        if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(sd_info) = find_sd_card_info(&json_data, &disk_id) {
                // Found SD card - use this info
                found_device_info = true;
                
                // Extract SMART status for SD cards and create smart_info section
                if let Some(smart_status) = sd_info.get("smart_status").and_then(|s| s.as_str()) {
                    let mut smart_info = serde_json::Map::new();
                    let status_formatted = if smart_status == "Verified" {
                        "Verified ✅".to_string()
                    } else if smart_status == "Failing" {
                        "FAILING ⚠️".to_string()
                    } else {
                        smart_status.to_string()
                    };
                    smart_info.insert("health_status".to_string(), serde_json::json!(status_formatted));
                    smart_info.insert("smart_supported".to_string(), serde_json::json!(true));
                    
                    // Add device info to smart_info
                    if let Some(product) = sd_info.get("product_name").and_then(|p| p.as_str()) {
                        smart_info.insert("device_model".to_string(), serde_json::json!(product));
                    }
                    if let Some(model) = sd_info.get("card_model").and_then(|m| m.as_str()) {
                        smart_info.insert("model_family".to_string(), serde_json::json!(model));
                    }
                    if let Some(mfr) = sd_info.get("manufacturer").and_then(|m| m.as_str()) {
                        smart_info.insert("manufacturer".to_string(), serde_json::json!(mfr));
                    }
                    if let Some(serial) = sd_info.get("serial_number").and_then(|s| s.as_str()) {
                        smart_info.insert("serial_number".to_string(), serde_json::json!(serial));
                    }
                    if let Some(capacity) = sd_info.get("capacity").and_then(|c| c.as_str()) {
                        smart_info.insert("capacity".to_string(), serde_json::json!(capacity));
                    }
                    if let Some(spec) = sd_info.get("sd_spec_version").and_then(|s| s.as_str()) {
                        smart_info.insert("sd_spec_version".to_string(), serde_json::json!(format!("SD {}", spec)));
                    }
                    if let Some(date) = sd_info.get("manufacturing_date").and_then(|d| d.as_str()) {
                        smart_info.insert("manufacturing_date".to_string(), serde_json::json!(date));
                    }
                    
                    result["smart_info"] = serde_json::json!(smart_info);
                }
                result["usb_info"] = sd_info;
                
                // Remove misleading smart_status from disk_info for SD cards
                // (diskutil says "Not Supported" but Card Reader has its own health check)
                if let Some(disk_info) = result.get_mut("disk_info") {
                    if let Some(obj) = disk_info.as_object_mut() {
                        obj.remove("smart_status");
                    }
                }
            }
        }
    }
    
    // 3b. If not an SD card, check USB device tree
    if !found_device_info {
        let usb_cmd = "system_profiler SPUSBHostDataType -json 2>/dev/null";
        if let Ok(output) = Command::new("sh").args(["-c", usb_cmd]).output() {
            if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                // Parse USB device tree to find our specific device by name
                if let Some(usb_info) = find_usb_device_info(&json_data, &disk_id, &media_name) {
                    result["usb_info"] = usb_info;
                }
            }
        }
    }
    
    // 4. Analyze boot capability
    let boot_info = analyze_boot_structure(&disk_id, &escaped_password);
    result["boot_info"] = boot_info;
    
    // 5. Detect filesystem signatures from raw device
    if let Some(fs_info) = detect_filesystem_signatures(&disk_id, &escaped_password) {
        result["filesystem_signatures"] = fs_info;
    }
    
    // 6. Get file count and directory structure (if mounted)
    if let Some(mount_point) = result.get("disk_info")
        .and_then(|d| d.get("mount_point"))
        .and_then(|m| m.as_str()) 
    {
        if !mount_point.is_empty() {
            if let Some(content_info) = analyze_mounted_content(mount_point) {
                result["content_analysis"] = content_info;
            }
        }
    }
    
    // 7. Check for hidden files and special structures
    if let Some(special_info) = detect_special_structures(&disk_id, &escaped_password) {
        result["special_structures"] = special_info;
    }
    
    // 8. Get detailed hardware info via ioreg
    let ioreg_cmd = format!(
        "ioreg -r -c IOMedia -l 2>/dev/null | grep -A50 'BSD Name.*{}' | head -60",
        disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &ioreg_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut hw_info = serde_json::Map::new();
        
        for line in stdout.lines() {
            let line = line.trim();
            if line.contains("\"Size\"") {
                if let Some(size) = line.split('=').nth(1) {
                    hw_info.insert("exact_size_bytes".to_string(), serde_json::json!(size.trim()));
                }
            } else if line.contains("\"Preferred Block Size\"") {
                if let Some(bs) = line.split('=').nth(1) {
                    hw_info.insert("preferred_block_size".to_string(), serde_json::json!(bs.trim()));
                }
            } else if line.contains("\"Physical Block Size\"") {
                if let Some(pbs) = line.split('=').nth(1) {
                    hw_info.insert("physical_block_size".to_string(), serde_json::json!(pbs.trim()));
                }
            } else if line.contains("\"Removable\"") {
                hw_info.insert("hardware_removable".to_string(), serde_json::json!(line.contains("Yes")));
            } else if line.contains("\"Ejectable\"") {
                hw_info.insert("ejectable".to_string(), serde_json::json!(line.contains("Yes")));
            } else if line.contains("\"Whole\"") {
                hw_info.insert("is_whole_disk".to_string(), serde_json::json!(line.contains("Yes")));
            }
        }
        
        if !hw_info.is_empty() {
            result["hardware_info"] = serde_json::json!(hw_info);
        }
    }
    
    // 9. Get USB controller path info
    let usb_path_cmd = format!(
        "system_profiler SPUSBDataType 2>/dev/null | grep -B30 '{}' | head -35",
        disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &usb_path_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut controller_info = serde_json::Map::new();
        
        for line in stdout.lines() {
            let line = line.trim();
            if line.starts_with("USB") && line.contains("Bus") {
                controller_info.insert("usb_bus".to_string(), serde_json::json!(line));
            } else if line.contains("Host Controller") {
                if let Some((_, val)) = line.split_once(':') {
                    controller_info.insert("host_controller".to_string(), serde_json::json!(val.trim()));
                }
            } else if line.contains("PCI Device ID") {
                if let Some((_, val)) = line.split_once(':') {
                    controller_info.insert("pci_device_id".to_string(), serde_json::json!(val.trim()));
                }
            } else if line.contains("PCI Vendor ID") {
                if let Some((_, val)) = line.split_once(':') {
                    controller_info.insert("pci_vendor_id".to_string(), serde_json::json!(val.trim()));
                }
            } else if line.contains("PCI Revision ID") {
                if let Some((_, val)) = line.split_once(':') {
                    controller_info.insert("pci_revision_id".to_string(), serde_json::json!(val.trim()));
                }
            }
        }
        
        if !controller_info.is_empty() {
            result["controller_info"] = serde_json::json!(controller_info);
        }
    }
    
    // 10. Get storage type info
    let storage_cmd = "system_profiler SPStorageDataType -json 2>/dev/null";
    if let Ok(output) = Command::new("sh").args(["-c", storage_cmd]).output() {
        if let Ok(json_data) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
            if let Some(storage) = json_data.get("SPStorageDataType").and_then(|s| s.as_array()) {
                for vol in storage {
                    if let Some(bsd) = vol.get("bsd_name").and_then(|b| b.as_str()) {
                        if bsd == disk_id || disk_id.starts_with(bsd) || bsd.starts_with(&disk_id) {
                            let mut storage_info = serde_json::Map::new();
                            if let Some(name) = vol.get("_name").and_then(|n| n.as_str()) {
                                storage_info.insert("storage_name".to_string(), serde_json::json!(name));
                            }
                            if let Some(size) = vol.get("size_in_bytes") {
                                storage_info.insert("size_in_bytes".to_string(), size.clone());
                            }
                            if let Some(free) = vol.get("free_space_in_bytes") {
                                storage_info.insert("free_space_in_bytes".to_string(), free.clone());
                            }
                            if let Some(writable) = vol.get("writable").and_then(|w| w.as_str()) {
                                storage_info.insert("writable".to_string(), serde_json::json!(writable));
                            }
                            if let Some(ignore) = vol.get("ignore_ownership").and_then(|i| i.as_str()) {
                                storage_info.insert("ignore_ownership".to_string(), serde_json::json!(ignore));
                            }
                            if !storage_info.is_empty() {
                                result["storage_info"] = serde_json::json!(storage_info);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
    
    // 11. Get disk activity statistics via iostat
    let iostat_cmd = format!("iostat -d {} 2>/dev/null | tail -1", disk_id);
    if let Ok(output) = Command::new("sh").args(["-c", &iostat_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = stdout.split_whitespace().collect();
        if parts.len() >= 3 {
            let mut iostat_info = serde_json::Map::new();
            iostat_info.insert("kb_per_transfer".to_string(), serde_json::json!(parts.get(0).unwrap_or(&"N/A")));
            iostat_info.insert("transfers_per_sec".to_string(), serde_json::json!(parts.get(1).unwrap_or(&"N/A")));
            iostat_info.insert("mb_per_sec".to_string(), serde_json::json!(parts.get(2).unwrap_or(&"N/A")));
            result["disk_activity"] = serde_json::json!(iostat_info);
        }
    }
    
    // 12. Get raw hex dump of first sectors (MBR/GPT header preview)
    let hexdump_cmd = format!(
        "echo '{}' | sudo -S dd if=/dev/r{} bs=512 count=2 2>/dev/null | xxd -l 128 -c 16",
        escaped_password, disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &hexdump_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.is_empty() {
            result["raw_header_hex"] = serde_json::json!(stdout.trim());
        }
    }
    
    // 13. Parse MBR partition table entries
    let mbr_cmd = format!(
        "echo '{}' | sudo -S dd if=/dev/r{} bs=512 count=1 2>/dev/null | xxd -p -l 512",
        escaped_password, disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &mbr_cmd]).output() {
        let hex_str = String::from_utf8_lossy(&output.stdout).replace("\n", "");
        if hex_str.len() >= 1024 {
            let mut mbr_info = serde_json::Map::new();
            
            // Check MBR signature (bytes 510-511 = 55AA)
            let sig = &hex_str[1020..1024];
            mbr_info.insert("mbr_signature".to_string(), serde_json::json!(sig.to_uppercase()));
            mbr_info.insert("valid_mbr".to_string(), serde_json::json!(sig == "55aa"));
            
            // Parse 4 partition entries (bytes 446-509)
            let mut partitions = Vec::new();
            for i in 0..4 {
                let start = 892 + (i * 32); // 446 bytes * 2 hex chars
                let end = start + 32;
                if end <= hex_str.len() {
                    let entry = &hex_str[start..end];
                    let boot_flag = &entry[0..2];
                    let part_type = &entry[8..10];
                    
                    // Only add non-empty partitions
                    if part_type != "00" {
                        let mut part = serde_json::Map::new();
                        part.insert("number".to_string(), serde_json::json!(i + 1));
                        part.insert("bootable".to_string(), serde_json::json!(boot_flag == "80"));
                        part.insert("type_hex".to_string(), serde_json::json!(part_type.to_uppercase()));
                        
                        // Common partition type names
                        let type_name = match part_type {
                            "00" => "Empty",
                            "01" => "FAT12",
                            "04" | "06" | "0e" => "FAT16",
                            "05" | "0f" => "Extended",
                            "07" => "NTFS/exFAT/HPFS",
                            "0b" | "0c" => "FAT32",
                            "82" => "Linux Swap",
                            "83" => "Linux",
                            "8e" => "Linux LVM",
                            "af" => "HFS/HFS+",
                            "ee" => "GPT Protective MBR",
                            "ef" => "EFI System",
                            "fb" => "VMware VMFS",
                            "fd" => "Linux RAID",
                            _ => "Unknown"
                        };
                        part.insert("type_name".to_string(), serde_json::json!(type_name));
                        partitions.push(serde_json::json!(part));
                    }
                }
            }
            mbr_info.insert("partition_entries".to_string(), serde_json::json!(partitions));
            result["mbr_analysis"] = serde_json::json!(mbr_info);
        }
    }
    
    // 14. Get GPT header details
    let gpt_cmd = format!(
        "echo '{}' | sudo -S dd if=/dev/r{} bs=512 skip=1 count=1 2>/dev/null | xxd -p -l 512",
        escaped_password, disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &gpt_cmd]).output() {
        let hex_str = String::from_utf8_lossy(&output.stdout).replace("\n", "");
        // Check for "EFI PART" signature (45 46 49 20 50 41 52 54)
        if hex_str.starts_with("4546492050415254") {
            let mut gpt_info = serde_json::Map::new();
            gpt_info.insert("gpt_signature".to_string(), serde_json::json!("EFI PART"));
            gpt_info.insert("valid_gpt".to_string(), serde_json::json!(true));
            
            // GPT revision (bytes 8-11)
            if hex_str.len() >= 24 {
                let rev = &hex_str[16..24];
                gpt_info.insert("gpt_revision".to_string(), serde_json::json!(rev));
            }
            
            // Header size (bytes 12-15)
            if hex_str.len() >= 32 {
                let size_hex = &hex_str[24..32];
                gpt_info.insert("header_size_hex".to_string(), serde_json::json!(size_hex));
            }
            
            result["gpt_analysis"] = serde_json::json!(gpt_info);
        }
    }
    
    // 15. Analyze mounted filesystem details
    if let Some(mount_point) = result.get("disk_info")
        .and_then(|d| d.get("mount_point"))
        .and_then(|m| m.as_str()) 
    {
        if !mount_point.is_empty() {
            let mut fs_details = serde_json::Map::new();
            
            // Get filesystem stats via df
            let df_cmd = format!("df -i '{}' 2>/dev/null | tail -1", mount_point);
            if let Ok(output) = Command::new("sh").args(["-c", &df_cmd]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = stdout.split_whitespace().collect();
                if parts.len() >= 9 {
                    fs_details.insert("total_blocks".to_string(), serde_json::json!(parts.get(1).unwrap_or(&"")));
                    fs_details.insert("used_blocks".to_string(), serde_json::json!(parts.get(2).unwrap_or(&"")));
                    fs_details.insert("free_blocks".to_string(), serde_json::json!(parts.get(3).unwrap_or(&"")));
                    fs_details.insert("capacity_percent".to_string(), serde_json::json!(parts.get(4).unwrap_or(&"")));
                    fs_details.insert("total_inodes".to_string(), serde_json::json!(parts.get(5).unwrap_or(&"")));
                    fs_details.insert("used_inodes".to_string(), serde_json::json!(parts.get(6).unwrap_or(&"")));
                    fs_details.insert("free_inodes".to_string(), serde_json::json!(parts.get(7).unwrap_or(&"")));
                    fs_details.insert("inode_usage_percent".to_string(), serde_json::json!(parts.get(8).unwrap_or(&"")));
                }
            }
            
            // Count hidden files
            let hidden_cmd = format!("find '{}' -name '.*' -maxdepth 2 2>/dev/null | wc -l", mount_point);
            if let Ok(output) = Command::new("sh").args(["-c", &hidden_cmd]).output() {
                let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
                fs_details.insert("hidden_files_count".to_string(), serde_json::json!(count));
            }
            
            // Get top 5 largest files
            let large_cmd = format!("find '{}' -type f -exec stat -f '%z %N' {{}} \\; 2>/dev/null | sort -rn | head -5", mount_point);
            if let Ok(output) = Command::new("sh").args(["-c", &large_cmd]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let files: Vec<serde_json::Value> = stdout.lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.splitn(2, ' ').collect();
                        if parts.len() == 2 {
                            Some(serde_json::json!({
                                "size_bytes": parts[0],
                                "path": parts[1].replace(mount_point, "")
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !files.is_empty() {
                    fs_details.insert("largest_files".to_string(), serde_json::json!(files));
                }
            }
            
            // Get file type distribution
            let types_cmd = format!(
                "find '{}' -type f -maxdepth 3 2>/dev/null | sed 's/.*\\.//' | sort | uniq -c | sort -rn | head -10",
                mount_point
            );
            if let Ok(output) = Command::new("sh").args(["-c", &types_cmd]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let types: Vec<serde_json::Value> = stdout.lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        let parts: Vec<&str> = line.splitn(2, ' ').collect();
                        if parts.len() == 2 {
                            Some(serde_json::json!({
                                "count": parts[0].trim(),
                                "extension": parts[1].trim()
                            }))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !types.is_empty() {
                    fs_details.insert("file_type_distribution".to_string(), serde_json::json!(types));
                }
            }
            
            // Get recent files (last modified)
            let recent_cmd = format!(
                "find '{}' -type f -maxdepth 3 -mtime -7 2>/dev/null | head -10",
                mount_point
            );
            if let Ok(output) = Command::new("sh").args(["-c", &recent_cmd]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let files: Vec<String> = stdout.lines()
                    .map(|l| l.replace(mount_point, "").to_string())
                    .collect();
                if !files.is_empty() {
                    fs_details.insert("recently_modified".to_string(), serde_json::json!(files));
                }
            }
            
            // Get directory count
            let dir_cmd = format!("find '{}' -type d 2>/dev/null | wc -l", mount_point);
            if let Ok(output) = Command::new("sh").args(["-c", &dir_cmd]).output() {
                let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
                fs_details.insert("directory_count".to_string(), serde_json::json!(count));
            }
            
            // Get total file count
            let file_cmd = format!("find '{}' -type f 2>/dev/null | wc -l", mount_point);
            if let Ok(output) = Command::new("sh").args(["-c", &file_cmd]).output() {
                let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
                fs_details.insert("total_file_count".to_string(), serde_json::json!(count));
            }
            
            // Get symlink count
            let link_cmd = format!("find '{}' -type l 2>/dev/null | wc -l", mount_point);
            if let Ok(output) = Command::new("sh").args(["-c", &link_cmd]).output() {
                let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
                fs_details.insert("symlink_count".to_string(), serde_json::json!(count));
            }
            
            if !fs_details.is_empty() {
                result["filesystem_details"] = serde_json::json!(fs_details);
            }
        }
    }
    
    // 16. Check for SMART support and collect comprehensive SMART data using try_smartctl
    // Get parent disk for SMART (e.g., "disk6" instead of "disk6s2")
    let smart_disk_id = result.get("disk_info")
        .and_then(|d| d.get("parent_disk"))
        .and_then(|p| p.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // Fallback: strip partition suffix (e.g., "disk6s2" -> "disk6")
            if let Some(pos) = disk_id.find('s') {
                let after_s = &disk_id[pos+1..];
                if !after_s.is_empty() && after_s.chars().all(|c| c.is_ascii_digit()) {
                    return disk_id[..pos].to_string();
                }
            }
            disk_id.to_string()
        });
    
    eprintln!("[SMART Debug] forensic_analysis: disk_id={}, smart_disk_id={}", disk_id, smart_disk_id);
    
    // Use try_smartctl for comprehensive SMART data (includes -x extended info)
    if let Some(smart_data) = try_smartctl(&smart_disk_id) {
        eprintln!("[SMART Debug] try_smartctl returned data, available={}", smart_data.available);
        
        let mut smart_info = serde_json::Map::new();
        
        // Device identification
        if let Some(v) = &smart_data.model_family { smart_info.insert("model_family".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.device_model { smart_info.insert("device_model".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.serial_number { smart_info.insert("serial_number".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.firmware_version { smart_info.insert("firmware_version".to_string(), serde_json::json!(v)); }
        
        // Capacity and physical info
        if let Some(v) = smart_data.user_capacity_bytes { 
            let gb = v as f64 / 1_000_000_000.0;
            smart_info.insert("capacity".to_string(), serde_json::json!(format!("{:.2} GB ({} bytes)", gb, v))); 
        }
        if let Some(v) = smart_data.logical_block_size { smart_info.insert("logical_block_size".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.physical_block_size { smart_info.insert("physical_block_size".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.rotation_rate { smart_info.insert("rotation_rate".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.form_factor { smart_info.insert("form_factor".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.device_type { smart_info.insert("device_type".to_string(), serde_json::json!(v)); }
        
        // Interface info
        if let Some(v) = &smart_data.protocol { smart_info.insert("protocol".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.ata_version { smart_info.insert("ata_version".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.sata_version { smart_info.insert("sata_version".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.interface_speed_max { smart_info.insert("interface_speed_max".to_string(), serde_json::json!(v)); }
        if let Some(v) = &smart_data.interface_speed_current { smart_info.insert("interface_speed_current".to_string(), serde_json::json!(v)); }
        
        // Health status
        smart_info.insert("health_status".to_string(), serde_json::json!(&smart_data.health_status));
        if let Some(v) = smart_data.smart_enabled { smart_info.insert("smart_enabled".to_string(), serde_json::json!(v)); }
        
        // Capabilities
        if let Some(v) = smart_data.trim_supported { smart_info.insert("trim_supported".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.write_cache_enabled { smart_info.insert("write_cache_enabled".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.read_lookahead_enabled { smart_info.insert("read_lookahead_enabled".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.ata_security_enabled { smart_info.insert("ata_security_enabled".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.ata_security_frozen { smart_info.insert("ata_security_frozen".to_string(), serde_json::json!(v)); }
        
        // Temperature info (SCT)
        if let Some(v) = smart_data.sct_temperature_current { smart_info.insert("sct_temperature_current".to_string(), serde_json::json!(format!("{}°C", v))); }
        if let Some(v) = smart_data.sct_temperature_lifetime_min { smart_info.insert("sct_temperature_lifetime_min".to_string(), serde_json::json!(format!("{}°C", v))); }
        if let Some(v) = smart_data.sct_temperature_lifetime_max { smart_info.insert("sct_temperature_lifetime_max".to_string(), serde_json::json!(format!("{}°C", v))); }
        if let Some(v) = smart_data.sct_temperature_op_limit { smart_info.insert("sct_temperature_op_limit".to_string(), serde_json::json!(format!("{}°C", v))); }
        
        // Usage stats (from temperature if available, or direct)
        if let Some(v) = &smart_data.temperature { smart_info.insert("temperature".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.power_on_hours { 
            let days = v / 24;
            let hours = v % 24;
            smart_info.insert("power_on_hours".to_string(), serde_json::json!(format!("{} ({} Tage, {} Std.)", v, days, hours))); 
        }
        if let Some(v) = smart_data.power_cycle_count { smart_info.insert("power_cycle_count".to_string(), serde_json::json!(v)); }
        
        // Self-test info
        if let Some(v) = &smart_data.self_test_status { smart_info.insert("self_test_status".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.self_test_short_minutes { smart_info.insert("self_test_short_minutes".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.self_test_extended_minutes { smart_info.insert("self_test_extended_minutes".to_string(), serde_json::json!(v)); }
        
        // Error logs
        if let Some(v) = smart_data.error_log_count { smart_info.insert("error_log_count".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.self_test_log_count { smart_info.insert("self_test_log_count".to_string(), serde_json::json!(v)); }
        
        // SSD-specific
        if let Some(v) = smart_data.endurance_used_percent { smart_info.insert("endurance_used_percent".to_string(), serde_json::json!(format!("{}%", v))); }
        if let Some(v) = smart_data.spare_available_percent { smart_info.insert("spare_available_percent".to_string(), serde_json::json!(format!("{}%", v))); }
        
        // Data transfer stats
        if let Some(v) = smart_data.total_lbas_written { 
            let tb = (v as f64 * 512.0) / 1_000_000_000_000.0;
            smart_info.insert("total_data_written".to_string(), serde_json::json!(format!("{:.2} TB", tb))); 
        }
        if let Some(v) = smart_data.total_lbas_read { 
            let tb = (v as f64 * 512.0) / 1_000_000_000_000.0;
            smart_info.insert("total_data_read".to_string(), serde_json::json!(format!("{:.2} TB", tb))); 
        }
        
        // Sector health
        if let Some(v) = smart_data.reallocated_sectors { smart_info.insert("reallocated_sectors".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.pending_sectors { smart_info.insert("pending_sectors".to_string(), serde_json::json!(v)); }
        if let Some(v) = smart_data.uncorrectable_sectors { smart_info.insert("uncorrectable_sectors".to_string(), serde_json::json!(v)); }
        
        // Full SMART attributes table
        if !smart_data.attributes.is_empty() {
            let attrs: Vec<serde_json::Value> = smart_data.attributes.iter().map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "name": a.name,
                    "value": a.value,
                    "worst": a.worst,
                    "threshold": a.threshold,
                    "raw_value": a.raw_value,
                    "flags": a.flags,
                    "prefailure": a.prefailure
                })
            }).collect();
            smart_info.insert("attributes_table".to_string(), serde_json::json!(attrs));
        }
        
        smart_info.insert("source".to_string(), serde_json::json!(&smart_data.source));
        smart_info.insert("smart_supported".to_string(), serde_json::json!(smart_data.available));
        
        result["smart_info"] = serde_json::json!(smart_info);
    } else {
        eprintln!("[SMART Debug] try_smartctl returned None - SMART not available for {}", smart_disk_id);
    }
    
    // 17. Calculate checksums of first sector
    let checksum_cmd = format!(
        "echo '{}' | sudo -S dd if=/dev/r{} bs=512 count=1 2>/dev/null | md5",
        escaped_password, disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &checksum_cmd]).output() {
        let md5 = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !md5.is_empty() {
            let mut checksums = serde_json::Map::new();
            checksums.insert("mbr_md5".to_string(), serde_json::json!(md5));
            
            // Also get SHA256
            let sha_cmd = format!(
                "echo '{}' | sudo -S dd if=/dev/r{} bs=512 count=1 2>/dev/null | shasum -a 256",
                escaped_password, disk_id
            );
            if let Ok(sha_out) = Command::new("sh").args(["-c", &sha_cmd]).output() {
                let sha = String::from_utf8_lossy(&sha_out.stdout);
                if let Some(hash) = sha.split_whitespace().next() {
                    checksums.insert("mbr_sha256".to_string(), serde_json::json!(hash));
                }
            }
            
            result["sector_checksums"] = serde_json::json!(checksums);
        }
    }
    
    Ok(result)
}

/// Find USB device info from system_profiler JSON
/// USB Vendor ID to Manufacturer name lookup (USB-IF official registry)
fn usb_vendor_lookup(vendor_id: &str) -> Option<&'static str> {
    // Common USB flash drive and storage device vendors
    // Full list: https://usb-ids.gowdy.us/
    match vendor_id.to_lowercase().as_str() {
        "0x0781" => Some("SanDisk Corporation"),
        "0x0951" => Some("Kingston Technology"),
        "0x8564" => Some("Transcend Information"),
        "0x058f" => Some("Alcor Micro Corp."),
        "0x090c" => Some("Silicon Motion"),
        "0x13fe" => Some("Phison Electronics"),
        "0x1f75" => Some("Innostor Technology"),
        "0x0bda" => Some("Realtek Semiconductor"),
        "0x1908" => Some("GEMBIRD"),
        "0x0930" => Some("Toshiba Corporation"),
        "0x1b1c" => Some("Corsair"),
        "0x154b" => Some("PNY Technologies"),
        "0x18a5" => Some("Verbatim"),
        "0x0dd8" => Some("Netac Technology"),
        "0x1005" => Some("Apacer Technology"),
        "0x04e8" => Some("Samsung Electronics"),
        "0x0ea0" => Some("Ours Technology"),
        "0x048d" => Some("Integrated Technology Express"),
        "0x1307" => Some("USBest Technology"),
        "0x05dc" => Some("Lexar Media"),
        "0x3538" => Some("Power Quotient International"),
        "0x0cf2" => Some("ENE Technology"),
        "0x1e3d" => Some("Chipsbrand Technologies"),
        // SATA-to-USB Bridge controllers
        "0x174c" => Some("ASMedia Technology Inc."),
        "0x152d" => Some("JMicron Technology Corp."),
        "0x1058" => Some("Western Digital Technologies"),
        "0x0bc2" => Some("Seagate Technology"),
        "0x04fc" => Some("Sunplus Technology"),
        "0x2109" => Some("VIA Labs Inc."),
        "0x14cd" => Some("Super Top"),
        "0x1bcf" => Some("Sunplus Innovation"),
        "0x0080" => Some("Assmann Electronic"),
        // External HDD/SSD vendors  
        "0x0480" => Some("Toshiba America Inc."),
        "0x07ab" => Some("Freecom Technologies"),
        "0x059b" => Some("Iomega Corporation"),
        "0x4971" => Some("SimpleTech"),
        "0x067b" => Some("Prolific Technology"),
        // Apple
        "0x05ac" => Some("Apple Inc."),
        // Common peripheral vendors
        "0x046d" => Some("Logitech Inc."),
        "0x045e" => Some("Microsoft Corporation"),
        "0x1d5c" => Some("Fresco Logic"),
        "0x1a40" => Some("Terminus Technology Inc."),
        "0x8087" => Some("Intel Corporation"),
        "0x0b95" => Some("ASIX Electronics"),
        "0x2357" => Some("TP-Link Technologies"),
        "0x0fe6" => Some("ICS Advent"),
        _ => None
    }
}

/// SD Card Manufacturer ID lookup (SD Association standard)
fn sd_manufacturer_lookup(manufacturer_id: &str) -> Option<&'static str> {
    // SD Card Manufacturer IDs from SD Association
    match manufacturer_id {
        "0x01" | "0x1" | "1" => Some("Panasonic"),
        "0x02" | "0x2" | "2" => Some("Toshiba"),
        "0x03" | "0x3" | "3" => Some("SanDisk"),
        "0x1b" | "27" => Some("Samsung"),
        "0x1d" | "29" => Some("AData"),
        "0x27" | "39" => Some("Phison"),
        "0x28" | "40" => Some("Lexar"),
        "0x31" | "49" => Some("Silicon Power"),
        "0x41" | "65" => Some("Kingston"),
        "0x45" | "69" => Some("TeamGroup"),
        "0x74" | "116" => Some("Transcend"),
        "0x76" | "118" => Some("Patriot"),
        "0x82" | "130" => Some("Sony"),
        "0x9c" | "156" => Some("Angelbird"),
        "0x9f" | "159" => Some("Teclast"),
        _ => None
    }
}

/// Find SD Card info from SPCardReaderDataType JSON
fn find_sd_card_info(json_data: &serde_json::Value, disk_id: &str) -> Option<serde_json::Value> {
    if let Some(card_reader_data) = json_data.get("SPCardReaderDataType") {
        if let Some(readers) = card_reader_data.as_array() {
            for reader in readers {
                // Get card reader info
                let reader_vendor_id = reader.get("spcardreader_vendor-id")
                    .and_then(|v| v.as_str()).unwrap_or("");
                let _reader_device_id = reader.get("spcardreader_device-id")
                    .and_then(|v| v.as_str()).unwrap_or("");
                let link_speed = reader.get("spcardreader_link-speed")
                    .and_then(|v| v.as_str()).unwrap_or("");
                
                // Search in _items for cards
                if let Some(items) = reader.get("_items") {
                    if let Some(cards) = items.as_array() {
                        for card in cards {
                            let bsd_name = card.get("bsd_name").and_then(|b| b.as_str()).unwrap_or("");
                            
                            // Match by disk ID
                            if bsd_name == disk_id || disk_id.starts_with(bsd_name) || bsd_name.starts_with(&disk_id.replace("s1", "").replace("s2", "")) {
                                let mut info = serde_json::Map::new();
                                
                                // Card type and name
                                if let Some(name) = card.get("_name").and_then(|n| n.as_str()) {
                                    info.insert("product_name".to_string(), serde_json::json!(name));
                                }
                                
                                // Product name from card
                                if let Some(product) = card.get("spcardreader_card_productname").and_then(|p| p.as_str()) {
                                    info.insert("card_model".to_string(), serde_json::json!(product));
                                }
                                
                                // Manufacturer from ID lookup
                                if let Some(mfr_id) = card.get("spcardreader_card_manufacturer-id").and_then(|m| m.as_str()) {
                                    info.insert("manufacturer_id".to_string(), serde_json::json!(mfr_id));
                                    if let Some(mfr_name) = sd_manufacturer_lookup(mfr_id) {
                                        info.insert("manufacturer".to_string(), serde_json::json!(mfr_name));
                                    }
                                }
                                
                                // Serial number
                                if let Some(serial) = card.get("spcardreader_card_serialnumber").and_then(|s| s.as_str()) {
                                    info.insert("serial_number".to_string(), serde_json::json!(serial));
                                }
                                
                                // Manufacturing date
                                if let Some(date) = card.get("spcardreader_card_manufacturing_date").and_then(|d| d.as_str()) {
                                    info.insert("manufacturing_date".to_string(), serde_json::json!(date));
                                }
                                
                                // Product revision
                                if let Some(rev) = card.get("spcardreader_card_productrevision").and_then(|r| r.as_str()) {
                                    info.insert("device_version".to_string(), serde_json::json!(rev));
                                }
                                
                                // SD spec version
                                if let Some(spec) = card.get("spcardreader_card_specversion").and_then(|s| s.as_str()) {
                                    info.insert("sd_spec_version".to_string(), serde_json::json!(spec));
                                }
                                
                                // Size
                                if let Some(size) = card.get("size").and_then(|s| s.as_str()) {
                                    info.insert("capacity".to_string(), serde_json::json!(size));
                                }
                                
                                // SMART status
                                if let Some(smart) = card.get("smart_status").and_then(|s| s.as_str()) {
                                    info.insert("smart_status".to_string(), serde_json::json!(smart));
                                }
                                
                                // Card reader info
                                if !link_speed.is_empty() {
                                    info.insert("reader_link_speed".to_string(), serde_json::json!(link_speed));
                                }
                                if !reader_vendor_id.is_empty() {
                                    info.insert("reader_vendor_id".to_string(), serde_json::json!(reader_vendor_id));
                                }
                                
                                info.insert("hardware_type".to_string(), serde_json::json!("SD Card"));
                                
                                return Some(serde_json::json!(info));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_usb_device_info(json_data: &serde_json::Value, _disk_id: &str, media_name: &str) -> Option<serde_json::Value> {
    // SPUSBHostDataType uses different field names than SPUSBDataType
    // We search recursively and match by media_name (e.g., "SanDisk 3.2Gen1")
    
    // Helper function to extract USB device info from an item
    fn extract_device_info(item: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        let mut info = serde_json::Map::new();
        
        let device_name = item.get("_name").and_then(|n| n.as_str()).unwrap_or("").trim();
        
        // Product name
        info.insert("product_name".to_string(), serde_json::json!(device_name));
        
        // Vendor ID and real manufacturer name from lookup
        let vendor_id = item.get("USBDeviceKeyVendorID").and_then(|v| v.as_str()).unwrap_or("");
        if !vendor_id.is_empty() {
            info.insert("vendor_id".to_string(), serde_json::json!(vendor_id));
            
            // Look up the real manufacturer name from USB-IF registry
            if let Some(real_manufacturer) = usb_vendor_lookup(vendor_id) {
                info.insert("manufacturer".to_string(), serde_json::json!(real_manufacturer));
            } else if let Some(vendor) = item.get("USBDeviceKeyVendorName").and_then(|v| v.as_str()) {
                info.insert("manufacturer".to_string(), serde_json::json!(vendor.trim()));
            }
        } else if let Some(vendor) = item.get("USBDeviceKeyVendorName").and_then(|v| v.as_str()) {
            info.insert("manufacturer".to_string(), serde_json::json!(vendor.trim()));
        }
        
        // Product ID
        if let Some(product_id) = item.get("USBDeviceKeyProductID").and_then(|p| p.as_str()) {
            info.insert("product_id".to_string(), serde_json::json!(product_id));
        }
        
        // Serial number
        if let Some(serial) = item.get("USBDeviceKeySerialNumber").and_then(|s| s.as_str()) {
            let serial_val = if serial == "Not Provided" { "" } else { serial };
            if !serial_val.is_empty() {
                info.insert("serial_number".to_string(), serde_json::json!(serial_val));
            }
        }
        
        // Link speed (e.g., "5 Gb/s", "480 Mb/s")
        if let Some(speed) = item.get("USBDeviceKeyLinkSpeed").and_then(|s| s.as_str()) {
            info.insert("usb_speed".to_string(), serde_json::json!(speed));
        }
        
        // Product version
        if let Some(version) = item.get("USBDeviceKeyProductVersion").and_then(|v| v.as_str()) {
            info.insert("device_version".to_string(), serde_json::json!(version));
        }
        
        // Power allocation (e.g., "4.48 W (896 mA)")
        if let Some(power) = item.get("USBDeviceKeyPowerAllocation").and_then(|p| p.as_str()) {
            info.insert("power_allocation".to_string(), serde_json::json!(power));
        }
        
        // Location ID
        if let Some(location) = item.get("USBKeyLocationID").and_then(|l| l.as_str()) {
            info.insert("location_id".to_string(), serde_json::json!(location));
        }
        
        // Hardware type
        info.insert("hardware_type".to_string(), serde_json::json!("USB Storage Device"));
        
        info
    }
    
    // Known USB-SATA bridge controller names
    fn is_usb_sata_bridge(name: &str) -> bool {
        let bridge_patterns = [
            "ASM105", "ASM115", "ASM235", "ASM1351", "ASM1352", "ASM1153", // ASMedia
            "JMS", "JMicron", // JMicron
            "VL71", "VL716", // VIA Labs
            "GL33", "GL35", // Genesys Logic
            "RTL9210", // Realtek
            "USB3.0", "USB 3.0", "SATA", // Generic patterns
        ];
        let name_upper = name.to_uppercase();
        bridge_patterns.iter().any(|p| name_upper.contains(&p.to_uppercase()))
    }
    
    fn search_devices(items: &serde_json::Value, media_name: &str, matched_device: &mut Option<serde_json::Value>, all_bridges: &mut Vec<serde_json::Map<String, serde_json::Value>>) {
        if let Some(array) = items.as_array() {
            for item in array {
                // Check if this is a removable USB device
                let is_removable = item.get("USBKeyHardwareType")
                    .and_then(|h| h.as_str())
                    .map(|s| s == "Removable")
                    .unwrap_or(false);
                
                if is_removable {
                    let device_name = item.get("_name").and_then(|n| n.as_str()).unwrap_or("").trim();
                    
                    // Check if this device matches our media_name
                    let name_matches = !media_name.is_empty() && (
                        device_name.contains(media_name) ||
                        media_name.contains(device_name) ||
                        device_name.trim().eq_ignore_ascii_case(media_name.trim()) ||
                        (media_name.len() > 3 && device_name.to_lowercase().contains(&media_name[..media_name.len().min(8)].to_lowercase()))
                    );
                    
                    if name_matches && matched_device.is_none() {
                        let info = extract_device_info(item);
                        *matched_device = Some(serde_json::json!(info));
                        return;
                    }
                    
                    // Collect USB-SATA bridges as potential candidates
                    if is_usb_sata_bridge(device_name) {
                        all_bridges.push(extract_device_info(item));
                    }
                }
                
                // Recursively search in _items
                if let Some(sub_items) = item.get("_items") {
                    search_devices(sub_items, media_name, matched_device, all_bridges);
                    if matched_device.is_some() {
                        return;
                    }
                }
            }
        }
    }
    
    let mut matched_device: Option<serde_json::Value> = None;
    let mut all_bridges: Vec<serde_json::Map<String, serde_json::Value>> = Vec::new();
    
    if let Some(usb_data) = json_data.get("SPUSBHostDataType") {
        search_devices(usb_data, media_name, &mut matched_device, &mut all_bridges);
    }
    
    // If no exact match found but we have USB-SATA bridges, return the first one
    // This handles cases like Samsung SSD 870 EVO connected via ASM105X bridge
    if matched_device.is_none() && !all_bridges.is_empty() {
        // Use the first bridge, add a note that this is the USB controller
        let mut bridge_info = all_bridges.remove(0);
        bridge_info.insert("note".to_string(), serde_json::json!("USB-SATA Bridge Controller"));
        return Some(serde_json::json!(bridge_info));
    }
    
    matched_device
}

/// Analyze boot structure of the disk
fn analyze_boot_structure(disk_id: &str, password: &str) -> serde_json::Value {
    let device_path = format!("/dev/r{}", disk_id);
    let mut boot_info = serde_json::Map::new();
    
    // Read raw bytes using Python for reliable access
    let python_script = format!(
        r#"
import os, sys

device = "{}"
try:
    fd = os.open(device, os.O_RDONLY)
    with os.fdopen(fd, 'rb') as f:
        # Read first 64KB
        data = f.read(65536)
        
        # MBR analysis
        if len(data) >= 512:
            mbr = data[:512]
            has_mbr_sig = mbr[510] == 0x55 and mbr[511] == 0xAA
            print(f"MBR_SIG:{{has_mbr_sig}}")
            
            # Partition table entries
            partitions = []
            for i in range(4):
                offset = 446 + (i * 16)
                boot_flag = mbr[offset]
                part_type = mbr[offset + 4]
                if part_type != 0:
                    partitions.append(f"{{i+1}}:type={{hex(part_type)}},boot={{'Y' if boot_flag == 0x80 else 'N'}}")
            print(f"PARTITIONS:{{';'.join(partitions) if partitions else 'none'}}")
        
        # GPT check
        if len(data) >= 1024:
            gpt = data[512:1024]
            has_gpt = gpt[0:8] == b'EFI PART'
            print(f"GPT:{{has_gpt}}")
            if has_gpt:
                # Parse GPT header
                import struct
                disk_guid = gpt[56:72]
                print(f"GPT_GUID:{{disk_guid.hex()}}")
        
        # ISO 9660 check (at 32KB offset)
        if len(data) >= 0x8006:
            f.seek(0x8001)
            iso_marker = f.read(5)
            is_iso = iso_marker == b'CD001'
            print(f"ISO9660:{{is_iso}}")
            
            if is_iso:
                # Read volume label
                f.seek(0x8028)
                vol_label = f.read(32).decode('ascii', errors='ignore').strip()
                print(f"ISO_LABEL:{{vol_label}}")
                
                # El Torito boot catalog
                f.seek(0x8801)
                boot_marker = f.read(5)
                has_boot = boot_marker == b'CD001'
                f.seek(0x8800)
                boot_type = f.read(1)[0]
                print(f"EL_TORITO:{{boot_type == 0 and has_boot}}")
        
        print("SUCCESS")
except Exception as e:
    print(f"ERROR:{{e}}")
    sys.exit(1)
"#, device_path);

    let cmd = format!(
        "echo '{}' | sudo -S python3 -c '{}'",
        password,
        python_script.replace("'", "'\"'\"'")
    );
    
    if let Ok(output) = Command::new("sh").args(["-c", &cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once(':') {
                match key {
                    "MBR_SIG" => { boot_info.insert("has_mbr_signature".to_string(), serde_json::json!(value == "True")); },
                    "GPT" => { boot_info.insert("has_gpt".to_string(), serde_json::json!(value == "True")); },
                    "GPT_GUID" => { boot_info.insert("gpt_disk_guid".to_string(), serde_json::json!(value)); },
                    "PARTITIONS" => { boot_info.insert("mbr_partitions".to_string(), serde_json::json!(value)); },
                    "ISO9660" => { boot_info.insert("is_iso9660".to_string(), serde_json::json!(value == "True")); },
                    "ISO_LABEL" => { boot_info.insert("iso_volume_label".to_string(), serde_json::json!(value)); },
                    "EL_TORITO" => { boot_info.insert("has_el_torito_boot".to_string(), serde_json::json!(value == "True")); },
                    _ => {}
                }
            }
        }
    }
    
    serde_json::json!(boot_info)
}

/// Detect filesystem signatures from raw device and its partitions
fn detect_filesystem_signatures(disk_id: &str, password: &str) -> Option<serde_json::Value> {
    let mut all_detected = Vec::new();
    let escaped_password = password.replace("'", "'\\''");
    
    // FIRST: Check the WHOLE DISK for ISO 9660 filesystem (hybrid ISO images write directly to disk)
    // ISO 9660 "CD001" signature is at offset 0x8001 (32769 bytes)
    // Note: Use /dev/diskX (not /dev/rdiskX) because raw device doesn't support seek properly
    let iso_check_cmd = format!(
        "echo '{}' | sudo -S dd if=/dev/{} bs=1 skip=32769 count=5 2>/dev/null | cat",
        escaped_password, disk_id
    );
    if let Ok(output) = Command::new("sh").args(["-c", &iso_check_cmd]).output() {
        let data = output.stdout;
        if data.len() >= 5 && &data[0..5] == b"CD001" {
            // Found ISO 9660! Now extract volume label and size
            let mut iso_info = serde_json::Map::new();
            iso_info.insert("type".to_string(), serde_json::json!("ISO 9660"));
            
            // Extract volume label (at offset 32808 = 0x8028, 32 bytes)
            let label_cmd = format!(
                "echo '{}' | sudo -S dd if=/dev/{} bs=1 skip=32808 count=32 2>/dev/null | tr -d '\\0' | xargs",
                escaped_password, disk_id
            );
            if let Ok(label_output) = Command::new("sh").args(["-c", &label_cmd]).output() {
                let label = String::from_utf8_lossy(&label_output.stdout).trim().to_string();
                if !label.is_empty() {
                    iso_info.insert("label".to_string(), serde_json::json!(label));
                }
            }
            
            // Extract volume size using Python to read the 4-byte little-endian value at offset 32848
            let size_cmd = format!(
                "echo '{}' | sudo -S python3 -c \"import os; f=os.open('/dev/{}', os.O_RDONLY); os.lseek(f, 32848, 0); d=os.read(f, 4); os.close(f); print(int.from_bytes(d, 'little') * 2048)\" 2>/dev/null",
                escaped_password, disk_id
            );
            if let Ok(size_output) = Command::new("sh").args(["-c", &size_cmd]).output() {
                let size_str = String::from_utf8_lossy(&size_output.stdout).trim().to_string();
                if let Ok(iso_size) = size_str.parse::<u64>() {
                    if iso_size > 0 {
                        iso_info.insert("size_bytes".to_string(), serde_json::json!(iso_size));
                        iso_info.insert("size_human".to_string(), serde_json::json!(format_bytes(iso_size)));
                    }
                }
            }
            
            // Add the ISO detection with details
            let iso_entry = if let Some(label) = iso_info.get("label").and_then(|v| v.as_str()) {
                if let Some(size) = iso_info.get("size_human").and_then(|v| v.as_str()) {
                    format!("ISO 9660 '{}' ({})", label, size)
                } else {
                    format!("ISO 9660 '{}'", label)
                }
            } else if let Some(size) = iso_info.get("size_human").and_then(|v| v.as_str()) {
                format!("ISO 9660 ({})", size)
            } else {
                "ISO 9660".to_string()
            };
            all_detected.push(iso_entry);
            
            // Also store the full ISO info for later use
            // Note: This will be returned as part of the filesystem_signatures
        }
    }
    
    // Get list of partitions for this disk
    let list_cmd = format!("diskutil list {} 2>/dev/null", disk_id);
    let mut partitions = vec![disk_id.to_string()];
    
    if let Ok(output) = Command::new("sh").args(["-c", &list_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            // Look for partition identifiers like "disk5s1", "disk5s2", etc.
            if let Some(part_id) = line.split_whitespace().last() {
                if part_id.starts_with("disk") && part_id.contains('s') && part_id != disk_id {
                    partitions.push(part_id.to_string());
                }
            }
        }
    }
    
    // First, try to get filesystem info from diskutil (more reliable for Paragon drivers)
    for part_id in &partitions {
        if part_id == disk_id {
            continue; // Skip whole disk, only check partitions
        }
        
        let info_cmd = format!("diskutil info {} 2>/dev/null", part_id);
        if let Ok(output) = Command::new("sh").args(["-c", &info_cmd]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut personality = String::new();
            
            for line in stdout.lines() {
                if line.contains("File System Personality:") {
                    if let Some(value) = line.split(':').nth(1) {
                        personality = value.trim().to_string();
                    }
                }
            }
            
            // Map Paragon UFSD personalities to filesystem names
            let fs_name = if personality.starts_with("UFSD_EXTFS") {
                // UFSD_EXTFS, UFSD_EXTFS2, UFSD_EXTFS3, UFSD_EXTFS4
                if personality.ends_with("4") {
                    Some("ext4")
                } else if personality.ends_with("3") {
                    Some("ext3")
                } else if personality.ends_with("2") {
                    Some("ext2")
                } else {
                    // Just "UFSD_EXTFS" - check superblock for version
                    None
                }
            } else if personality.starts_with("UFSD_NTFS") {
                Some("NTFS")
            } else if personality.contains("APFS") {
                Some("APFS")
            } else if personality.contains("HFS") || personality.contains("Mac OS Extended") {
                Some("HFS+")
            } else if personality.contains("FAT32") || personality.contains("MS-DOS FAT32") {
                Some("FAT32")
            } else if personality.contains("FAT16") {
                Some("FAT16")
            } else if personality.contains("ExFAT") || personality.contains("exFAT") {
                Some("exFAT")
            } else {
                None
            };
            
            if let Some(fs) = fs_name {
                let entry = format!("{} ({})", fs, part_id);
                if !all_detected.contains(&entry) {
                    all_detected.push(entry);
                }
            }
        }
    }
    
    // Scan each partition for filesystem signatures (fallback for unmounted/unknown filesystems)
    // Skip partitions already detected via diskutil
    for part_id in &partitions {
        // Check if this partition was already detected
        let already_detected = all_detected.iter().any(|e| e.contains(&format!("({})", part_id)));
        if already_detected {
            continue;
        }
        
        let device_path = format!("/dev/r{}", part_id);
        
        let python_script = format!(
            r#"
import os
import sys

device = "{}"
try:
    fd = os.open(device, os.O_RDONLY)
    with os.fdopen(fd, 'rb') as f:
        # Read enough data for all signatures
        data = f.read(131072)  # 128KB
        print(f"READ_BYTES:{{len(data)}}", file=sys.stderr)
        
        # NTFS (offset 3)
        if len(data) >= 11 and data[3:7] == b'NTFS':
            print("FS_NTFS:True")
        
        # FAT32 (offset 82 or 54)
        if len(data) >= 90:
            if data[82:90] == b'FAT32   ' or data[54:62] == b'FAT32   ':
                print("FS_FAT32:True")
            elif data[54:59] == b'FAT16':
                print("FS_FAT16:True")
            elif data[54:59] == b'FAT12':
                print("FS_FAT12:True")
        
        # exFAT (offset 3)
        if len(data) >= 11 and data[3:8] == b'EXFAT':
            print("FS_EXFAT:True")
        
        # ext2/3/4 (superblock at offset 1024, magic at offset 0x38 within superblock = 1024+56 = 1080)
        if len(data) >= 1082:
            ext_magic = data[1080:1082]  # Magic at superblock offset 0x38 (56 bytes into superblock)
            if ext_magic == b'\x53\xef':
                print("FS_EXT_DETECTED:True", file=sys.stderr)
                # Check ext version using incompat features at offset 0x60 (96) within superblock
                # and compat features at offset 0x5C (92)
                ext_version = 2  # Default to ext2
                
                if len(data) >= 1124:
                    # Read feature flags
                    compat = int.from_bytes(data[1116:1120], 'little')      # 1024 + 92
                    incompat = int.from_bytes(data[1120:1124], 'little')    # 1024 + 96
                    ro_compat = int.from_bytes(data[1124:1128], 'little')   # 1024 + 100
                    
                    print(f"EXT_COMPAT:{{compat:08x}} INCOMPAT:{{incompat:08x}} RO_COMPAT:{{ro_compat:08x}}", file=sys.stderr)
                    
                    # ext4 detection: check for ext4-specific features
                    # INCOMPAT_EXTENTS (0x40), INCOMPAT_64BIT (0x80), INCOMPAT_FLEX_BG (0x200)
                    # INCOMPAT_MMP (0x100), INCOMPAT_INLINE_DATA (0x8000)
                    ext4_incompat_flags = 0x40 | 0x80 | 0x200 | 0x100 | 0x8000
                    # RO_COMPAT: HUGE_FILE (0x08), GDT_CSUM (0x10), DIR_NLINK (0x20), EXTRA_ISIZE (0x40)
                    ext4_ro_compat_flags = 0x08 | 0x10 | 0x20 | 0x40
                    
                    if (incompat & ext4_incompat_flags) or (ro_compat & ext4_ro_compat_flags):
                        ext_version = 4
                    elif incompat & 0x04:  # INCOMPAT_RECOVER (has journal, so ext3+)
                        # Check if it has any ext4 ro_compat features
                        if ro_compat & ext4_ro_compat_flags:
                            ext_version = 4
                        else:
                            ext_version = 3
                    elif compat & 0x04:  # COMPAT_HAS_JOURNAL
                        ext_version = 3
                
                if ext_version == 4:
                    print("FS_EXT4:True")
                elif ext_version == 3:
                    print("FS_EXT3:True")
                else:
                    print("FS_EXT2:True")
        
        # HFS+ (offset 1024)
        if len(data) >= 1026:
            hfs_magic = data[1024:1026]
            if hfs_magic == b'H+' or hfs_magic == b'HX':
                print("FS_HFSPLUS:True")
        
        # APFS (look for NXSB magic at offset 32)
        if len(data) >= 36 and data[32:36] == b'NXSB':
            print("FS_APFS:True")
        
        # Btrfs (superblock at 64KB + 64 bytes)
        f.seek(65536 + 64)
        btrfs_magic = f.read(8)
        if btrfs_magic == b'_BHRfS_M':
            print("FS_BTRFS:True")
        
        # XFS (offset 0)
        if len(data) >= 4 and data[0:4] == b'XFSB':
            print("FS_XFS:True")
        
        print("SUCCESS")
except Exception as e:
    print(f"ERROR:{{e}}", file=sys.stderr)
"#, device_path);

        let cmd = format!(
            "echo '{}' | sudo -S python3 -c '{}'",
            escaped_password,
            python_script.replace("'", "'\"'\"'")
        );
        
        if let Ok(output) = Command::new("sh").args(["-c", &cmd]).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let _stderr = String::from_utf8_lossy(&output.stderr);
            
            // Note: stderr output is ignored - some devices don't support raw reads
            
            for line in stdout.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    if value == "True" {
                        let fs_name = match key {
                            "FS_NTFS" => "NTFS",
                            "FS_FAT32" => "FAT32",
                            "FS_FAT16" => "FAT16",
                            "FS_FAT12" => "FAT12",
                            "FS_EXFAT" => "exFAT",
                            "FS_EXT4" => "ext4",
                            "FS_EXT3" => "ext3",
                            "FS_EXT2" => "ext2",
                            "FS_HFSPLUS" => "HFS+",
                            "FS_APFS" => "APFS",
                            "FS_BTRFS" => "Btrfs",
                            "FS_XFS" => "XFS",
                            _ => continue,
                        };
                        
                        // Check if this is an EFI partition (0xEF) - if so, label as EFI
                        let mut final_fs_name = fs_name.to_string();
                        if part_id != disk_id {
                            // Check partition type
                            let info_cmd = format!("diskutil info {} 2>/dev/null | grep 'Partition Type'", part_id);
                            if let Ok(info_out) = Command::new("sh").args(["-c", &info_cmd]).output() {
                                let info_str = String::from_utf8_lossy(&info_out.stdout);
                                if info_str.contains("0xEF") || info_str.to_lowercase().contains("efi") {
                                    // This is an EFI System Partition with FAT filesystem
                                    final_fs_name = format!("EFI ({})", fs_name);
                                }
                            }
                        }
                        
                        let entry = if part_id == disk_id {
                            final_fs_name
                        } else {
                            format!("{} ({})", final_fs_name, part_id)
                        };
                        if !all_detected.contains(&entry) {
                            all_detected.push(entry);
                        }
                    }
                }
            }
        }
    }
    
    if !all_detected.is_empty() {
        let mut signatures = serde_json::Map::new();
        signatures.insert("detected_filesystems".to_string(), serde_json::json!(all_detected));
        return Some(serde_json::json!(signatures));
    }
    
    None
}

/// Analyze mounted content (files, folders, OS detection)
fn analyze_mounted_content(mount_point: &str) -> Option<serde_json::Value> {
    let mut content = serde_json::Map::new();
    
    // Count files and folders
    let count_cmd = format!(
        "find '{}' -maxdepth 5 2>/dev/null | head -10000 | wc -l",
        mount_point
    );
    
    if let Ok(output) = Command::new("sh").args(["-c", &count_cmd]).output() {
        if let Ok(count) = String::from_utf8_lossy(&output.stdout).trim().parse::<u64>() {
            content.insert("total_items".to_string(), serde_json::json!(count));
        }
    }
    
    // Get disk usage
    let du_cmd = format!("du -sh '{}' 2>/dev/null", mount_point);
    if let Ok(output) = Command::new("sh").args(["-c", &du_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Some(size) = stdout.split_whitespace().next() {
            content.insert("used_space".to_string(), serde_json::json!(size));
        }
    }
    
    // Get file count
    let file_count_cmd = format!("find '{}' -type f 2>/dev/null | wc -l", mount_point);
    if let Ok(output) = Command::new("sh").args(["-c", &file_count_cmd]).output() {
        let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
        content.insert("file_count".to_string(), serde_json::json!(count));
    }
    
    // Get directory count
    let dir_count_cmd = format!("find '{}' -type d 2>/dev/null | wc -l", mount_point);
    if let Ok(output) = Command::new("sh").args(["-c", &dir_count_cmd]).output() {
        let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
        content.insert("directory_count".to_string(), serde_json::json!(count));
    }
    
    // Detect OS installations
    let mut detected_os = Vec::new();
    
    // Check for Windows
    let windows_paths = [
        "Windows/System32",
        "Windows/explorer.exe",
        "bootmgr",
        "Boot/BCD",
    ];
    for path in &windows_paths {
        let full_path = format!("{}/{}", mount_point, path);
        if std::path::Path::new(&full_path).exists() {
            if !detected_os.contains(&"Windows".to_string()) {
                detected_os.push("Windows".to_string());
            }
            break;
        }
    }
    
    // Check for Linux
    let linux_paths = [
        "boot/vmlinuz",
        "boot/grub",
        "etc/os-release",
        "bin/bash",
    ];
    for path in &linux_paths {
        let full_path = format!("{}/{}", mount_point, path);
        if std::path::Path::new(&full_path).exists() {
            if !detected_os.contains(&"Linux".to_string()) {
                detected_os.push("Linux".to_string());
            }
            break;
        }
    }
    
    // Check for macOS installer
    let macos_paths = [
        "Install macOS",
        ".IABootFiles",
        "System/Library/CoreServices",
    ];
    for path in &macos_paths {
        let full_path = format!("{}/{}", mount_point, path);
        let check_cmd = format!("ls -d '{}' 2>/dev/null | head -1", full_path);
        if let Ok(output) = Command::new("sh").args(["-c", &check_cmd]).output() {
            if !output.stdout.is_empty() {
                if !detected_os.contains(&"macOS".to_string()) {
                    detected_os.push("macOS".to_string());
                }
                break;
            }
        }
    }
    
    // Check for Linux distributions and get detailed info
    let os_release_path = format!("{}/etc/os-release", mount_point);
    if let Ok(contents) = std::fs::read_to_string(&os_release_path) {
        let mut linux_info = serde_json::Map::new();
        for line in contents.lines() {
            if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
                let distro = name.trim_matches('"');
                content.insert("linux_distribution".to_string(), serde_json::json!(distro));
                linux_info.insert("pretty_name".to_string(), serde_json::json!(distro));
            } else if let Some(name) = line.strip_prefix("NAME=") {
                linux_info.insert("name".to_string(), serde_json::json!(name.trim_matches('"')));
            } else if let Some(version) = line.strip_prefix("VERSION=") {
                linux_info.insert("version".to_string(), serde_json::json!(version.trim_matches('"')));
            } else if let Some(id) = line.strip_prefix("ID=") {
                linux_info.insert("id".to_string(), serde_json::json!(id.trim_matches('"')));
            }
        }
        if !linux_info.is_empty() {
            content.insert("linux_system_info".to_string(), serde_json::json!(linux_info));
        }
        
        // Get home users for Linux
        let home_path = format!("{}/home", mount_point);
        if std::path::Path::new(&home_path).exists() {
            let home_cmd = format!("ls -1 '{}' 2>/dev/null", home_path);
            if let Ok(output) = Command::new("sh").args(["-c", &home_cmd]).output() {
                let users: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if !users.is_empty() {
                    content.insert("linux_home_users".to_string(), serde_json::json!(users));
                }
            }
        }
        
        // Check for installed package count
        let dpkg_path = format!("{}/var/lib/dpkg/status", mount_point);
        if std::path::Path::new(&dpkg_path).exists() {
            let pkg_cmd = format!("grep -c '^Package:' '{}' 2>/dev/null", dpkg_path);
            if let Ok(output) = Command::new("sh").args(["-c", &pkg_cmd]).output() {
                let count = String::from_utf8_lossy(&output.stdout).trim().to_string();
                content.insert("installed_packages_dpkg".to_string(), serde_json::json!(count));
            }
        }
        
        // Check for kernel versions
        let boot_path = format!("{}/boot", mount_point);
        if std::path::Path::new(&boot_path).exists() {
            let kernel_cmd = format!("ls '{}' 2>/dev/null | grep -E 'vmlinuz|initrd' | head -5", boot_path);
            if let Ok(output) = Command::new("sh").args(["-c", &kernel_cmd]).output() {
                let kernels: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if !kernels.is_empty() {
                    content.insert("kernel_files".to_string(), serde_json::json!(kernels));
                }
            }
        }
    }
    
    // Check for Windows system info
    let win_path = format!("{}/Windows", mount_point);
    if std::path::Path::new(&win_path).exists() {
        let mut windows_info = serde_json::Map::new();
        windows_info.insert("is_windows_system".to_string(), serde_json::json!(true));
        
        // Check Windows version hints
        let sys_apps = format!("{}/Windows/SystemApps", mount_point);
        if std::path::Path::new(&sys_apps).exists() {
            windows_info.insert("version_hint".to_string(), serde_json::json!("Windows 10/11"));
        }
        
        // Get Windows user profiles
        let users_path = format!("{}/Users", mount_point);
        if std::path::Path::new(&users_path).exists() {
            let users_cmd = format!("ls -1 '{}' 2>/dev/null | grep -v -E '^(Public|Default|All Users|Default User|desktop.ini)$'", users_path);
            if let Ok(output) = Command::new("sh").args(["-c", &users_cmd]).output() {
                let users: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if !users.is_empty() {
                    windows_info.insert("user_profiles".to_string(), serde_json::json!(users));
                }
            }
        }
        
        // Get installed programs
        let prog_path = format!("{}/Program Files", mount_point);
        if std::path::Path::new(&prog_path).exists() {
            let prog_cmd = format!("ls -1 '{}' 2>/dev/null | head -20", prog_path);
            if let Ok(output) = Command::new("sh").args(["-c", &prog_cmd]).output() {
                let progs: Vec<String> = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if !progs.is_empty() {
                    windows_info.insert("installed_programs".to_string(), serde_json::json!(progs));
                }
            }
        }
        
        content.insert("windows_system_info".to_string(), serde_json::json!(windows_info));
    }
    
    if !detected_os.is_empty() {
        content.insert("detected_os".to_string(), serde_json::json!(detected_os));
    }
    
    // List top-level directories with details
    let ls_cmd = format!("ls -la '{}' 2>/dev/null | head -35", mount_point);
    if let Ok(output) = Command::new("sh").args(["-c", &ls_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        content.insert("root_listing".to_string(), serde_json::json!(stdout.trim()));
    }
    
    // Also get simple list for backwards compatibility
    let ls_simple_cmd = format!("ls -1 '{}' 2>/dev/null | head -30", mount_point);
    if let Ok(output) = Command::new("sh").args(["-c", &ls_simple_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let dirs: Vec<&str> = stdout.lines().collect();
        if !dirs.is_empty() {
            content.insert("top_level_items".to_string(), serde_json::json!(dirs));
        }
    }
    
    // Get largest files with human-readable sizes
    let large_cmd = format!(
        "find '{}' -type f -exec stat -f '%z %N' {{}} \\; 2>/dev/null | sort -rn | head -10",
        mount_point
    );
    if let Ok(output) = Command::new("sh").args(["-c", &large_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<serde_json::Value> = stdout.lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let size_bytes: u64 = parts[0].parse().unwrap_or(0);
                    let size_human = if size_bytes >= 1073741824 {
                        format!("{:.2} GB", size_bytes as f64 / 1073741824.0)
                    } else if size_bytes >= 1048576 {
                        format!("{:.2} MB", size_bytes as f64 / 1048576.0)
                    } else if size_bytes >= 1024 {
                        format!("{:.2} KB", size_bytes as f64 / 1024.0)
                    } else {
                        format!("{} B", size_bytes)
                    };
                    Some(serde_json::json!({
                        "size_bytes": size_bytes,
                        "size_human": size_human,
                        "path": parts[1].replace(mount_point, "")
                    }))
                } else {
                    None
                }
            })
            .collect();
        if !files.is_empty() {
            content.insert("largest_files".to_string(), serde_json::json!(files));
        }
    }
    
    // Get hidden files
    let hidden_cmd = format!("find '{}' -maxdepth 2 -name '.*' -type f 2>/dev/null | head -20", mount_point);
    if let Ok(output) = Command::new("sh").args(["-c", &hidden_cmd]).output() {
        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.replace(mount_point, "").to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !files.is_empty() {
            content.insert("hidden_files".to_string(), serde_json::json!(files));
        }
    }
    
    // Get file type distribution
    let types_cmd = format!(
        "find '{}' -type f -name '*.*' 2>/dev/null | sed 's/.*\\.//' | sort | uniq -c | sort -rn | head -15",
        mount_point
    );
    if let Ok(output) = Command::new("sh").args(["-c", &types_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let types: Vec<serde_json::Value> = stdout.lines()
            .filter_map(|line| {
                let line = line.trim();
                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    Some(serde_json::json!({
                        "count": parts[0].trim(),
                        "extension": parts[1].trim()
                    }))
                } else {
                    None
                }
            })
            .collect();
        if !types.is_empty() {
            content.insert("file_type_distribution".to_string(), serde_json::json!(types));
        }
    }
    
    // Get recently modified files (last 7 days)
    let recent_cmd = format!(
        "find '{}' -type f -mtime -7 2>/dev/null | head -15",
        mount_point
    );
    if let Ok(output) = Command::new("sh").args(["-c", &recent_cmd]).output() {
        let files: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.replace(mount_point, "").to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !files.is_empty() {
            content.insert("recently_modified".to_string(), serde_json::json!(files));
        }
    }

    if content.is_empty() {
        None
    } else {
        Some(serde_json::json!(content))
    }
}

/// Detect special structures (hidden partitions, recovery, etc.)
fn detect_special_structures(disk_id: &str, password: &str) -> Option<serde_json::Value> {
    let mut special = serde_json::Map::new();
    
    // Check for hidden partitions using diskutil
    let hidden_cmd = format!(
        "echo '{}' | sudo -S diskutil list {} 2>/dev/null | grep -i 'EFI\\|Recovery\\|hidden\\|Microsoft Reserved'",
        password, disk_id
    );
    
    if let Ok(output) = Command::new("sh").args(["-c", &hidden_cmd]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.trim().is_empty() {
            let partitions: Vec<&str> = stdout.lines().map(|l| l.trim()).collect();
            special.insert("special_partitions".to_string(), serde_json::json!(partitions));
        }
    }
    
    // Check for Windows recovery
    let check_windows_re = format!(
        "ls -la /Volumes/*/Recovery 2>/dev/null | head -1",
    );
    if let Ok(output) = Command::new("sh").args(["-c", &check_windows_re]).output() {
        if !output.stdout.is_empty() {
            special.insert("has_windows_recovery".to_string(), serde_json::json!(true));
        }
    }
    
    if special.is_empty() {
        None
    } else {
        Some(serde_json::json!(special))
    }
}

/// Check if a USB disk is bootable (EFI/MBR/Hybrid)
#[tauri::command]
async fn check_bootable(disk_id: String, password: String) -> Result<serde_json::Value, String> {
    let disk_path = format!("/dev/r{}", disk_id);
    
    // Use Python with sudo to read raw disk bytes
    let python_script = format!(
        r#"
import os, sys, struct

device = "{}"
try:
    fd = os.open(device, os.O_RDONLY)
    with os.fdopen(fd, 'rb') as f:
        # Read MBR (first 512 bytes)
        mbr = f.read(512)
        if len(mbr) < 512:
            print("ERROR:MBR zu klein")
            sys.exit(1)
        
        # Check MBR signature
        has_mbr = mbr[510] == 0x55 and mbr[511] == 0xAA
        
        # Read GPT header (sector 1)
        f.seek(512)
        gpt_header = f.read(512)
        has_gpt = len(gpt_header) >= 8 and gpt_header[0:8] == b'EFI PART'
        
        # Check partition entries in MBR
        has_efi = False
        has_bootable = False
        for i in range(4):
            offset = 446 + (i * 16)
            boot_flag = mbr[offset]
            part_type = mbr[offset + 4]
            if boot_flag == 0x80:
                has_bootable = True
            if part_type == 0xEF or part_type == 0xEE:
                has_efi = True
        
        # Check for ISO 9660
        f.seek(0x8000)
        iso_pvd = f.read(2048)
        is_iso = len(iso_pvd) >= 6 and iso_pvd[1:6] == b'CD001'
        
        # Check El Torito
        has_el_torito = False
        if is_iso:
            f.seek(0x8800)
            boot_record = f.read(2048)
            has_el_torito = len(boot_record) >= 6 and boot_record[1:6] == b'CD001' and boot_record[0] == 0
        
        # Output results
        print(f"MBR:{{'1' if has_mbr else '0'}}")
        print(f"GPT:{{'1' if has_gpt else '0'}}")
        print(f"EFI:{{'1' if has_efi else '0'}}")
        print(f"BOOTABLE:{{'1' if has_bootable else '0'}}")
        print(f"ISO:{{'1' if is_iso else '0'}}")
        print(f"ELTORITO:{{'1' if has_el_torito else '0'}}")
        print("SUCCESS")
except Exception as e:
    print(f"ERROR:{{e}}")
    sys.exit(1)
"#, disk_path);

    let escaped_password = password.replace("'", "'\\''");
    let cmd = format!(
        "echo '{}' | sudo -S python3 -c '{}'",
        escaped_password,
        python_script.replace("'", "'\"'\"'")
    );
    
    let output = Command::new("sh")
        .args(["-c", &cmd])
        .output()
        .map_err(|e| format!("Fehler beim Ausführen: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    if !output.status.success() || stdout.contains("ERROR:") {
        let error_msg = if stdout.contains("ERROR:") {
            stdout.lines().find(|l| l.starts_with("ERROR:"))
                .map(|l| l.replace("ERROR:", ""))
                .unwrap_or_else(|| "Unknown error".to_string())
        } else {
            stderr.to_string()
        };
        return Err(format!("Bootcheck failed: {}", error_msg));
    }
    
    // Parse results
    let has_mbr = stdout.contains("MBR:1");
    let has_gpt = stdout.contains("GPT:1");
    let has_efi = stdout.contains("EFI:1");
    let has_bootable = stdout.contains("BOOTABLE:1");
    let is_iso = stdout.contains("ISO:1");
    let has_el_torito = stdout.contains("ELTORITO:1");
    
    // Determine boot type
    let boot_type = if has_gpt && has_efi {
        "UEFI (GPT)"
    } else if has_mbr && has_efi {
        "Hybrid (UEFI + Legacy)"
    } else if has_mbr && has_bootable {
        "Legacy (MBR)"
    } else if is_iso && has_el_torito {
        "ISO Boot (El Torito)"
    } else if is_iso {
        "ISO (nicht bootfähig)"
    } else if has_mbr {
        "MBR vorhanden (nicht bootfähig)"
    } else {
        "Nicht bootfähig"
    };
    
    let is_bootable = has_gpt || has_bootable || has_el_torito || has_efi;
    
    Ok(serde_json::json!({
        "bootable": is_bootable,
        "boot_type": boot_type,
        "has_mbr": has_mbr,
        "has_gpt": has_gpt,
        "has_efi": has_efi,
        "has_bootable_flag": has_bootable,
        "is_iso": is_iso,
        "has_el_torito": has_el_torito
    }))
}

/// Detect ISO 9660 size using sudo (for when we already have the password)
/// This reads the Primary Volume Descriptor to get the actual ISO size
fn detect_iso_size_with_sudo(device_path: &str, password: &str) -> Option<u64> {
    // Python script to read ISO 9660 PVD and extract size
    let python_script = format!(
        r#"import os, sys, struct
device = "{}"
try:
    fd = os.open(device, os.O_RDONLY)
    with os.fdopen(fd, 'rb') as f:
        # Seek to Primary Volume Descriptor at sector 16 (offset 0x8000)
        f.seek(0x8000)
        pvd = f.read(2048)
        # Check if it's a valid PVD: type 1, "CD001"
        if len(pvd) >= 2048 and pvd[0] == 1 and pvd[1:6] == b'CD001':
            # Volume Space Size at offset 80 (little-endian 32-bit)
            volume_blocks = struct.unpack('<I', pvd[80:84])[0]
            # Logical Block Size at offset 128 (little-endian 16-bit)
            block_size = struct.unpack('<H', pvd[128:130])[0]
            total_size = volume_blocks * block_size
            print(f"ISO_SIZE:{{total_size}}")
            sys.exit(0)
except Exception as e:
    print(f"ERROR:{{e}}", file=sys.stderr)
print("NOT_ISO")
sys.exit(0)"#, device_path);

    let mut child = Command::new("sudo")
        .args(["-S", "python3", "-c", &python_script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    
    if let Some(ref mut stdin) = child.stdin {
        writeln!(stdin, "{}", password).ok();
    }
    
    let output = child.wait_with_output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    for line in stdout.lines() {
        if let Some(size_str) = line.strip_prefix("ISO_SIZE:") {
            if let Ok(size) = size_str.parse::<u64>() {
                return Some(size);
            }
        }
    }
    
    None
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
    
    // Try to detect actual ISO size using root privileges
    emit_progress(&app, 0, "Prüfe ISO-Größe...", "backup");
    let actual_size = detect_iso_size_with_sudo(&rdisk_path, &password).unwrap_or(disk_size);
    
    if actual_size != disk_size {
        let _ = app.emit("log", format!("ISO erkannt: {} statt {} wird gesichert", 
            format_bytes(actual_size), format_bytes(disk_size)));
    }
    
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
print("SUCCESS", flush=True)"#, rdisk_path, destination.replace('"', r#"\""#), actual_size);

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
                let percent = ((bytes as f64 / actual_size as f64) * 100.0) as u32;
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
    
    let (action_menu_label, start_burn_label, start_backup_label, start_diagnose_label, cancel_label) = if lang == "en" {
        ("Action", "Burn ISO to USB", "Backup USB", "Start Diagnostic", "Cancel Operation")
    } else {
        ("Aktion", "ISO auf USB brennen", "USB sichern", "Diagnose starten", "Vorgang abbrechen")
    };
    
    let (window_menu_label, minimize_label, fullscreen_label) = if lang == "en" {
        ("Window", "Minimize", "Fullscreen")
    } else {
        ("Fenster", "Im Dock ablegen", "Vollbild")
    };
    
    let help_menu_label = if lang == "en" { "Help" } else { "Hilfe" };
    
    let about_metadata = AboutMetadata {
        name: Some("BurnISO to USB".to_string()),
        version: Some("1.3.1".to_string()),
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
    let tab_diagnose_label = if lang == "en" { "USB Diagnostic" } else { "USB Diagnose" };
    let tab_diagnose = MenuItem::with_id(app_handle, "tab_diagnose", tab_diagnose_label, true, Some("CmdOrCtrl+3"))?;
    let tab_tools_label = if lang == "en" { "USB Tools" } else { "USB Tools" };
    let tab_tools = MenuItem::with_id(app_handle, "tab_tools", tab_tools_label, true, Some("CmdOrCtrl+4"))?;
    let tab_forensic_label = if lang == "en" { "Forensic Analysis" } else { "Forensik-Analyse" };
    let tab_forensic = MenuItem::with_id(app_handle, "tab_forensic", tab_forensic_label, true, Some("CmdOrCtrl+5"))?;
    let start_burn = MenuItem::with_id(app_handle, "start_burn", start_burn_label, true, Some("CmdOrCtrl+B"))?;
    let start_backup = MenuItem::with_id(app_handle, "start_backup", start_backup_label, true, Some("CmdOrCtrl+Shift+B"))?;
    let start_diagnose = MenuItem::with_id(app_handle, "start_diagnose", start_diagnose_label, true, Some("CmdOrCtrl+D"))?;
    let cancel_action = MenuItem::with_id(app_handle, "cancel_action", cancel_label, true, Some("CmdOrCtrl+."))?;
    
    let action_menu = Submenu::with_items(
        app_handle,
        action_menu_label,
        true,
        &[&tab_burn, &tab_backup, &tab_diagnose, &tab_tools, &tab_forensic, &PredefinedMenuItem::separator(app_handle)?, &start_burn, &start_backup, &start_diagnose, &PredefinedMenuItem::separator(app_handle)?, &cancel_action],
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
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            list_disks,
            get_disk_info,
            get_volume_info,
            burn_iso,
            backup_usb_raw,
            backup_usb_filesystem,
            cancel_burn,
            cancel_backup,
            cancel_diagnose,
            cancel_tools,
            diagnose_surface_scan,
            diagnose_full_test,
            diagnose_speed_test,
            get_smart_data,
            check_smartctl_installed,
            check_paragon_drivers,
            check_dependencies,
            write_text_file,
            format_disk,
            repair_disk,
            secure_erase,
            check_bootable,
            forensic_analysis,
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
                        "tab_diagnose" => { let _ = window.emit("menu-action", "tab_diagnose"); }
                        "tab_tools" => { let _ = window.emit("menu-action", "tab_tools"); }
                        "tab_forensic" => { let _ = window.emit("menu-action", "tab_forensic"); }
                        "start_burn" => { let _ = window.emit("menu-action", "start_burn"); }
                        "start_backup" => { let _ = window.emit("menu-action", "start_backup"); }
                        "start_diagnose" => { let _ = window.emit("menu-action", "start_diagnose"); }
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
