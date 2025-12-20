// Wait for Tauri to be ready
document.addEventListener('DOMContentLoaded', async () => {
  // Initialize i18n first
  await window.i18n.init();
  window.i18n.applyTranslations();
  
  // Wait a bit for Tauri to initialize
  await new Promise(resolve => setTimeout(resolve, 100));
  
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const { open, save } = window.__TAURI__.dialog;
  const { getCurrentWindow, ProgressBarStatus } = window.__TAURI__.window;

  // Dock progress helper (macOS dock icon progress bar)
  const appWindow = getCurrentWindow();
  async function setDockProgress(percent, status = 'normal') {
    try {
      if (status === 'none') {
        await appWindow.setProgressBar({ status: ProgressBarStatus.None });
      } else if (status === 'error') {
        await appWindow.setProgressBar({ status: ProgressBarStatus.Error, progress: percent });
      } else if (status === 'paused') {
        await appWindow.setProgressBar({ status: ProgressBarStatus.Paused, progress: percent });
      } else {
        await appWindow.setProgressBar({ status: ProgressBarStatus.Normal, progress: percent });
      }
    } catch (err) {
      console.log('Dock progress error:', err);
    }
  }
  
  // Notification helper
  async function sendNotification(title, body) {
    try {
      const { isPermissionGranted, requestPermission, sendNotification: notify } = window.__TAURI__.notification;
      let permissionGranted = await isPermissionGranted();
      if (!permissionGranted) {
        const permission = await requestPermission();
        permissionGranted = permission === 'granted';
      }
      if (permissionGranted) {
        notify({ title, body });
      }
    } catch (err) {
      console.log('Notification error:', err);
    }
  }

  // State
  let selectedIsoPath = '';
  let selectedBurnDisk = null;
  let selectedBackupDisk = null;
  let selectedBackupDestination = '';
  let volumeInfo = null;
  let isBurning = false;
  let isBackingUp = false;
  let burnCancelled = false;
  let backupCancelled = false;
  let selectedDiagnoseDisk = null;
  let isDiagnosing = false;
  let diagnoseCancelled = false;
  
  // ETA tracking
  let burnStartTime = null;
  let backupStartTime = null;
  let diagnoseStartTime = null;

  // Recent Files Management
  const MAX_RECENT_FILES = 10;
  
  function getRecentIsoFiles() {
    try {
      const stored = localStorage.getItem('recentIsoFiles');
      return stored ? JSON.parse(stored) : [];
    } catch (e) {
      return [];
    }
  }
  
  function addRecentIsoFile(path) {
    if (!path) return;
    let recent = getRecentIsoFiles();
    // Remove if already exists
    recent = recent.filter(f => f !== path);
    // Add to front
    recent.unshift(path);
    // Limit to MAX_RECENT_FILES
    recent = recent.slice(0, MAX_RECENT_FILES);
    localStorage.setItem('recentIsoFiles', JSON.stringify(recent));
    updateRecentFilesDropdown();
  }
  
  function getRecentBackupDestinations() {
    try {
      const stored = localStorage.getItem('recentBackupDestinations');
      return stored ? JSON.parse(stored) : [];
    } catch (e) {
      return [];
    }
  }
  
  function addRecentBackupDestination(path) {
    if (!path) return;
    // Store only directory, not full file path
    const dir = path.substring(0, path.lastIndexOf('/'));
    if (!dir) return;
    let recent = getRecentBackupDestinations();
    recent = recent.filter(d => d !== dir);
    recent.unshift(dir);
    recent = recent.slice(0, MAX_RECENT_FILES);
    localStorage.setItem('recentBackupDestinations', JSON.stringify(recent));
  }

  // Confirm modal elements
  const confirmModal = document.getElementById('confirm-modal');
  const confirmTitle = document.getElementById('confirm-title');
  const confirmMessage = document.getElementById('confirm-message');
  const confirmOkBtn = document.getElementById('confirm-ok-btn');
  const confirmCancelBtn = document.getElementById('confirm-cancel-btn');
  let confirmResolve = null;

  // Confirm dialog function - returns a Promise
  function requestConfirm(title, message, okLabel, cancelLabel) {
    return new Promise((resolve) => {
      confirmResolve = resolve;
      confirmTitle.textContent = title;
      confirmMessage.textContent = message;
      confirmOkBtn.textContent = okLabel || 'Ja, löschen';
      confirmCancelBtn.textContent = cancelLabel || 'Abbrechen';
      confirmModal.classList.remove('hidden');
      setTimeout(() => confirmOkBtn.focus(), 100);
    });
  }

  // Confirm modal event handlers
  confirmOkBtn.addEventListener('click', function() {
    confirmModal.classList.add('hidden');
    if (confirmResolve) {
      confirmResolve(true);
    }
    confirmResolve = null;
  });

  confirmCancelBtn.addEventListener('click', function() {
    confirmModal.classList.add('hidden');
    if (confirmResolve) {
      confirmResolve(false);
    }
    confirmResolve = null;
  });

  // Handle Escape key in confirm modal
  confirmModal.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      confirmCancelBtn.click();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      confirmOkBtn.click();
    }
  });

  // Password modal elements
  const passwordModal = document.getElementById('password-modal');
  const passwordInput = document.getElementById('password-input');
  const passwordPrompt = document.getElementById('password-prompt');
  const passwordOkBtn = document.getElementById('password-ok-btn');
  const passwordCancelBtn = document.getElementById('password-cancel-btn');
  let passwordResolve = null;
  let passwordReject = null;

  // Password dialog function - returns a Promise
  function requestPassword(promptText) {
    return new Promise((resolve, reject) => {
      passwordResolve = resolve;
      passwordReject = reject;
      passwordPrompt.textContent = promptText;
      passwordInput.value = '';
      passwordModal.classList.remove('hidden');
      setTimeout(() => passwordInput.focus(), 100);
    });
  }

  // Password modal event handlers
  passwordOkBtn.addEventListener('click', function() {
    const password = passwordInput.value;
    passwordModal.classList.add('hidden');
    if (password && passwordResolve) {
      passwordResolve(password);
    } else if (passwordReject) {
      passwordReject('No password entered');
    }
    passwordResolve = null;
    passwordReject = null;
  });

  passwordCancelBtn.addEventListener('click', function() {
    passwordModal.classList.add('hidden');
    passwordInput.value = '';
    if (passwordReject) {
      passwordReject('Passwortabfrage abgebrochen');
    }
    passwordResolve = null;
    passwordReject = null;
  });

  // Handle Enter key in password input
  passwordInput.addEventListener('keydown', function(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      passwordOkBtn.click();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      passwordCancelBtn.click();
    }
  });

  // DOM Elements
  const tabs = document.querySelectorAll('.tab-btn');
  const tabContents = document.querySelectorAll('.tab-content');

  // Burn tab elements
  const isoPathInput = document.getElementById('iso-path');
  const selectIsoBtn = document.getElementById('select-iso-btn');
  const recentIsoSelect = document.getElementById('recent-iso-select');
  const burnDiskSelect = document.getElementById('burn-disk-select');
  const refreshBurnDisks = document.getElementById('refresh-burn-disks');
  const burnDiskInfo = document.getElementById('burn-disk-info');
  const verifyAfterBurn = document.getElementById('verify-after-burn');
  const ejectAfterBurn = document.getElementById('eject-after-burn');
  const burnBtn = document.getElementById('burn-btn');
  const cancelBurnBtn = document.getElementById('cancel-burn-btn');
  const burnProgressFill = document.getElementById('burn-progress-fill');
  const burnProgressText = document.getElementById('burn-progress-text');
  const burnEta = document.getElementById('burn-eta');
  const burnPhase = document.getElementById('burn-phase');
  const burnLog = document.getElementById('burn-log');
  
  // ETA calculation helper
  function formatEta(seconds) {
    if (seconds <= 0 || !isFinite(seconds)) return '';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    const remaining = window.i18n.t('common.remaining') || 'verbleibend';
    if (mins >= 60) {
      const hours = Math.floor(mins / 60);
      const remainingMins = mins % 60;
      return `~${hours}:${remainingMins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')} ${remaining}`;
    }
    return `~${mins}:${secs.toString().padStart(2, '0')} ${remaining}`;
  }
  
  function calculateEta(startTime, percent) {
    if (!startTime || percent <= 0) return '';
    const elapsed = (Date.now() - startTime) / 1000; // seconds
    const remaining = (elapsed / percent) * (100 - percent);
    return formatEta(remaining);
  }

  // Translation helper shortcut
  function t(key) {
    return window.i18n.t(key) || key;
  }

  // Backup tab elements
  const backupDiskSelect = document.getElementById('backup-disk-select');
  const refreshBackupDisks = document.getElementById('refresh-backup-disks');
  const backupDiskInfo = document.getElementById('backup-disk-info');
  const backupDestinationInput = document.getElementById('backup-destination');
  const selectDestinationBtn = document.getElementById('select-destination-btn');
  const backupModeRaw = document.querySelector('input[name="backup-mode"][value="raw"]');
  const backupModeFilesystem = document.querySelector('input[name="backup-mode"][value="filesystem"]');
  const backupBtn = document.getElementById('backup-btn');
  const cancelBackupBtn = document.getElementById('cancel-backup-btn');
  const backupProgressFill = document.getElementById('backup-progress-fill');
  const backupProgressText = document.getElementById('backup-progress-text');
  const backupEta = document.getElementById('backup-eta');
  const backupLog = document.getElementById('backup-log');

  // Diagnose tab elements
  const diagnoseDiskSelect = document.getElementById('diagnose-disk-select');
  const refreshDiagnoseDisks = document.getElementById('refresh-diagnose-disks');
  const diagnoseDiskInfo = document.getElementById('diagnose-disk-info');
  const diagnoseModeInputs = document.querySelectorAll('input[name="diagnose-mode"]');
  const diagnoseWarning = document.getElementById('diagnose-warning');
  const diagnoseBtn = document.getElementById('diagnose-btn');
  const cancelDiagnoseBtn = document.getElementById('cancel-diagnose-btn');
  const diagnoseProgressFill = document.getElementById('diagnose-progress-fill');
  const diagnoseProgressText = document.getElementById('diagnose-progress-text');
  const diagnoseEta = document.getElementById('diagnose-eta');
  const diagnosePhase = document.getElementById('diagnose-phase');
  const statSectorsChecked = document.getElementById('stat-sectors-checked');
  const statErrorsFound = document.getElementById('stat-errors-found');
  const statReadSpeed = document.getElementById('stat-read-speed');
  const statWriteSpeed = document.getElementById('stat-write-speed');
  const diagnoseLog = document.getElementById('diagnose-log');
  
  // SMART elements
  const smartLoading = document.getElementById('smart-loading');
  const smartUnavailable = document.getElementById('smart-unavailable');
  const smartUnavailableMsg = document.getElementById('smart-unavailable-msg');
  const smartData = document.getElementById('smart-data');
  const smartHealthValue = document.getElementById('smart-health-value');
  const smartTempValue = document.getElementById('smart-temp-value');
  const smartHoursValue = document.getElementById('smart-hours-value');
  const smartCyclesValue = document.getElementById('smart-cycles-value');
  const smartReallocatedValue = document.getElementById('smart-reallocated-value');
  const smartPendingValue = document.getElementById('smart-pending-value');
  const smartUncorrectableValue = document.getElementById('smart-uncorrectable-value');
  const smartSource = document.getElementById('smart-source');
  const smartWarning = document.getElementById('smart-warning');
  const smartStatusBadge = document.getElementById('smart-status-badge');
  const smartSection = document.getElementById('smart-section');
  const smartHeader = document.getElementById('smart-header');
  const smartContent = document.getElementById('smart-content');
  const statsSection = document.getElementById('diagnose-stats-section');
  const statsHeader = document.getElementById('stats-header');
  const statsContent = document.getElementById('stats-content');
  const statsSummaryBadge = document.getElementById('stats-summary-badge');

  // Tools tab elements
  const toolsDiskSelect = document.getElementById('tools-disk-select');
  const refreshToolsDisks = document.getElementById('refresh-tools-disks');
  const toolsDiskInfo = document.getElementById('tools-disk-info');
  const formatFilesystem = document.getElementById('format-filesystem');
  const formatName = document.getElementById('format-name');
  const formatScheme = document.getElementById('format-scheme');
  const formatBtn = document.getElementById('format-btn');
  const repairBtn = document.getElementById('repair-btn');
  const eraseLevelInputs = document.querySelectorAll('input[name="erase-level"]');
  const secureEraseBtn = document.getElementById('secure-erase-btn');
  const cancelEraseBtn = document.getElementById('cancel-erase-btn');
  const bootcheckBtn = document.getElementById('bootcheck-btn');
  const bootcheckResult = document.getElementById('bootcheck-result');
  const toolsProgressFill = document.getElementById('tools-progress-fill');
  const toolsProgressText = document.getElementById('tools-progress-text');
  const toolsEta = document.getElementById('tools-eta');
  const toolsPhase = document.getElementById('tools-phase');
  const toolsLog = document.getElementById('tools-log');
  
  // Forensic tab elements
  const forensicDiskSelect = document.getElementById('forensic-disk-select');
  const refreshForensicDisks = document.getElementById('refresh-forensic-disks');
  const forensicBtn = document.getElementById('forensic-btn');
  const forensicResult = document.getElementById('forensic-result');
  const forensicExportSection = document.getElementById('forensic-export-section');
  const copyForensicBtn = document.getElementById('copy-forensic-btn');
  const exportHtmlBtn = document.getElementById('export-html-btn');
  const forensicLog = document.getElementById('forensic-log');
  
  // Debug check for Tools elements
  console.log('Tools Tab Elements loaded:', {
    toolsDiskSelect: !!toolsDiskSelect,
    refreshToolsDisks: !!refreshToolsDisks,
    formatBtn: !!formatBtn,
    secureEraseBtn: !!secureEraseBtn,
    bootcheckBtn: !!bootcheckBtn,
    bootcheckResult: !!bootcheckResult,
    toolsLog: !!toolsLog
  });
  
  // Forensic tab state
  let selectedForensicDisk = null;
  let lastForensicResult = null;
  let forensicTabLoaded = false;
  
  // Tools tab state
  let selectedToolsDisk = null;
  let isToolsRunning = false;
  let toolsStartTime = null;

  // Collapsible section toggle
  function setupCollapsible(header, content, section) {
    header.addEventListener('click', () => {
      const isExpanded = section.classList.contains('expanded');
      if (isExpanded) {
        section.classList.remove('expanded');
        content.classList.add('hidden');
      } else {
        section.classList.add('expanded');
        content.classList.remove('hidden');
      }
    });
  }
  
  setupCollapsible(smartHeader, smartContent, smartSection);
  setupCollapsible(statsHeader, statsContent, statsSection);

  // Track if smartctl check was already done
  let smartctlCheckDone = false;
  let toolsTabLoaded = false;

  // Tab switching
  tabs.forEach(tab => {
    tab.addEventListener('click', async () => {
      tabs.forEach(t => t.classList.remove('active'));
      tabContents.forEach(c => c.classList.remove('active'));
      tab.classList.add('active');
      document.getElementById(tab.dataset.tab + '-tab').classList.add('active');
      
      // Check smartctl when switching to diagnose tab
      if (tab.dataset.tab === 'diagnose' && !smartctlCheckDone) {
        smartctlCheckDone = true;
        try {
          const installed = await invoke('check_smartctl_installed');
          if (!installed) {
            logDiagnose(t('diagnose.smartTip'), 'info');
            logDiagnose(t('diagnose.smartInstall'), 'warning');
            logDiagnose(t('diagnose.smartNote'), 'info');
          } else {
            logDiagnose(t('diagnose.smartDetected'), 'success');
          }
        } catch (err) {
          // Ignore errors
        }
      }
      
      // Load disks when switching to tools tab (only first time with logging)
      if (tab.dataset.tab === 'tools') {
        if (!toolsTabLoaded) {
          toolsTabLoaded = true;
          loadDisks(toolsDiskSelect, toolsDiskInfo, logTools);
        } else {
          loadDisksSilent(toolsDiskSelect, toolsDiskInfo);
        }
      }
      
      // Load disks when switching to forensic tab
      if (tab.dataset.tab === 'forensic') {
        if (!forensicTabLoaded) {
          forensicTabLoaded = true;
          loadDisks(forensicDiskSelect, null, logForensic);
        } else {
          loadDisksSilent(forensicDiskSelect, null);
        }
      }
    });
  });

  // ===== DRAG & DROP FOR ISO FILES (Tauri 2) =====
  const dropOverlay = document.getElementById('drop-overlay');
  const container = document.querySelector('.container');

  // Helper to set ISO file from path
  function setIsoFile(path) {
    if (path && (path.toLowerCase().endsWith('.iso') || path.toLowerCase().endsWith('.img'))) {
      selectedIsoPath = path;
      isoPathInput.value = path;
      logBurn('ISO file selected: ' + path.split('/').pop(), 'info');
      updateBurnButton();
      // Reset progress when selecting new file
      setDockProgress(0, 'none');
      burnProgressFill.style.width = '0%';
      burnProgressText.textContent = '0%';
      burnEta.textContent = '';
      burnPhase.textContent = '';
      burnPhase.className = 'phase-text';
      
      // Switch to burn tab if not already there
      const burnTab = document.querySelector('[data-tab="burn"]');
      if (!burnTab.classList.contains('active')) {
        burnTab.click();
      }
      return true;
    }
    return false;
  }

  // Listen for Tauri's drag-drop events
  listen('tauri://drag-enter', (event) => {
    dropOverlay.classList.remove('hidden');
    container.classList.add('drag-over');
  });

  listen('tauri://drag-leave', (event) => {
    dropOverlay.classList.add('hidden');
    container.classList.remove('drag-over');
  });

  listen('tauri://drag-drop', (event) => {
    dropOverlay.classList.add('hidden');
    container.classList.remove('drag-over');
    
    // event.payload contains the paths array
    const paths = event.payload.paths || event.payload;
    if (paths && paths.length > 0) {
      const filePath = paths[0];
      if (setIsoFile(filePath)) {
        logBurn('✓ ISO file dropped: ' + filePath.split('/').pop(), 'success');
      } else {
        logBurn('⚠ Only .iso and .img files are supported', 'warning');
      }
    }
  });

  // Logging functions
  function logBurn(message, type) {
    type = type || 'info';
    const timestamp = new Date().toLocaleTimeString();
    burnLog.innerHTML += '<span class="' + type + '">[' + timestamp + '] ' + message + '</span>\n';
    burnLog.scrollTop = burnLog.scrollHeight;
  }

  function logBackup(message, type) {
    type = type || 'info';
    const timestamp = new Date().toLocaleTimeString();
    backupLog.innerHTML += '<span class="' + type + '">[' + timestamp + '] ' + message + '</span>\n';
    backupLog.scrollTop = backupLog.scrollHeight;
  }

  function logDiagnose(message, type) {
    type = type || 'info';
    const timestamp = new Date().toLocaleTimeString();
    diagnoseLog.innerHTML += '<span class="' + type + '">[' + timestamp + '] ' + message + '</span>\n';
    diagnoseLog.scrollTop = diagnoseLog.scrollHeight;
  }

  function logTools(message, type) {
    type = type || 'info';
    const timestamp = new Date().toLocaleTimeString();
    toolsLog.innerHTML += '<span class="' + type + '">[' + timestamp + '] ' + message + '</span>\n';
    toolsLog.scrollTop = toolsLog.scrollHeight;
  }

  function logForensic(message, type) {
    type = type || 'info';
    const timestamp = new Date().toLocaleTimeString();
    forensicLog.innerHTML += '<span class="' + type + '">[' + timestamp + '] ' + message + '</span>\n';
    forensicLog.scrollTop = forensicLog.scrollHeight;
  }

  // Reset burn state to initial (silent = no disk reload log)
  function resetBurnState(silent) {
    isBurning = false;
    burnStartTime = null;
    burnProgressFill.style.width = '0%';
    burnProgressText.textContent = '0%';
    burnEta.textContent = '';
    burnPhase.textContent = '';
    burnPhase.className = 'phase-text';
    cancelBurnBtn.disabled = true;
    // Clear dock progress bar
    setDockProgress(0, 'none');
    updateBurnButton();
    if (!silent) {
      loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
    } else {
      loadDisksSilent(burnDiskSelect, burnDiskInfo);
    }
  }

  // Reset backup state to initial
  function resetBackupState(silent) {
    isBackingUp = false;
    backupStartTime = null;
    backupProgressFill.style.width = '0%';
    backupProgressText.textContent = '0%';
    backupEta.textContent = '';
    cancelBackupBtn.disabled = true;
    // Clear dock progress bar
    setDockProgress(0, 'none');
    updateBackupButton();
    if (!silent) {
      loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
    } else {
      loadDisksSilent(backupDiskSelect, backupDiskInfo);
    }
  }

  // Reset diagnose state to initial
  function resetDiagnoseState(silent) {
    isDiagnosing = false;
    diagnoseStartTime = null;
    diagnoseProgressFill.style.width = '0%';
    diagnoseProgressText.textContent = '0%';
    diagnoseEta.textContent = '';
    diagnosePhase.textContent = '';
    diagnosePhase.className = 'phase-text';
    statSectorsChecked.textContent = '0';
    statErrorsFound.textContent = '0';
    statReadSpeed.textContent = '-';
    statWriteSpeed.textContent = '-';
    cancelDiagnoseBtn.disabled = true;
    // Clear dock progress bar
    setDockProgress(0, 'none');
    updateDiagnoseButton();
    if (!silent) {
      loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
    } else {
      loadDisksSilent(diagnoseDiskSelect, diagnoseDiskInfo);
    }
  }

  // Format bytes to human readable string
  function formatBytes(bytes) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  }

  // Load disks (with logging)
  async function loadDisks(selectElement, infoElement, logFn) {
    selectElement.innerHTML = '<option value="">' + window.i18n.t('burn.selectUsbPlaceholder') + '</option>';
    
    try {
      const disks = await invoke('list_disks');
      selectElement.innerHTML = '<option value="">' + window.i18n.t('burn.selectUsbPlaceholder') + '</option>';
      
      if (disks.length === 0) {
        logFn('No external USB drives found', 'warning');
      } else {
        disks.forEach(function(disk) {
          const option = document.createElement('option');
          option.value = JSON.stringify(disk);
          option.textContent = disk.id + ' - ' + disk.name + ' (' + disk.size + ')';
          selectElement.appendChild(option);
        });
        logFn(disks.length + ' external USB drive(s) found', 'info');
      }
    } catch (err) {
      selectElement.innerHTML = '<option value="">' + window.i18n.t('burn.selectUsbPlaceholder') + '</option>';
      logFn('Error: ' + err, 'error');
    }
    
    infoElement.classList.remove('visible');
  }

  // Load disks silently (no logging)
  async function loadDisksSilent(selectElement, infoElement) {
    selectElement.innerHTML = '<option value="">' + window.i18n.t('burn.selectUsbPlaceholder') + '</option>';
    
    try {
      const disks = await invoke('list_disks');
      selectElement.innerHTML = '<option value="">' + window.i18n.t('burn.selectUsbPlaceholder') + '</option>';
      
      disks.forEach(function(disk) {
        const option = document.createElement('option');
        option.value = JSON.stringify(disk);
        option.textContent = disk.id + ' - ' + disk.name + ' (' + disk.size + ')';
        selectElement.appendChild(option);
      });
    } catch (err) {
      selectElement.innerHTML = '<option value="">' + window.i18n.t('burn.selectUsbPlaceholder') + '</option>';
    }
    
    infoElement.classList.remove('visible');
  }

  // Update recent files dropdown
  function updateRecentFilesDropdown() {
    if (!recentIsoSelect) return;
    const recent = getRecentIsoFiles();
    const placeholderText = window.i18n?.t('burn.recentFiles') || 'Zuletzt verwendet...';
    
    if (recent.length === 0) {
      recentIsoSelect.innerHTML = '<option value="">' + placeholderText + '</option>';
      recentIsoSelect.disabled = true;
      return;
    }
    
    recentIsoSelect.disabled = false;
    recentIsoSelect.innerHTML = '<option value="">' + placeholderText + '</option>';
    recent.forEach(function(path) {
      const option = document.createElement('option');
      option.value = path;
      // Show only filename for display
      const filename = path.split('/').pop();
      option.textContent = filename;
      option.title = path; // Full path as tooltip
      recentIsoSelect.appendChild(option);
    });
  }
  
  // Initialize recent files dropdown on load
  updateRecentFilesDropdown();

  // Show disk info
  async function showDiskInfo(diskId, infoElement, logFn) {
    try {
      const info = await invoke('get_disk_info', { diskId: diskId });
      infoElement.textContent = info;
      infoElement.classList.add('visible');
    } catch (err) {
      logFn('Error loading disk info: ' + err, 'error');
    }
  }

  // Check volume info for filesystem backup support
  async function checkVolumeInfo(diskId) {
    try {
      volumeInfo = await invoke('get_volume_info', { diskId: diskId });
      
      if (volumeInfo) {
        // Bei ISO-Dateisystemen: "Dateibasiert" deaktiviert lassen
        if (volumeInfo.filesystem && volumeInfo.filesystem.startsWith('ISO:')) {
          backupModeFilesystem.disabled = true;
          backupModeRaw.checked = true;
        } else {
          backupModeFilesystem.disabled = false;
        }
      } else {
        // Kein Dateisystem erkannt - Raw-Modus erzwingen
        backupModeFilesystem.disabled = true;
        backupModeRaw.checked = true;
      }
    } catch (err) {
      console.error('Volume info error:', err);
      backupModeFilesystem.disabled = true;
      backupModeRaw.checked = true;
      volumeInfo = null;
    }
  }

  // Update button states
  function updateBurnButton() {
    burnBtn.disabled = !selectedIsoPath || !selectedBurnDisk || isBurning;
  }

  function updateBackupButton() {
    backupBtn.disabled = !selectedBackupDisk || !selectedBackupDestination || isBackingUp;
  }

  function updateDiagnoseButton() {
    diagnoseBtn.disabled = !selectedDiagnoseDisk || isDiagnosing;
  }

  // Event listeners - Burn tab
  selectIsoBtn.addEventListener('click', async function() {
    try {
      const selected = await open({
        filters: [{ name: 'ISO/IMG Files', extensions: ['iso', 'img', 'dmg'] }],
        multiple: false
      });
      
      if (selected) {
        selectedIsoPath = selected;
        isoPathInput.value = selected;
        logBurn('ISO selected: ' + selected, 'success');
        updateBurnButton();
        // Reset progress when selecting new file
        setDockProgress(0, 'none');
        burnProgressFill.style.width = '0%';
        burnProgressText.textContent = '0%';
        burnEta.textContent = '';
        burnPhase.textContent = '';
        burnPhase.className = 'phase-text';
        // Reset recent dropdown selection
        if (recentIsoSelect) recentIsoSelect.value = '';
      }
    } catch (err) {
      logBurn('Selection error: ' + err, 'error');
    }
  });

  // Recent files dropdown change
  if (recentIsoSelect) {
    recentIsoSelect.addEventListener('change', function() {
      if (recentIsoSelect.value) {
        selectedIsoPath = recentIsoSelect.value;
        isoPathInput.value = recentIsoSelect.value;
        logBurn('ISO selected: ' + recentIsoSelect.value, 'success');
        updateBurnButton();
        // Reset progress
        setDockProgress(0, 'none');
        burnProgressFill.style.width = '0%';
        burnProgressText.textContent = '0%';
        burnEta.textContent = '';
        burnPhase.textContent = '';
        burnPhase.className = 'phase-text';
      }
    });
  }

  refreshBurnDisks.addEventListener('click', function() {
    loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
  });

  burnDiskSelect.addEventListener('change', async function() {
    if (burnDiskSelect.value) {
      selectedBurnDisk = JSON.parse(burnDiskSelect.value);
      // Reset progress when selecting new disk
      setDockProgress(0, 'none');
      burnProgressFill.style.width = '0%';
      burnProgressText.textContent = '0%';
      burnEta.textContent = '';
      burnPhase.textContent = '';
      burnPhase.className = 'phase-text';
      await showDiskInfo(selectedBurnDisk.id, burnDiskInfo, logBurn);
      logBurn('USB selected: ' + selectedBurnDisk.name + ' (' + selectedBurnDisk.size + ')', 'info');
    } else {
      selectedBurnDisk = null;
      burnDiskInfo.classList.remove('visible');
    }
    updateBurnButton();
  });

  burnBtn.addEventListener('click', async function() {
    if (!selectedIsoPath || !selectedBurnDisk) return;
    
    // Confirmation dialog
    const confirmed = await requestConfirm(
      '⚠️ WARNING!',
      'All data on "' + selectedBurnDisk.name + '" (' + selectedBurnDisk.id + ') will be PERMANENTLY deleted!\n\nContinue?',
      'Yes, delete',
      'Cancel'
    );
    
    if (!confirmed) {
      logBurn('Burn cancelled', 'warning');
      return;
    }

    // Passwort im App-Fenster abfragen
    let password;
    try {
      password = await requestPassword('Zum Schreiben auf den USB-Stick werden Administrator-Rechte benötigt.\n\nBitte geben Sie Ihr macOS-Passwort ein:');
    } catch (err) {
      logBurn('Password prompt cancelled', 'warning');
      return;
    }
    
    // Optionen lesen
    const doVerify = verifyAfterBurn.checked;
    const doEject = ejectAfterBurn.checked;
    
    // Start burn
    isBurning = true;
    burnCancelled = false;
    burnStartTime = Date.now();
    burnBtn.disabled = true;
    cancelBurnBtn.disabled = false;
    burnProgressFill.style.width = '0%';
    burnProgressText.textContent = '0%';
    burnEta.textContent = '';
    burnPhase.textContent = 'Phase 1: Writing...';
    burnPhase.className = 'phase-text writing';
    
    logBurn('Starting burn process...', 'info');
    if (doVerify) {
      logBurn('Verification after burn enabled', 'info');
    }
    
    try {
      const result = await invoke('burn_iso', {
        isoPath: selectedIsoPath,
        diskId: selectedBurnDisk.id,
        password: password,
        verify: doVerify,
        eject: doEject
      });
      logBurn(result, 'success');
      burnProgressFill.style.width = '100%';
      burnProgressText.textContent = '100%';
      burnEta.textContent = '';
      burnPhase.textContent = doVerify ? '✓ Written and verified!' : '✓ Successfully written!';
      burnPhase.className = 'phase-text success';
      
      // Add to recent files on success
      addRecentIsoFile(selectedIsoPath);
      
      // Send notification
      sendNotification(
        window.i18n.t('notifications.burnComplete') || 'Brennvorgang abgeschlossen',
        window.i18n.t('notifications.burnSuccess') || 'ISO wurde erfolgreich auf USB gebrannt!'
      );
      
      isBurning = false;
      burnBtn.disabled = false;
      cancelBurnBtn.disabled = true;
      loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
    } catch (err) {
      // On cancel: Short message only
      if (burnCancelled) {
        logBurn('✗ Burn cancelled', 'warning');
        burnPhase.textContent = 'Cancelled';
        burnPhase.className = 'phase-text error';
      } else {
        logBurn('Error: ' + err, 'error');
        burnPhase.textContent = 'Error!';
        burnPhase.className = 'phase-text error';
      }
      resetBurnState(true); // silent reset
    }
  });

  cancelBurnBtn.addEventListener('click', async function() {
    burnCancelled = true;
    cancelBurnBtn.disabled = true;
    try {
      await invoke('cancel_burn');
      logBurn('Cancelling...', 'warning');
    } catch (err) {
      logBurn('Cancel error: ' + err, 'error');
    }
  });

  // Event listeners - Backup tab
  refreshBackupDisks.addEventListener('click', function() {
    loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
  });

  backupDiskSelect.addEventListener('change', async function() {
    if (backupDiskSelect.value) {
      selectedBackupDisk = JSON.parse(backupDiskSelect.value);
      // Reset progress when selecting new disk
      setDockProgress(0, 'none');
      backupProgressFill.style.width = '0%';
      backupProgressText.textContent = '0%';
      backupEta.textContent = '';
      await showDiskInfo(selectedBackupDisk.id, backupDiskInfo, logBackup);
      await checkVolumeInfo(selectedBackupDisk.id);
      logBackup('USB selected: ' + selectedBackupDisk.name + ' (' + selectedBackupDisk.size + ')', 'info');
    } else {
      selectedBackupDisk = null;
      backupDiskInfo.classList.remove('visible');
      backupModeFilesystem.disabled = true;
      volumeInfo = null;
    }
    updateBackupButton();
  });

  selectDestinationBtn.addEventListener('click', async function() {
    const isFilesystemMode = backupModeFilesystem.checked;
    const extension = isFilesystemMode ? 'dmg' : 'iso';
    const defaultName = 'USB_Backup_' + new Date().toISOString().slice(0, 10) + '.' + extension;
    
    try {
      const selected = await save({
        defaultPath: defaultName,
        filters: [{ 
          name: isFilesystemMode ? 'DMG Image' : 'ISO/IMG Image', 
          extensions: isFilesystemMode ? ['dmg'] : ['iso', 'img'] 
        }]
      });
      
      if (selected) {
        selectedBackupDestination = selected;
        backupDestinationInput.value = selected;
        logBackup('Destination: ' + selected, 'success');
        updateBackupButton();
      }
    } catch (err) {
      logBackup('Selection error: ' + err, 'error');
    }
  });

  backupBtn.addEventListener('click', async function() {
    if (!selectedBackupDisk || !selectedBackupDestination) return;
    
    const isFilesystemMode = backupModeFilesystem.checked;

    // Passwort nur bei Raw-Modus abfragen (Filesystem braucht kein sudo)
    let password = null;
    if (!isFilesystemMode) {
      try {
        password = await requestPassword('Zum Lesen des USB-Sticks werden Administrator-Rechte benötigt.\n\nBitte geben Sie Ihr macOS-Passwort ein:');
      } catch (err) {
        logBackup('Password prompt cancelled', 'warning');
        return;
      }
    }

    isBackingUp = true;
    backupCancelled = false;
    backupStartTime = Date.now();
    backupBtn.disabled = true;
    cancelBackupBtn.disabled = false;
    backupProgressFill.style.width = '0%';
    backupProgressText.textContent = '0%';
    backupEta.textContent = '';
    
    logBackup('Starting backup (' + (isFilesystemMode ? 'Filesystem' : 'Raw') + ')...', 'info');
    
    try {
      let result;
      
      if (isFilesystemMode && volumeInfo) {
        result = await invoke('backup_usb_filesystem', {
          mountPoint: volumeInfo.mount_point,
          destination: selectedBackupDestination,
          volumeName: volumeInfo.name
        });
      } else {
        // Bei ISO-Dateisystemen die Volume-Größe statt Disk-Größe verwenden
        let backupSize = selectedBackupDisk.bytes || 0;
        if (volumeInfo && volumeInfo.filesystem && volumeInfo.filesystem.startsWith('ISO:')) {
          backupSize = volumeInfo.bytes || backupSize;
          logBackup('ISO image detected: Only ' + formatBytes(backupSize) + ' will be backed up (instead of ' + selectedBackupDisk.size + ')', 'info');
        }
        
        result = await invoke('backup_usb_raw', {
          diskId: selectedBackupDisk.id,
          destination: selectedBackupDestination,
          diskSize: backupSize,
          password: password
        });
      }
      
      logBackup(result, 'success');
      backupProgressFill.style.width = '100%';
      backupProgressText.textContent = '100%';
      backupEta.textContent = '';
      
      // Clear dock progress bar on success
      setDockProgress(100, 'none');
      
      // Add to recent backup destinations on success
      addRecentBackupDestination(selectedBackupDestination);
      
      // Send notification
      sendNotification(
        window.i18n.t('notifications.backupComplete') || 'Backup abgeschlossen',
        window.i18n.t('notifications.backupSuccess') || 'USB wurde erfolgreich gesichert!'
      );
      
      isBackingUp = false;
      backupBtn.disabled = false;
      cancelBackupBtn.disabled = true;
      loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
    } catch (err) {
      // On cancel: Short message only
      if (backupCancelled) {
        logBackup('✗ Backup cancelled', 'warning');
      } else {
        logBackup('Error: ' + err, 'error');
      }
      resetBackupState(true); // silent reset
    }
  });

  cancelBackupBtn.addEventListener('click', async function() {
    backupCancelled = true;
    cancelBackupBtn.disabled = true;
    try {
      await invoke('cancel_backup');
      logBackup('Cancelling...', 'warning');
    } catch (err) {
      logBackup('Cancel error: ' + err, 'error');
    }
  });

  // Event listeners - Diagnose tab
  refreshDiagnoseDisks.addEventListener('click', function() {
    loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
  });

  diagnoseDiskSelect.addEventListener('change', async function() {
    if (diagnoseDiskSelect.value) {
      selectedDiagnoseDisk = JSON.parse(diagnoseDiskSelect.value);
      // Reset progress when selecting new disk
      setDockProgress(0, 'none');
      diagnoseProgressFill.style.width = '0%';
      diagnoseProgressText.textContent = '0%';
      diagnoseEta.textContent = '';
      diagnosePhase.textContent = '';
      diagnosePhase.className = 'phase-text';
      statSectorsChecked.textContent = '0';
      statErrorsFound.textContent = '0';
      statReadSpeed.textContent = '-';
      statWriteSpeed.textContent = '-';
      statsSummaryBadge.classList.add('hidden');
      await showDiskInfo(selectedDiagnoseDisk.id, diagnoseDiskInfo, logDiagnose);
      logDiagnose(t('diagnose.usbSelected').replace('{name}', selectedDiagnoseDisk.name).replace('{size}', selectedDiagnoseDisk.size), 'info');
      
      // Load SMART data
      await loadSmartData(selectedDiagnoseDisk.id);
    } else {
      selectedDiagnoseDisk = null;
      diagnoseDiskInfo.classList.remove('visible');
      resetSmartDisplay();
    }
    updateDiagnoseButton();
  });
  
  // SMART data functions
  function resetSmartDisplay() {
    smartLoading.classList.add('hidden');
    smartUnavailable.classList.add('hidden');
    smartData.classList.add('hidden');
    smartWarning.classList.add('hidden');
    smartStatusBadge.classList.add('hidden');
  }
  
  async function loadSmartData(diskId) {
    resetSmartDisplay();
    smartLoading.classList.remove('hidden');
    
    try {
      const data = await invoke('get_smart_data', { diskId: diskId });
      
      smartLoading.classList.add('hidden');
      
      if (!data.available) {
        smartUnavailable.classList.remove('hidden');
        if (data.error_message) {
          smartUnavailableMsg.textContent = data.error_message;
        }
        // Update badge
        smartStatusBadge.textContent = 'N/A';
        smartStatusBadge.className = 'status-badge unavailable';
        smartStatusBadge.classList.remove('hidden');
        logDiagnose(t('diagnose.smartNotAvailable').replace('{msg}', data.error_message || t('diagnose.smartUnavailable')), 'info');
        return;
      }
      
      // Show SMART data
      smartData.classList.remove('hidden');
      
      // Health status
      smartHealthValue.textContent = data.health_status;
      if (data.health_status.includes('PASSED') || data.health_status.includes('✅')) {
        smartHealthValue.className = 'smart-health-value passed';
        smartStatusBadge.textContent = 'OK ✅';
        smartStatusBadge.className = 'status-badge passed';
      } else if (data.health_status.includes('FAILED') || data.health_status.includes('❌')) {
        smartHealthValue.className = 'smart-health-value failed';
        smartStatusBadge.textContent = 'FAIL ❌';
        smartStatusBadge.className = 'status-badge failed';
      } else {
        smartHealthValue.className = 'smart-health-value';
        smartStatusBadge.textContent = data.health_status;
        smartStatusBadge.className = 'status-badge info';
      }
      smartStatusBadge.classList.remove('hidden');
      
      // Details
      smartTempValue.textContent = data.temperature !== null ? data.temperature + '°C' : '-';
      smartHoursValue.textContent = data.power_on_hours !== null ? data.power_on_hours.toLocaleString() + ' h' : '-';
      smartCyclesValue.textContent = data.power_cycle_count !== null ? data.power_cycle_count.toLocaleString() : '-';
      
      // Critical sectors
      const reallocated = data.reallocated_sectors;
      const pending = data.pending_sectors;
      const uncorrectable = data.uncorrectable_sectors;
      
      smartReallocatedValue.textContent = reallocated !== null ? reallocated : '-';
      smartPendingValue.textContent = pending !== null ? pending : '-';
      smartUncorrectableValue.textContent = uncorrectable !== null ? uncorrectable : '-';
      
      // Highlight warnings
      if (reallocated !== null && reallocated > 0) {
        smartReallocatedValue.className = 'smart-detail-value warning';
      } else {
        smartReallocatedValue.className = 'smart-detail-value';
      }
      
      if (pending !== null && pending > 0) {
        smartPendingValue.className = 'smart-detail-value warning';
      } else {
        smartPendingValue.className = 'smart-detail-value';
      }
      
      if (uncorrectable !== null && uncorrectable > 0) {
        smartUncorrectableValue.className = 'smart-detail-value critical';
      } else {
        smartUncorrectableValue.className = 'smart-detail-value';
      }
      
      // Source info
      if (data.source === 'smartctl') {
        smartSource.textContent = 'Datenquelle: smartmontools (smartctl)';
      } else if (data.source === 'diskutil') {
        smartSource.textContent = 'Datenquelle: macOS diskutil (eingeschränkt)';
      }
      
      // Show warning if there's additional info
      if (data.error_message) {
        smartWarning.textContent = 'ℹ️ ' + data.error_message;
        smartWarning.classList.remove('hidden');
      }
      
      logDiagnose(t('diagnose.smartStatusLog').replace('{status}', data.health_status).replace('{source}', data.source), 'success');
      
    } catch (err) {
      smartLoading.classList.add('hidden');
      smartUnavailable.classList.remove('hidden');
      smartUnavailableMsg.textContent = t('diagnose.smartError').replace('{error}', err);
      logDiagnose(t('diagnose.smartError').replace('{error}', err), 'error');
    }
  }

  // Show/hide warning based on test mode
  diagnoseModeInputs.forEach(function(input) {
    input.addEventListener('change', function() {
      const mode = document.querySelector('input[name="diagnose-mode"]:checked').value;
      if (mode === 'surface') {
        diagnoseWarning.classList.add('hidden');
      } else {
        diagnoseWarning.classList.remove('hidden');
      }
    });
  });

  diagnoseBtn.addEventListener('click', async function() {
    if (!selectedDiagnoseDisk) return;
    
    const mode = document.querySelector('input[name="diagnose-mode"]:checked').value;
    const isDestructive = (mode === 'full' || mode === 'speed');
    
    // Confirmation for destructive tests
    if (isDestructive) {
      const confirmed = await requestConfirm(
        t('diagnose.warningTitle'),
        t('diagnose.confirmDeleteMsg').replace('{name}', selectedDiagnoseDisk.name).replace('{id}', selectedDiagnoseDisk.id),
        t('diagnose.confirmDeleteYes'),
        t('dialogs.cancel')
      );
      
      if (!confirmed) {
        logDiagnose(t('diagnose.testCancelled'), 'warning');
        return;
      }
    }

    // Request password for raw device access
    let password;
    try {
      password = await requestPassword(t('dialogs.adminPasswordPrompt') + '\n\n' + t('dialogs.enterPassword') + ':');
    } catch (err) {
      logDiagnose(t('diagnose.passwordCancelled'), 'warning');
      return;
    }
    
    // Start diagnose
    isDiagnosing = true;
    diagnoseCancelled = false;
    diagnoseStartTime = Date.now();
    diagnoseBtn.disabled = true;
    cancelDiagnoseBtn.disabled = false;
    diagnoseProgressFill.style.width = '0%';
    diagnoseProgressText.textContent = '0%';
    diagnoseEta.textContent = '';
    statSectorsChecked.textContent = '0';
    statErrorsFound.textContent = '0';
    statReadSpeed.textContent = '-';
    statWriteSpeed.textContent = '-';
    statsSummaryBadge.classList.add('hidden');
    
    const modeNames = { surface: 'Surface Scan', full: t('diagnose.fullTest'), speed: t('diagnose.speedTest') };
    logDiagnose(t('diagnose.startingTest').replace('{mode}', modeNames[mode]), 'info');
    diagnosePhase.textContent = t('messages.loading');
    diagnosePhase.className = 'phase-text';
    
    try {
      let result;
      logDiagnose(t('diagnose.callingTest').replace('{mode}', mode), 'info');
      
      if (mode === 'surface') {
        result = await invoke('diagnose_surface_scan', {
          diskId: selectedDiagnoseDisk.id,
          password: password
        });
      } else if (mode === 'full') {
        logDiagnose(t('diagnose.invokingFullTest'), 'info');
        result = await invoke('diagnose_full_test', {
          diskId: selectedDiagnoseDisk.id,
          password: password
        });
        logDiagnose(t('diagnose.fullTestReturned'), 'info');
      } else if (mode === 'speed') {
        result = await invoke('diagnose_speed_test', {
          diskId: selectedDiagnoseDisk.id,
          password: password
        });
      }
      
      // Display results
      if (result.success) {
        logDiagnose('✓ ' + result.message, 'success');
        diagnosePhase.textContent = '✓ ' + t('diagnose.testComplete');
        diagnosePhase.className = 'phase-text success';
        diagnoseEta.textContent = '';
        statsSummaryBadge.textContent = '✓ OK';
        statsSummaryBadge.className = 'status-badge passed';
        statsSummaryBadge.classList.remove('hidden');
        
        // Send notification
        sendNotification(
          window.i18n.t('notifications.diagnoseComplete') || 'Test abgeschlossen',
          window.i18n.t('notifications.diagnoseSuccess') || 'USB-Test erfolgreich - keine Fehler gefunden!'
        );
      } else {
        logDiagnose('✗ ' + result.message, 'error');
        diagnosePhase.textContent = '✗ ' + t('diagnose.errorsDetected');
        diagnosePhase.className = 'phase-text error';
        diagnoseEta.textContent = '';
        
        // Send notification for errors too
        sendNotification(
          window.i18n.t('notifications.diagnoseComplete') || 'Test abgeschlossen',
          window.i18n.t('notifications.diagnoseFailed') || 'USB-Test: Fehler gefunden!'
        );
        statsSummaryBadge.textContent = '✗ ' + t('diagnose.errorsDetected');
        statsSummaryBadge.className = 'status-badge failed';
        statsSummaryBadge.classList.remove('hidden');
      }
      
      // Update final stats
      statSectorsChecked.textContent = result.sectors_checked.toLocaleString();
      statErrorsFound.textContent = result.errors_found.toLocaleString();
      if (result.read_speed_mbps > 0) {
        statReadSpeed.textContent = result.read_speed_mbps.toFixed(1) + ' MB/s';
      }
      if (result.write_speed_mbps > 0) {
        statWriteSpeed.textContent = result.write_speed_mbps.toFixed(1) + ' MB/s';
      }
      
      // Log bad sectors if any
      if (result.bad_sectors && result.bad_sectors.length > 0) {
        logDiagnose(t('diagnose.badSectorsFound').replace('{sectors}', result.bad_sectors.slice(0, 20).join(', ')) + 
                    (result.bad_sectors.length > 20 ? t('diagnose.andMore').replace('{count}', result.bad_sectors.length - 20) : ''), 'warning');
      }
      
      diagnoseProgressFill.style.width = '100%';
      diagnoseProgressText.textContent = '100%';
      
      // Clear dock progress bar on success
      setDockProgress(100, 'none');
      
      isDiagnosing = false;
      diagnoseBtn.disabled = false;
      cancelDiagnoseBtn.disabled = true;
      loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
    } catch (err) {
      if (diagnoseCancelled) {
        logDiagnose('✗ ' + t('diagnose.testCancelled'), 'warning');
        diagnosePhase.textContent = t('messages.cancelled');
        diagnosePhase.className = 'phase-text error';
      } else {
        logDiagnose(t('diagnose.error').replace('{error}', err), 'error');
        diagnosePhase.textContent = t('messages.error') + '!';
        diagnosePhase.className = 'phase-text error';
      }
      resetDiagnoseState(true);
    }
  });

  cancelDiagnoseBtn.addEventListener('click', async function() {
    diagnoseCancelled = true;
    cancelDiagnoseBtn.disabled = true;
    try {
      await invoke('cancel_diagnose');
      logDiagnose(t('diagnose.cancelling'), 'warning');
    } catch (err) {
      logDiagnose(t('diagnose.cancelError').replace('{error}', err), 'error');
    }
  });

  // ========== Tools Tab Event Handlers ==========
  
  refreshToolsDisks.addEventListener('click', function() {
    loadDisks(toolsDiskSelect, toolsDiskInfo, logTools);
  });

  toolsDiskSelect.addEventListener('change', async function() {
    if (toolsDiskSelect.value) {
      selectedToolsDisk = JSON.parse(toolsDiskSelect.value);
      await showDiskInfo(selectedToolsDisk.id, toolsDiskInfo, logTools);
      logTools('USB selected: ' + selectedToolsDisk.name + ' (' + selectedToolsDisk.size + ')', 'info');
      formatBtn.disabled = false;
      repairBtn.disabled = false;
      secureEraseBtn.disabled = false;
      bootcheckBtn.disabled = false;
    } else {
      selectedToolsDisk = null;
      toolsDiskInfo.classList.remove('visible');
      formatBtn.disabled = true;
      repairBtn.disabled = true;
      secureEraseBtn.disabled = true;
      bootcheckBtn.disabled = true;
    }
  });

  formatBtn.addEventListener('click', async function() {
    if (!selectedToolsDisk) return;
    
    const filesystem = formatFilesystem.value;
    const name = formatName.value || 'USB_STICK';
    const scheme = formatScheme.value;
    
    // Confirmation dialog
    const confirmed = await requestConfirm(
      '⚠️ ' + t('tools.formatWarning'),
      t('tools.formatConfirmMsg').replace('{name}', selectedToolsDisk.name).replace('{fs}', filesystem),
      t('tools.formatConfirmYes'),
      t('dialogs.cancel')
    );
    
    if (!confirmed) {
      logTools(t('tools.formatCancelled'), 'warning');
      return;
    }
    
    let password;
    try {
      password = await requestPassword(t('tools.formatAdminPrompt'));
    } catch (e) {
      logTools(t('tools.formatCancelled'), 'warning');
      return;
    }
    
    isToolsRunning = true;
    toolsStartTime = Date.now();
    formatBtn.disabled = true;
    repairBtn.disabled = true;
    secureEraseBtn.disabled = true;
    bootcheckBtn.disabled = true;
    
    // Reset progress display
    toolsProgressFill.style.width = '0%';
    toolsProgressText.textContent = '0%';
    toolsEta.textContent = '';
    toolsPhase.textContent = t('tools.formatFormatting');
    toolsPhase.className = 'phase-text';
    
    logTools(t('tools.formatStarting').replace('{fs}', filesystem), 'info');
    
    try {
      const result = await invoke('format_disk', {
        diskId: selectedToolsDisk.id,
        filesystem: filesystem,
        name: name,
        scheme: scheme,
        password: password
      });
      logTools(result, 'success');
      toolsProgressFill.style.width = '100%';
      toolsProgressText.textContent = '100%';
      toolsPhase.textContent = t('tools.formatComplete');
      toolsPhase.className = 'phase-text success';
      
      sendNotification(t('notifications.formatComplete'), t('notifications.formatSuccess'));
      loadDisks(toolsDiskSelect, toolsDiskInfo, logTools);
    } catch (err) {
      logTools(t('messages.error') + ': ' + err, 'error');
      toolsPhase.textContent = t('tools.formatError');
      toolsPhase.className = 'phase-text error';
    }
    
    isToolsRunning = false;
    formatBtn.disabled = !selectedToolsDisk;
    repairBtn.disabled = !selectedToolsDisk;
    secureEraseBtn.disabled = !selectedToolsDisk;
    bootcheckBtn.disabled = !selectedToolsDisk;
  });

  // Repair disk button
  repairBtn.addEventListener('click', async function() {
    if (!selectedToolsDisk) return;
    
    let password;
    try {
      password = await requestPassword(t('tools.repairAdminPrompt'));
    } catch (e) {
      logTools(t('tools.repairCancelled'), 'warning');
      return;
    }
    
    if (!password) {
      logTools(t('tools.repairCancelled'), 'warning');
      return;
    }
    
    isToolsRunning = true;
    formatBtn.disabled = true;
    repairBtn.disabled = true;
    secureEraseBtn.disabled = true;
    bootcheckBtn.disabled = true;
    
    logTools(t('tools.repairStarting'), 'info');
    toolsPhase.textContent = t('tools.repairRepairing');
    toolsPhase.className = 'phase-text';
    
    try {
      const result = await invoke('repair_disk', { 
        diskId: selectedToolsDisk.id,
        password: password
      });
      logTools(result, 'success');
      toolsProgressFill.style.width = '100%';
      toolsProgressText.textContent = '100%';
      
      // Check if result indicates success
      if (result.includes('OK') || result.includes('successfully') || result.includes('erfolgreich')) {
        toolsPhase.textContent = t('tools.repairNoErrors');
      } else {
        toolsPhase.textContent = t('tools.repairComplete');
      }
      toolsPhase.className = 'phase-text success';
      
      loadDisks(toolsDiskSelect, toolsDiskInfo, logTools);
    } catch (err) {
      logTools(t('tools.repairError') + ': ' + err, 'error');
      toolsPhase.textContent = t('tools.repairError');
      toolsPhase.className = 'phase-text error';
    }
    
    isToolsRunning = false;
    formatBtn.disabled = !selectedToolsDisk;
    repairBtn.disabled = !selectedToolsDisk;
    secureEraseBtn.disabled = !selectedToolsDisk;
    bootcheckBtn.disabled = !selectedToolsDisk;
  });

  secureEraseBtn.addEventListener('click', async function() {
    if (!selectedToolsDisk) return;
    
    const eraseLevel = document.querySelector('input[name="erase-level"]:checked').value;
    const levelNames = {
      '0': t('tools.eraseQuick'),
      '1': t('tools.eraseRandom'),
      '3': t('tools.eraseGutmann'),
      '4': t('tools.eraseDoe')
    };
    
    // Confirmation dialog
    const confirmed = await requestConfirm(
      '⚠️ ' + t('tools.eraseWarning'),
      t('tools.eraseConfirmMsg').replace('{name}', selectedToolsDisk.name).replace('{method}', levelNames[eraseLevel]),
      t('tools.eraseConfirmYes'),
      t('dialogs.cancel')
    );
    
    if (!confirmed) {
      logTools(t('tools.eraseCancelled'), 'warning');
      return;
    }
    
    let password;
    try {
      password = await requestPassword(t('tools.eraseAdminPrompt'));
    } catch (e) {
      logTools(t('tools.eraseCancelled'), 'warning');
      return;
    }
    
    isToolsRunning = true;
    toolsStartTime = Date.now();
    formatBtn.disabled = true;
    repairBtn.disabled = true;
    secureEraseBtn.disabled = true;
    bootcheckBtn.disabled = true;
    cancelEraseBtn.classList.remove('hidden');
    cancelEraseBtn.disabled = false;
    
    // Reset progress display
    toolsProgressFill.style.width = '0%';
    toolsProgressText.textContent = '0%';
    toolsEta.textContent = '';
    toolsPhase.textContent = t('tools.eraseErasing');
    toolsPhase.className = 'phase-text';
    
    logTools(t('tools.eraseStarting').replace('{method}', levelNames[eraseLevel]), 'info');
    logTools(t('tools.eraseTimeWarning'), 'warning');
    
    try {
      const result = await invoke('secure_erase', {
        diskId: selectedToolsDisk.id,
        level: parseInt(eraseLevel),
        password: password
      });
      logTools(result, 'success');
      toolsProgressFill.style.width = '100%';
      toolsProgressText.textContent = '100%';
      toolsPhase.textContent = t('tools.eraseComplete');
      toolsPhase.className = 'phase-text success';
      
      sendNotification(t('notifications.eraseComplete'), t('notifications.eraseSuccess'));
      loadDisks(toolsDiskSelect, toolsDiskInfo, logTools);
    } catch (err) {
      const errMsg = String(err);
      if (errMsg.includes('abgebrochen') || errMsg.includes('cancelled')) {
        logTools(t('tools.eraseCancelled'), 'warning');
        toolsPhase.textContent = t('tools.eraseAborted');
        toolsPhase.className = 'phase-text warning';
        toolsProgressFill.style.width = '0%';
        toolsProgressText.textContent = '0%';
      } else {
        logTools(t('messages.error') + ': ' + err, 'error');
        toolsPhase.textContent = t('tools.formatError');
        toolsPhase.className = 'phase-text error';
      }
    }
    
    isToolsRunning = false;
    formatBtn.disabled = !selectedToolsDisk;
    repairBtn.disabled = !selectedToolsDisk;
    secureEraseBtn.disabled = !selectedToolsDisk;
    bootcheckBtn.disabled = !selectedToolsDisk;
    cancelEraseBtn.classList.add('hidden');
    cancelEraseBtn.disabled = true;
  });

  cancelEraseBtn.addEventListener('click', async function() {
    logTools(t('messages.cancelled') + '...', 'warning');
    cancelEraseBtn.disabled = true;
    try {
      await invoke('cancel_tools');
    } catch (err) {
      logTools(t('messages.error') + ': ' + err, 'error');
    }
  });

  bootcheckBtn.addEventListener('click', async function() {
    console.log('Bootcheck clicked, selectedToolsDisk:', selectedToolsDisk);
    if (!selectedToolsDisk) {
      console.log('No disk selected, returning');
      return;
    }
    
    // Request password (needs raw disk access)
    let password;
    try {
      password = await requestPassword(t('tools.bootAdminPrompt'));
    } catch (e) {
      logTools(t('tools.bootCancelled'), 'warning');
      return;
    }
    
    console.log('Password received, starting bootcheck');
    logTools(t('tools.bootStarting') + ' ' + selectedToolsDisk.name + '...', 'info');
    bootcheckResult.classList.add('hidden');
    
    try {
      const result = await invoke('check_bootable', { diskId: selectedToolsDisk.id, password: password });
      console.log('Bootcheck result:', result);
      
      let html = '<div class="bootcheck-details">';
      html += '<div class="bootcheck-status ' + (result.bootable ? 'bootable' : 'not-bootable') + '">';
      html += result.bootable ? '✓ ' + t('tools.bootBootable') : '✗ ' + t('tools.bootNotBootable');
      html += '</div>';
      html += '<div class="bootcheck-type">' + result.boot_type + '</div>';
      html += '<ul class="bootcheck-info">';
      html += '<li>' + t('tools.bootMbrSig') + ': ' + (result.has_mbr ? '✓' : '✗') + '</li>';
      html += '<li>' + t('tools.bootGpt') + ': ' + (result.has_gpt ? '✓' : '✗') + '</li>';
      html += '<li>' + t('tools.bootEfiPart') + ': ' + (result.has_efi ? '✓' : '✗') + '</li>';
      html += '<li>' + t('tools.bootFlag') + ': ' + (result.has_bootable_flag ? '✓' : '✗') + '</li>';
      if (result.is_iso) {
        html += '<li>' + t('tools.bootIso9660') + ': ✓</li>';
        html += '<li>' + t('tools.bootElTorito') + ': ' + (result.has_el_torito ? '✓' : '✗') + '</li>';
      }
      html += '</ul></div>';
      
      bootcheckResult.innerHTML = html;
      bootcheckResult.classList.remove('hidden');
      
      logTools(t('tools.bootAnalysis') + ': ' + result.boot_type, result.bootable ? 'success' : 'warning');
    } catch (err) {
      logTools(t('tools.bootError') + ': ' + err, 'error');
      bootcheckResult.innerHTML = '<div class="bootcheck-error">' + t('messages.error') + ': ' + err + '</div>';
      bootcheckResult.classList.remove('hidden');
    }
  });

  // ===== FORENSIC TAB HANDLERS =====
  
  // Forensic disk select change handler
  forensicDiskSelect.addEventListener('change', async function() {
    if (forensicDiskSelect.value) {
      selectedForensicDisk = JSON.parse(forensicDiskSelect.value);
      logForensic('USB ausgewählt: ' + selectedForensicDisk.name + ' (' + selectedForensicDisk.size + ')', 'info');
      forensicBtn.disabled = false;
    } else {
      selectedForensicDisk = null;
      forensicBtn.disabled = true;
    }
  });
  
  // Refresh forensic disks button
  refreshForensicDisks.addEventListener('click', function() {
    loadDisks(forensicDiskSelect, null, logForensic);
  });

  // Forensic Analysis button handler
  forensicBtn.addEventListener('click', async function() {
    if (!selectedForensicDisk) return;
    
    // Request password (needs raw disk access)
    let password;
    try {
      password = await requestPassword(t('tools.forensicAdminPrompt') || 'Administrator-Rechte für forensische Analyse erforderlich');
    } catch (e) {
      logForensic(t('tools.forensicCancelled') || 'Forensik-Analyse abgebrochen', 'warning');
      return;
    }
    
    logForensic(t('tools.forensicStarting') || 'Starte Forensik-Analyse...', 'info');
    forensicResult.classList.add('hidden');
    forensicExportSection.classList.add('hidden');
    forensicBtn.disabled = true;
    
    try {
      const result = await invoke('forensic_analysis', { 
        diskId: selectedForensicDisk.id, 
        password: password 
      });
      
      // Store for export
      lastForensicResult = result;
      
      // Build the forensic report HTML
      let html = '<div class="forensic-report">';
      
      // Header with timestamp
      html += '<div class="forensic-header">';
      html += '<h4>🔬 ' + (t('tools.forensicTitle') || 'Forensik-Analyse') + '</h4>';
      html += '<div class="forensic-timestamp">' + (t('tools.forensicTimestamp') || 'Zeitstempel') + ': ' + result.timestamp + '</div>';
      html += '</div>';
      
      // Device Info Section
      html += '<div class="forensic-section">';
      html += '<h5>📱 ' + (t('tools.forensicDeviceInfo') || 'Geräteinformationen') + '</h5>';
      html += '<div class="forensic-grid">';
      for (let key in result.disk_info) {
        if (result.disk_info[key]) {
          html += '<div class="forensic-item"><span class="forensic-label">' + key + ':</span> <span class="forensic-value">' + result.disk_info[key] + '</span></div>';
        }
      }
      html += '</div></div>';
      
      // USB Info Section
      if (result.usb_info && Object.keys(result.usb_info).length > 0) {
        html += '<div class="forensic-section">';
        html += '<h5>🔌 ' + (t('tools.forensicUsbInfo') || 'USB-Controller-Daten') + '</h5>';
        html += '<div class="forensic-grid">';
        for (let key in result.usb_info) {
          if (result.usb_info[key]) {
            html += '<div class="forensic-item"><span class="forensic-label">' + key + ':</span> <span class="forensic-value">' + result.usb_info[key] + '</span></div>';
          }
        }
        html += '</div></div>';
      }
      
      // Partition Layout Section
      if (result.partition_layout) {
        html += '<div class="forensic-section">';
        html += '<h5>💾 ' + (t('tools.forensicPartitions') || 'Partitionslayout') + '</h5>';
        html += '<div class="forensic-partitions">';
        if (Array.isArray(result.partition_layout)) {
          result.partition_layout.forEach((p, i) => {
            html += '<div class="forensic-partition">';
            html += '<strong>' + p.identifier + '</strong> (' + (p.size || 'N/A') + ')';
            if (p.name) html += ' - ' + p.name;
            if (p.type) html += ' [' + p.type + ']';
            html += '</div>';
          });
        } else if (typeof result.partition_layout === 'string' && result.partition_layout.trim()) {
          // diskutil list output as string - display as preformatted text
          html += '<pre class="forensic-partition-raw">' + result.partition_layout + '</pre>';
        }
        html += '</div></div>';
      }
      
      // Boot Info Section
      if (result.boot_info) {
        html += '<div class="forensic-section">';
        html += '<h5>🚀 ' + (t('tools.forensicBootInfo') || 'Boot-Strukturen') + '</h5>';
        html += '<div class="forensic-grid">';
        // Use correct key names from Rust backend
        const hasMbr = result.boot_info.has_mbr_signature || result.boot_info.has_mbr;
        const hasGpt = result.boot_info.has_gpt;
        const hasEfi = result.mbr_analysis?.partition_entries?.some(p => p.type_hex === 'EF') || result.boot_info.has_efi;
        const isBootable = hasMbr || hasGpt || result.boot_info.is_iso9660;
        
        html += '<div class="forensic-item"><span class="forensic-label">MBR-Signatur:</span> <span class="forensic-value">' + (hasMbr ? '✓ (55AA)' : '✗') + '</span></div>';
        html += '<div class="forensic-item"><span class="forensic-label">GPT:</span> <span class="forensic-value">' + (hasGpt ? '✓ (EFI PART)' : '✗') + '</span></div>';
        html += '<div class="forensic-item"><span class="forensic-label">EFI-Partition:</span> <span class="forensic-value">' + (hasEfi ? '✓' : '✗') + '</span></div>';
        
        // Determine boot type
        let bootType = '';
        if (result.boot_info.is_iso9660) {
          bootType = 'ISO 9660';
          if (result.boot_info.has_el_torito_boot) bootType += ' + El Torito';
        } else if (hasGpt && hasEfi) {
          bootType = 'UEFI (GPT)';
        } else if (hasMbr && hasEfi) {
          bootType = 'UEFI (MBR)';
        } else if (hasMbr) {
          bootType = 'Legacy BIOS (MBR)';
        }
        
        html += '<div class="forensic-item"><span class="forensic-label">' + (t('tools.forensicBootable') || 'Bootfähig') + ':</span> <span class="forensic-value">' + (isBootable ? '✓ ' + bootType : '✗') + '</span></div>';
        
        if (result.boot_info.is_iso9660) {
          html += '<div class="forensic-item"><span class="forensic-label">ISO 9660:</span> <span class="forensic-value">✓</span></div>';
          if (result.boot_info.iso_volume_label) {
            html += '<div class="forensic-item"><span class="forensic-label">Volume Label:</span> <span class="forensic-value">' + result.boot_info.iso_volume_label + '</span></div>';
          }
          html += '<div class="forensic-item"><span class="forensic-label">El Torito:</span> <span class="forensic-value">' + (result.boot_info.has_el_torito_boot ? '✓' : '✗') + '</span></div>';
        }
        
        if (result.boot_info.mbr_partitions && result.boot_info.mbr_partitions !== 'none') {
          html += '<div class="forensic-item full-width"><span class="forensic-label">MBR-Partitionen:</span> <span class="forensic-value">' + result.boot_info.mbr_partitions + '</span></div>';
        }
        
        if (result.boot_info.gpt_disk_guid) {
          html += '<div class="forensic-item full-width"><span class="forensic-label">GPT Disk GUID:</span> <span class="forensic-value mono">' + result.boot_info.gpt_disk_guid + '</span></div>';
        }
        
        html += '</div></div>';
      }
      
      // Filesystem Signatures Section
      if (result.filesystem_signatures && result.filesystem_signatures.length > 0) {
        html += '<div class="forensic-section">';
        html += '<h5>📂 ' + (t('tools.forensicFilesystems') || 'Erkannte Dateisysteme') + '</h5>';
        html += '<div class="forensic-filesystems">';
        result.filesystem_signatures.forEach(fs => {
          html += '<div class="forensic-fs-item">';
          html += '<span class="fs-name">' + fs.filesystem + '</span>';
          html += ' @ Offset ' + fs.offset;
          if (fs.label) html += ' - Label: "' + fs.label + '"';
          html += '</div>';
        });
        html += '</div></div>';
      }
      
      // Content Analysis Section
      if (result.content_analysis) {
        html += '<div class="forensic-section">';
        html += '<h5>📁 ' + (t('tools.forensicContent') || 'Inhaltsanalyse') + '</h5>';
        html += '<div class="forensic-grid">';
        if (result.content_analysis.mount_point) {
          html += '<div class="forensic-item"><span class="forensic-label">Mount:</span> <span class="forensic-value">' + result.content_analysis.mount_point + '</span></div>';
        }
        if (result.content_analysis.total_items !== undefined) {
          html += '<div class="forensic-item"><span class="forensic-label">' + (t('tools.forensicTotalItems') || 'Elemente') + ':</span> <span class="forensic-value">' + result.content_analysis.total_items + '</span></div>';
        }
        if (result.content_analysis.detected_os && result.content_analysis.detected_os.length > 0) {
          html += '<div class="forensic-item"><span class="forensic-label">' + (t('tools.forensicDetectedOS') || 'Erkannte OS') + ':</span> <span class="forensic-value">' + result.content_analysis.detected_os.join(', ') + '</span></div>';
        }
        if (result.content_analysis.top_level && result.content_analysis.top_level.length > 0) {
          html += '<div class="forensic-item full-width"><span class="forensic-label">' + (t('tools.forensicTopLevel') || 'Hauptverzeichnis') + ':</span></div>';
          html += '<div class="forensic-toplevel">' + result.content_analysis.top_level.map(f => '<span class="toplevel-item">' + f + '</span>').join('') + '</div>';
        }
        html += '</div></div>';
      }
      
      // Special Structures Section
      if (result.special_structures) {
        html += '<div class="forensic-section">';
        html += '<h5>🔎 ' + (t('tools.forensicSpecial') || 'Spezialstrukturen') + '</h5>';
        html += '<div class="forensic-grid">';
        for (let key in result.special_structures) {
          html += '<div class="forensic-item"><span class="forensic-label">' + key + ':</span> <span class="forensic-value">' + (result.special_structures[key] ? '✓' : '✗') + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Hardware Info Section
      if (result.hardware_info) {
        html += '<div class="forensic-section">';
        html += '<h5>🔧 ' + (t('tools.forensicHardwareInfo') || 'Hardware-Details') + '</h5>';
        html += '<div class="forensic-grid">';
        for (let key in result.hardware_info) {
          html += '<div class="forensic-item"><span class="forensic-label">' + key.replace(/_/g, ' ') + ':</span> <span class="forensic-value">' + result.hardware_info[key] + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Controller Info Section
      if (result.controller_info) {
        html += '<div class="forensic-section">';
        html += '<h5>🎛️ ' + (t('tools.forensicController') || 'USB-Controller') + '</h5>';
        html += '<div class="forensic-grid">';
        for (let key in result.controller_info) {
          html += '<div class="forensic-item"><span class="forensic-label">' + key.replace(/_/g, ' ') + ':</span> <span class="forensic-value">' + result.controller_info[key] + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Storage Info Section
      if (result.storage_info) {
        html += '<div class="forensic-section">';
        html += '<h5>💿 ' + (t('tools.forensicStorageInfo') || 'Speicher-Details') + '</h5>';
        html += '<div class="forensic-grid">';
        for (let key in result.storage_info) {
          let value = result.storage_info[key];
          // Format bytes to human-readable
          if (key.includes('bytes') && typeof value === 'number') {
            value = formatBytes(value);
          }
          html += '<div class="forensic-item"><span class="forensic-label">' + key.replace(/_/g, ' ') + ':</span> <span class="forensic-value">' + value + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Disk Activity Section
      if (result.disk_activity) {
        html += '<div class="forensic-section">';
        html += '<h5>📊 ' + (t('tools.forensicActivity') || 'Disk-Aktivität') + '</h5>';
        html += '<div class="forensic-grid">';
        html += '<div class="forensic-item"><span class="forensic-label">KB/Transfer:</span> <span class="forensic-value">' + result.disk_activity.kb_per_transfer + '</span></div>';
        html += '<div class="forensic-item"><span class="forensic-label">Transfers/s:</span> <span class="forensic-value">' + result.disk_activity.transfers_per_sec + '</span></div>';
        html += '<div class="forensic-item"><span class="forensic-label">MB/s:</span> <span class="forensic-value">' + result.disk_activity.mb_per_sec + '</span></div>';
        html += '</div></div>';
      }
      
      // MBR Analysis Section
      if (result.mbr_analysis) {
        html += '<div class="forensic-section">';
        html += '<h5>📀 ' + (t('tools.forensicMbrAnalysis') || 'MBR-Analyse') + '</h5>';
        html += '<div class="forensic-grid">';
        html += '<div class="forensic-item"><span class="forensic-label">MBR-Signatur:</span> <span class="forensic-value">' + result.mbr_analysis.mbr_signature + '</span></div>';
        html += '<div class="forensic-item"><span class="forensic-label">Gültig:</span> <span class="forensic-value">' + (result.mbr_analysis.valid_mbr ? '✓ Ja' : '✗ Nein') + '</span></div>';
        html += '</div>';
        if (result.mbr_analysis.partition_entries && result.mbr_analysis.partition_entries.length > 0) {
          html += '<div class="forensic-partitions" style="margin-top:8px;">';
          result.mbr_analysis.partition_entries.forEach(p => {
            html += '<div class="forensic-partition">';
            html += '<strong>Partition ' + p.number + '</strong>';
            html += ' [' + p.type_hex + '] ' + p.type_name;
            if (p.bootable) html += ' 🚀 Boot';
            html += '</div>';
          });
          html += '</div>';
        }
        html += '</div>';
      }
      
      // GPT Analysis Section
      if (result.gpt_analysis) {
        html += '<div class="forensic-section">';
        html += '<h5>📦 ' + (t('tools.forensicGptAnalysis') || 'GPT-Analyse') + '</h5>';
        html += '<div class="forensic-grid">';
        html += '<div class="forensic-item"><span class="forensic-label">GPT-Signatur:</span> <span class="forensic-value">' + result.gpt_analysis.gpt_signature + '</span></div>';
        html += '<div class="forensic-item"><span class="forensic-label">Gültig:</span> <span class="forensic-value">' + (result.gpt_analysis.valid_gpt ? '✓ Ja' : '✗ Nein') + '</span></div>';
        if (result.gpt_analysis.gpt_revision) {
          html += '<div class="forensic-item"><span class="forensic-label">Revision:</span> <span class="forensic-value">' + result.gpt_analysis.gpt_revision + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Filesystem Details Section
      if (result.filesystem_details) {
        html += '<div class="forensic-section">';
        html += '<h5>📁 ' + (t('tools.forensicFsDetails') || 'Dateisystem-Details') + '</h5>';
        html += '<div class="forensic-grid">';
        if (result.filesystem_details.total_file_count) {
          html += '<div class="forensic-item"><span class="forensic-label">Dateien:</span> <span class="forensic-value">' + result.filesystem_details.total_file_count + '</span></div>';
        }
        if (result.filesystem_details.directory_count) {
          html += '<div class="forensic-item"><span class="forensic-label">Ordner:</span> <span class="forensic-value">' + result.filesystem_details.directory_count + '</span></div>';
        }
        if (result.filesystem_details.hidden_files_count) {
          html += '<div class="forensic-item"><span class="forensic-label">Versteckte Dateien:</span> <span class="forensic-value">' + result.filesystem_details.hidden_files_count + '</span></div>';
        }
        if (result.filesystem_details.symlink_count) {
          html += '<div class="forensic-item"><span class="forensic-label">Symlinks:</span> <span class="forensic-value">' + result.filesystem_details.symlink_count + '</span></div>';
        }
        if (result.filesystem_details.capacity_percent) {
          html += '<div class="forensic-item"><span class="forensic-label">Kapazität:</span> <span class="forensic-value">' + result.filesystem_details.capacity_percent + '</span></div>';
        }
        if (result.filesystem_details.inode_usage_percent) {
          html += '<div class="forensic-item"><span class="forensic-label">Inode-Nutzung:</span> <span class="forensic-value">' + result.filesystem_details.inode_usage_percent + '</span></div>';
        }
        html += '</div>';
        
        // Largest files
        if (result.filesystem_details.largest_files && result.filesystem_details.largest_files.length > 0) {
          html += '<div class="forensic-subsection"><strong>' + (t('tools.forensicLargestFiles') || 'Größte Dateien') + ':</strong>';
          html += '<div class="forensic-filelist">';
          result.filesystem_details.largest_files.forEach(f => {
            const sizeFormatted = formatBytes(parseInt(f.size_bytes) || 0);
            html += '<div class="forensic-file-item"><span class="file-size">' + sizeFormatted + '</span> <span class="file-path">' + f.path + '</span></div>';
          });
          html += '</div></div>';
        }
        
        // File type distribution
        if (result.filesystem_details.file_type_distribution && result.filesystem_details.file_type_distribution.length > 0) {
          html += '<div class="forensic-subsection"><strong>' + (t('tools.forensicFileTypes') || 'Dateitypen') + ':</strong>';
          html += '<div class="forensic-types">';
          result.filesystem_details.file_type_distribution.forEach(ft => {
            html += '<span class="forensic-type-badge">' + ft.extension + ' (' + ft.count + ')</span>';
          });
          html += '</div></div>';
        }
        
        // Recently modified
        if (result.filesystem_details.recently_modified && result.filesystem_details.recently_modified.length > 0) {
          html += '<div class="forensic-subsection"><strong>' + (t('tools.forensicRecent') || 'Kürzlich geändert (7 Tage)') + ':</strong>';
          html += '<div class="forensic-filelist">';
          result.filesystem_details.recently_modified.forEach(f => {
            html += '<div class="forensic-file-item"><span class="file-path">' + f + '</span></div>';
          });
          html += '</div></div>';
        }
        html += '</div>';
      }
      
      // SMART Info Section
      if (result.smart_info) {
        html += '<div class="forensic-section">';
        html += '<h5>🔬 ' + (t('tools.forensicSmart') || 'SMART-Daten') + '</h5>';
        html += '<div class="forensic-grid">';
        for (let key in result.smart_info) {
          html += '<div class="forensic-item"><span class="forensic-label">' + key.replace(/_/g, ' ') + ':</span> <span class="forensic-value">' + result.smart_info[key] + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Sector Checksums Section
      if (result.sector_checksums) {
        html += '<div class="forensic-section">';
        html += '<h5>🔐 ' + (t('tools.forensicChecksums') || 'MBR-Checksummen') + '</h5>';
        html += '<div class="forensic-grid">';
        if (result.sector_checksums.mbr_md5) {
          html += '<div class="forensic-item full-width"><span class="forensic-label">MD5:</span> <span class="forensic-value mono">' + result.sector_checksums.mbr_md5 + '</span></div>';
        }
        if (result.sector_checksums.mbr_sha256) {
          html += '<div class="forensic-item full-width"><span class="forensic-label">SHA256:</span> <span class="forensic-value mono">' + result.sector_checksums.mbr_sha256 + '</span></div>';
        }
        html += '</div></div>';
      }
      
      // Raw Header Hex Dump Section
      if (result.raw_header_hex) {
        html += '<div class="forensic-section">';
        html += '<h5>🔢 ' + (t('tools.forensicRawHeader') || 'Raw Header (Hex)') + '</h5>';
        html += '<pre class="forensic-hexdump">' + result.raw_header_hex + '</pre>';
        html += '</div>';
      }
      
      html += '</div>';
      
      forensicResult.innerHTML = html;
      forensicResult.classList.remove('hidden');
      forensicExportSection.classList.remove('hidden');
      
      logForensic((t('tools.forensicComplete') || '✓ Forensik-Analyse abgeschlossen!'), 'success');
    } catch (err) {
      logForensic((t('tools.forensicError') || 'Forensik-Analyse Fehler') + ': ' + err, 'error');
      forensicResult.innerHTML = '<div class="forensic-error">' + t('messages.error') + ': ' + err + '</div>';
      forensicResult.classList.remove('hidden');
    } finally {
      forensicBtn.disabled = !selectedForensicDisk;
    }
  });
  
  // Copy forensic JSON button
  copyForensicBtn.addEventListener('click', async function() {
    if (!lastForensicResult) return;
    const reportText = JSON.stringify(lastForensicResult, null, 2);
    try {
      await navigator.clipboard.writeText(reportText);
      copyForensicBtn.textContent = '✓ Kopiert!';
      logForensic('JSON-Report in Zwischenablage kopiert', 'success');
      setTimeout(() => {
        copyForensicBtn.textContent = '📋 JSON kopieren';
      }, 2000);
    } catch (err) {
      logForensic('Clipboard error: ' + err, 'error');
    }
  });
  
  // Export as HTML button
  exportHtmlBtn.addEventListener('click', async function() {
    if (!lastForensicResult) return;
    
    try {
      const deviceName = (lastForensicResult.disk_info?.Device || lastForensicResult.disk_info?.['Device Identifier'] || 'usb').replace('/dev/', '');
      const filePath = await save({
        defaultPath: 'forensic-report-' + deviceName + '.html',
        filters: [{ name: 'HTML', extensions: ['html'] }]
      });
      
      if (filePath) {
        const htmlContent = generateForensicHtmlReport(lastForensicResult);
        await invoke('write_text_file', { path: filePath, content: htmlContent });
        logForensic('Report als HTML gespeichert: ' + filePath, 'success');
      }
    } catch (err) {
      logForensic('Export-Fehler: ' + err, 'error');
    }
  });
  
  // Helper function to generate standalone HTML report
  function generateForensicHtmlReport(result) {
    const deviceName = result.disk_info?.Device || result.disk_info?.['Device Identifier'] || 'USB';
    let html = `<!DOCTYPE html>
<html lang="de">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Forensik-Report - ${deviceName}</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 1000px; margin: 0 auto; padding: 20px; background: #f5f5f5; }
    .report { background: white; border-radius: 8px; padding: 20px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
    .header { border-bottom: 2px solid #2196F3; padding-bottom: 10px; margin-bottom: 20px; }
    .header h1 { margin: 0; color: #2196F3; }
    .timestamp { color: #666; font-size: 14px; }
    .section { margin-bottom: 20px; padding: 15px; background: #f9f9f9; border-radius: 6px; }
    .section h2 { margin: 0 0 10px 0; font-size: 16px; color: #333; }
    .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 10px; }
    .item { display: flex; gap: 5px; }
    .label { font-weight: 500; color: #555; }
    .value { color: #333; }
    .mono { font-family: 'Monaco', 'Consolas', monospace; font-size: 12px; }
    .hexdump { background: #1e1e1e; color: #d4d4d4; padding: 10px; border-radius: 4px; font-family: monospace; font-size: 11px; overflow-x: auto; white-space: pre; }
    .partition { padding: 5px 10px; background: #e3f2fd; border-radius: 4px; margin: 2px 0; }
    .filelist { margin-top: 5px; }
    .file-item { font-size: 13px; padding: 2px 0; }
    .type-badge { display: inline-block; padding: 2px 8px; background: #e0e0e0; border-radius: 10px; margin: 2px; font-size: 12px; }
    .full-width { grid-column: 1 / -1; }
    @media print { body { background: white; } .report { box-shadow: none; } }
  </style>
</head>
<body>
  <div class="report">
    <div class="header">
      <h1>🔬 Forensik-Analyse Report</h1>
      <div class="timestamp">Erstellt: ${result.timestamp}</div>
    </div>`;
    
    // Device Info
    html += `<div class="section"><h2>📱 Geräteinformationen</h2><div class="grid">`;
    for (let key in result.disk_info) {
      if (result.disk_info[key]) {
        html += `<div class="item"><span class="label">${key}:</span> <span class="value">${result.disk_info[key]}</span></div>`;
      }
    }
    html += `</div></div>`;
    
    // USB Info
    if (result.usb_info && Object.keys(result.usb_info).length > 0) {
      html += `<div class="section"><h2>🔌 USB-Controller-Daten</h2><div class="grid">`;
      for (let key in result.usb_info) {
        if (result.usb_info[key]) {
          html += `<div class="item"><span class="label">${key}:</span> <span class="value">${result.usb_info[key]}</span></div>`;
        }
      }
      html += `</div></div>`;
    }
    
    // Boot Info
    if (result.boot_info) {
      const hasMbr = result.boot_info.has_mbr_signature || result.boot_info.has_mbr;
      const hasGpt = result.boot_info.has_gpt;
      html += `<div class="section"><h2>🚀 Boot-Strukturen</h2><div class="grid">`;
      html += `<div class="item"><span class="label">MBR-Signatur:</span> <span class="value">${hasMbr ? '✓ (55AA)' : '✗'}</span></div>`;
      html += `<div class="item"><span class="label">GPT:</span> <span class="value">${hasGpt ? '✓ (EFI PART)' : '✗'}</span></div>`;
      html += `</div></div>`;
    }
    
    // MBR Analysis
    if (result.mbr_analysis) {
      html += `<div class="section"><h2>📀 MBR-Analyse</h2><div class="grid">`;
      html += `<div class="item"><span class="label">Signatur:</span> <span class="value">${result.mbr_analysis.mbr_signature}</span></div>`;
      html += `<div class="item"><span class="label">Gültig:</span> <span class="value">${result.mbr_analysis.valid_mbr ? '✓ Ja' : '✗ Nein'}</span></div>`;
      html += `</div>`;
      if (result.mbr_analysis.partition_entries && result.mbr_analysis.partition_entries.length > 0) {
        result.mbr_analysis.partition_entries.forEach(p => {
          html += `<div class="partition"><strong>Partition ${p.number}</strong> [${p.type_hex}] ${p.type_name} ${p.bootable ? '🚀 Boot' : ''}</div>`;
        });
      }
      html += `</div>`;
    }
    
    // Checksums
    if (result.sector_checksums) {
      html += `<div class="section"><h2>🔐 MBR-Checksummen</h2><div class="grid">`;
      if (result.sector_checksums.mbr_md5) {
        html += `<div class="item full-width"><span class="label">MD5:</span> <span class="value mono">${result.sector_checksums.mbr_md5}</span></div>`;
      }
      if (result.sector_checksums.mbr_sha256) {
        html += `<div class="item full-width"><span class="label">SHA256:</span> <span class="value mono">${result.sector_checksums.mbr_sha256}</span></div>`;
      }
      html += `</div></div>`;
    }
    
    // Raw Header Hex
    if (result.raw_header_hex) {
      html += `<div class="section"><h2>🔢 Raw Header (Hex)</h2><pre class="hexdump">${result.raw_header_hex}</pre></div>`;
    }
    
    // JSON Data
    html += `<div class="section"><h2>📋 Vollständige Daten (JSON)</h2><pre class="mono" style="font-size:10px; max-height:400px; overflow:auto;">${JSON.stringify(result, null, 2)}</pre></div>`;
    
    html += `</div></body></html>`;
    return html;
  }

  // Listen for log events from backend
  listen('log', function(event) {
    const message = event.payload;
    // Route to appropriate log based on current operation
    if (isBurning) {
      logBurn(message, 'info');
    } else if (isBackingUp) {
      logBackup(message, 'info');
    } else if (isDiagnosing) {
      logDiagnose(message, 'info');
    } else if (isToolsRunning) {
      logTools(message, 'info');
    }
  });

  // Listen for progress events
  listen('progress', function(event) {
    const percent = event.payload.percent;
    const status = event.payload.status;
    const operation = event.payload.operation;
    
    // Update dock progress bar
    setDockProgress(percent);
    
    if (operation === 'burn') {
      burnProgressFill.style.width = percent + '%';
      burnProgressText.textContent = percent + '%';
      burnEta.textContent = calculateEta(burnStartTime, percent);
      // Don't log every progress update, only significant ones
      if (status.indexOf('✓') >= 0 || status.indexOf('FEHLER') >= 0) {
        logBurn(status, status.indexOf('FEHLER') >= 0 ? 'error' : 'success');
      }
    } else if (operation === 'backup') {
      backupProgressFill.style.width = percent + '%';
      backupProgressText.textContent = percent + '%';
      backupEta.textContent = calculateEta(backupStartTime, percent);
      if (status.indexOf('✓') >= 0) {
        logBackup(status, 'success');
      }
    } else if (operation === 'tools') {
      toolsProgressFill.style.width = percent + '%';
      toolsProgressText.textContent = percent + '%';
      // ETA is included in the status message from backend
      toolsEta.textContent = '';
      toolsPhase.textContent = status;
    }
  });

  // Listen for burn phase events
  listen('burn_phase', function(event) {
    const phase = event.payload;
    if (phase === 'writing') {
      burnPhase.textContent = 'Phase 1: Writing...';
      burnPhase.className = 'phase-text writing';
    } else if (phase === 'verifying') {
      burnPhase.textContent = 'Phase 2: Verifying...';
      burnPhase.className = 'phase-text verifying';
      // Reset start time for accurate ETA in verify phase
      burnStartTime = Date.now();
      burnEta.textContent = '';
      logBurn('Starting verification...', 'info');
    } else if (phase === 'success') {
      burnPhase.textContent = '✓ Successfully completed!';
      burnPhase.className = 'phase-text success';
      burnEta.textContent = '';
      // Clear dock progress bar on success
      setDockProgress(100, 'none');
    } else if (phase === 'error') {
      burnPhase.textContent = '✗ Verification failed!';
      burnPhase.className = 'phase-text error';
      burnEta.textContent = '';
      // Show error state in dock, then clear
      setDockProgress(100, 'error');
      setTimeout(() => setDockProgress(0, 'none'), 2000);
    }
  });

  // Listen for diagnose progress events
  listen('diagnose_progress', function(event) {
    const payload = event.payload;
    diagnoseProgressFill.style.width = payload.percent + '%';
    diagnoseProgressText.textContent = payload.percent + '%';
    diagnoseEta.textContent = calculateEta(diagnoseStartTime, payload.percent);
    diagnosePhase.textContent = payload.phase + ': ' + payload.status;
    
    // Update dock progress bar for diagnose
    setDockProgress(payload.percent);
    
    // Update stats in real-time
    statSectorsChecked.textContent = payload.sectors_checked.toLocaleString();
    statErrorsFound.textContent = payload.errors_found.toLocaleString();
    if (payload.read_speed_mbps > 0) {
      statReadSpeed.textContent = payload.read_speed_mbps.toFixed(1) + ' MB/s';
    }
    if (payload.write_speed_mbps > 0) {
      statWriteSpeed.textContent = payload.write_speed_mbps.toFixed(1) + ' MB/s';
    }
  });

  // Listen for menu events
  listen('menu-action', function(event) {
    const action = event.payload;
    switch (action) {
      case 'refresh':
        loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
        loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
        loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
        break;
      case 'select_iso':
        selectIsoBtn.click();
        break;
      case 'select_destination':
        selectDestinationBtn.click();
        break;
      case 'tab_burn':
        document.querySelector('[data-tab="burn"]').click();
        break;
      case 'tab_backup':
        document.querySelector('[data-tab="backup"]').click();
        break;
      case 'tab_diagnose':
        document.querySelector('[data-tab="diagnose"]').click();
        break;
      case 'tab_tools':
        document.querySelector('[data-tab="tools"]').click();
        break;
      case 'tab_forensic':
        document.querySelector('[data-tab="forensic"]').click();
        break;
      case 'start_burn':
        if (!burnBtn.disabled) burnBtn.click();
        break;
      case 'start_backup':
        if (!backupBtn.disabled) backupBtn.click();
        break;
      case 'start_diagnose':
        if (!diagnoseBtn.disabled) diagnoseBtn.click();
        break;
      case 'cancel_action':
        if (!cancelBurnBtn.disabled) cancelBurnBtn.click();
        if (!cancelBackupBtn.disabled) cancelBackupBtn.click();
        if (!cancelDiagnoseBtn.disabled) cancelDiagnoseBtn.click();
        break;
      case 'lang_de':
        window.i18n.setLanguage('de');
        break;
      case 'lang_en':
        window.i18n.setLanguage('en');
        break;
      case 'theme_dark':
        window.i18n.setTheme('dark');
        break;
      case 'theme_light':
        window.i18n.setTheme('light');
        break;
      case 'help':
        // Open help in new Tauri window
        (async () => {
          try {
            const { WebviewWindow } = window.__TAURI__.webviewWindow;
            const helpWindow = new WebviewWindow('help', {
              url: 'help.html',
              title: window.i18n.currentLang === 'de' ? 'Hilfe - BurnISO to USB' : 'Help - BurnISO to USB',
              width: 700,
              height: 800,
              center: true,
              resizable: true
            });
            helpWindow.once('tauri://created', () => {
              console.log('Help window created');
            });
            helpWindow.once('tauri://error', (e) => {
              console.error('Error creating help window:', e);
            });
          } catch (err) {
            console.error('Failed to open help:', err);
          }
        })();
        break;
    }
  });

  // Window state persistence - save position and size
  async function saveWindowState() {
    try {
      const { getCurrentWindow } = window.__TAURI__.window;
      const appWindow = getCurrentWindow();
      const size = await appWindow.innerSize();
      const position = await appWindow.outerPosition();
      const scaleFactor = await appWindow.scaleFactor();
      
      // Convert physical pixels to logical pixels
      const width = Math.round(size.width / scaleFactor);
      const height = Math.round(size.height / scaleFactor);
      
      await invoke('save_window_state', { 
        width: width, 
        height: height, 
        x: position.x, 
        y: position.y 
      });
    } catch (err) {
      console.error('Failed to save window state:', err);
    }
  }

  // Initialize window state tracking
  function initWindowStateTracking() {
    if (!window.__TAURI__ || !window.__TAURI__.window) {
      console.warn('Tauri window API not available');
      return;
    }
    
    const { getCurrentWindow } = window.__TAURI__.window;
    const appWindow = getCurrentWindow();
    
    // Debounced save on resize
    let resizeTimeout = null;
    appWindow.onResized(() => {
      if (resizeTimeout) clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(saveWindowState, 500);
    });
    
    // Debounced save on move
    let moveTimeout = null;
    appWindow.onMoved(() => {
      if (moveTimeout) clearTimeout(moveTimeout);
      moveTimeout = setTimeout(saveWindowState, 500);
    });
  }

  // Initialize
  logBurn('BurnISO to USB ready', 'info');
  logBackup('USB Backup ready', 'info');
  logDiagnose(t('diagnose.ready'), 'info');
  loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
  loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
  loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
  
  // Start window state tracking
  initWindowStateTracking();
});
