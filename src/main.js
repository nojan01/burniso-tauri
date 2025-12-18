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

  // Backup tab elements
  const backupDiskSelect = document.getElementById('backup-disk-select');
  const refreshBackupDisks = document.getElementById('refresh-backup-disks');
  const backupDiskInfo = document.getElementById('backup-disk-info');
  const backupDestinationInput = document.getElementById('backup-destination');
  const selectDestinationBtn = document.getElementById('select-destination-btn');
  const backupModeRaw = document.querySelector('input[name="backup-mode"][value="raw"]');
  const backupModeFilesystem = document.querySelector('input[name="backup-mode"][value="filesystem"]');
  const filesystemNote = document.getElementById('filesystem-note');
  const detectedFs = document.getElementById('detected-fs');
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
            logDiagnose('💡 Tip: For extended S.M.A.R.T. data on USB hard drives:', 'info');
            logDiagnose('   brew install smartmontools', 'warning');
            logDiagnose('   (USB sticks and SD cards do not support SMART)', 'info');
          } else {
            logDiagnose('✅ smartmontools detected - full SMART support available', 'success');
          }
        } catch (err) {
          // Ignore errors
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
        filesystemNote.classList.remove('hidden');
        
        // Bei ISO-Dateisystemen: "Dateibasiert" deaktiviert lassen, aber Info anzeigen
        if (volumeInfo.filesystem && volumeInfo.filesystem.startsWith('ISO:')) {
          detectedFs.textContent = volumeInfo.filesystem.substring(4) + ' (ISO-Image erkannt - ' + formatBytes(volumeInfo.bytes || 0) + ')';
          backupModeFilesystem.disabled = true;
          backupModeRaw.checked = true;
        } else {
          detectedFs.textContent = volumeInfo.filesystem;
          backupModeFilesystem.disabled = false;
        }
      } else {
        filesystemNote.classList.add('hidden');
        backupModeFilesystem.disabled = true;
        backupModeRaw.checked = true;
      }
    } catch (err) {
      console.error('Volume info error:', err);
      filesystemNote.classList.add('hidden');
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
      }
    } catch (err) {
      logBurn('Selection error: ' + err, 'error');
    }
  });

  refreshBurnDisks.addEventListener('click', function() {
    loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
  });

  burnDiskSelect.addEventListener('change', async function() {
    if (burnDiskSelect.value) {
      selectedBurnDisk = JSON.parse(burnDiskSelect.value);
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
      burnPhase.textContent = doVerify ? '✓ Written and verified!' : '✓ Successfully written!';
      burnPhase.className = 'phase-text success';
      
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
      await showDiskInfo(selectedBackupDisk.id, backupDiskInfo, logBackup);
      await checkVolumeInfo(selectedBackupDisk.id);
      logBackup('USB selected: ' + selectedBackupDisk.name + ' (' + selectedBackupDisk.size + ')', 'info');
    } else {
      selectedBackupDisk = null;
      backupDiskInfo.classList.remove('visible');
      filesystemNote.classList.add('hidden');
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
      await showDiskInfo(selectedDiagnoseDisk.id, diagnoseDiskInfo, logDiagnose);
      logDiagnose('USB selected: ' + selectedDiagnoseDisk.name + ' (' + selectedDiagnoseDisk.size + ')', 'info');
      
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
        logDiagnose('SMART: ' + (data.error_message || 'Not available'), 'info');
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
      
      logDiagnose('SMART Status: ' + data.health_status + ' (via ' + data.source + ')', 'success');
      
    } catch (err) {
      smartLoading.classList.add('hidden');
      smartUnavailable.classList.remove('hidden');
      smartUnavailableMsg.textContent = 'Fehler beim Laden der SMART-Daten: ' + err;
      logDiagnose('SMART error: ' + err, 'error');
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
        '⚠️ WARNING!',
        'All data on "' + selectedDiagnoseDisk.name + '" (' + selectedDiagnoseDisk.id + ') will be PERMANENTLY deleted!\n\nContinue?',
        'Yes, delete',
        'Cancel'
      );
      
      if (!confirmed) {
        logDiagnose('Test cancelled', 'warning');
        return;
      }
    }

    // Request password for raw device access
    let password;
    try {
      password = await requestPassword('Zum Zugriff auf den USB-Stick werden Administrator-Rechte benötigt.\n\nBitte geben Sie Ihr macOS-Passwort ein:');
    } catch (err) {
      logDiagnose('Password prompt cancelled', 'warning');
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
    
    const modeNames = { surface: 'Surface Scan', full: 'Full Test', speed: 'Speed Test' };
    logDiagnose('Starting ' + modeNames[mode] + '...', 'info');
    diagnosePhase.textContent = 'Initializing...';
    diagnosePhase.className = 'phase-text';
    
    try {
      let result;
      logDiagnose('Calling test function: ' + mode, 'info');
      
      if (mode === 'surface') {
        result = await invoke('diagnose_surface_scan', {
          diskId: selectedDiagnoseDisk.id,
          password: password
        });
      } else if (mode === 'full') {
        logDiagnose('Invoking diagnose_full_test...', 'info');
        result = await invoke('diagnose_full_test', {
          diskId: selectedDiagnoseDisk.id,
          password: password
        });
        logDiagnose('diagnose_full_test returned', 'info');
      } else if (mode === 'speed') {
        result = await invoke('diagnose_speed_test', {
          diskId: selectedDiagnoseDisk.id,
          password: password
        });
      }
      
      // Display results
      if (result.success) {
        logDiagnose('✓ ' + result.message, 'success');
        diagnosePhase.textContent = '✓ Test completed!';
        diagnosePhase.className = 'phase-text success';
        statsSummaryBadge.textContent = '✓ OK';
        statsSummaryBadge.className = 'status-badge passed';
        statsSummaryBadge.classList.remove('hidden');
      } else {
        logDiagnose('✗ ' + result.message, 'error');
        diagnosePhase.textContent = '✗ Test failed!';
        diagnosePhase.className = 'phase-text error';
        statsSummaryBadge.textContent = '✗ Errors';
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
        logDiagnose('Bad sectors found: ' + result.bad_sectors.slice(0, 20).join(', ') + 
                    (result.bad_sectors.length > 20 ? '... and ' + (result.bad_sectors.length - 20) + ' more' : ''), 'warning');
      }
      
      diagnoseProgressFill.style.width = '100%';
      diagnoseProgressText.textContent = '100%';
      
      isDiagnosing = false;
      diagnoseBtn.disabled = false;
      cancelDiagnoseBtn.disabled = true;
      loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
    } catch (err) {
      if (diagnoseCancelled) {
        logDiagnose('✗ Test cancelled', 'warning');
        diagnosePhase.textContent = 'Cancelled';
        diagnosePhase.className = 'phase-text error';
      } else {
        logDiagnose('Error: ' + err, 'error');
        diagnosePhase.textContent = 'Error!';
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
      logDiagnose('Cancelling...', 'warning');
    } catch (err) {
      logDiagnose('Cancel error: ' + err, 'error');
    }
  });

  // Listen for progress events
  listen('progress', function(event) {
    const percent = event.payload.percent;
    const status = event.payload.status;
    const operation = event.payload.operation;
    
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
    } else if (phase === 'error') {
      burnPhase.textContent = '✗ Verification failed!';
      burnPhase.className = 'phase-text error';
      burnEta.textContent = '';
    }
  });

  // Listen for diagnose progress events
  listen('diagnose_progress', function(event) {
    const payload = event.payload;
    diagnoseProgressFill.style.width = payload.percent + '%';
    diagnoseProgressText.textContent = payload.percent + '%';
    diagnoseEta.textContent = calculateEta(diagnoseStartTime, payload.percent);
    diagnosePhase.textContent = payload.phase + ': ' + payload.status;
    
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
  logDiagnose('USB Diagnose ready', 'info');
  loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
  loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
  loadDisks(diagnoseDiskSelect, diagnoseDiskInfo, logDiagnose);
  
  // Start window state tracking
  initWindowStateTracking();
});
