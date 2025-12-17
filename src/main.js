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
      passwordReject('Kein Passwort eingegeben');
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
  const burnPhase = document.getElementById('burn-phase');
  const burnLog = document.getElementById('burn-log');

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
  const backupLog = document.getElementById('backup-log');

  // Tab switching
  tabs.forEach(tab => {
    tab.addEventListener('click', () => {
      tabs.forEach(t => t.classList.remove('active'));
      tabContents.forEach(c => c.classList.remove('active'));
      tab.classList.add('active');
      document.getElementById(tab.dataset.tab + '-tab').classList.add('active');
    });
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

  // Reset burn state to initial (silent = no disk reload log)
  function resetBurnState(silent) {
    isBurning = false;
    burnProgressFill.style.width = '0%';
    burnProgressText.textContent = '0%';
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
    backupProgressFill.style.width = '0%';
    backupProgressText.textContent = '0%';
    cancelBackupBtn.disabled = true;
    updateBackupButton();
    if (!silent) {
      loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
    } else {
      loadDisksSilent(backupDiskSelect, backupDiskInfo);
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
      password = await requestPassword('Administrator privileges required to write to USB drive.\n\nPlease enter your macOS password:');
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
    burnBtn.disabled = true;
    cancelBurnBtn.disabled = false;
    burnProgressFill.style.width = '0%';
    burnProgressText.textContent = '0%';
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
      
      // Show macOS disk warning dialog
      const { message } = window.__TAURI__.dialog;
      await message(window.i18n.t('messages.macOsDiskWarning'), {
        title: window.i18n.t('messages.macOsDiskWarningTitle'),
        kind: 'info'
      });
      
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
        password = await requestPassword('Administrator privileges required to read USB drive.\n\nPlease enter your macOS password:');
      } catch (err) {
        logBackup('Password prompt cancelled', 'warning');
        return;
      }
    }

    isBackingUp = true;
    backupCancelled = false;
    backupBtn.disabled = true;
    cancelBackupBtn.disabled = false;
    backupProgressFill.style.width = '0%';
    backupProgressText.textContent = '0%';
    
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

  // Listen for progress events
  listen('progress', function(event) {
    const percent = event.payload.percent;
    const status = event.payload.status;
    const operation = event.payload.operation;
    
    if (operation === 'burn') {
      burnProgressFill.style.width = percent + '%';
      burnProgressText.textContent = percent + '%';
      // Don't log every progress update, only significant ones
      if (status.indexOf('✓') >= 0 || status.indexOf('FEHLER') >= 0) {
        logBurn(status, status.indexOf('FEHLER') >= 0 ? 'error' : 'success');
      }
    } else if (operation === 'backup') {
      backupProgressFill.style.width = percent + '%';
      backupProgressText.textContent = percent + '%';
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
      logBurn('Starting verification...', 'info');
    } else if (phase === 'success') {
      burnPhase.textContent = '✓ Successfully completed!';
      burnPhase.className = 'phase-text success';
    } else if (phase === 'error') {
      burnPhase.textContent = '✗ Verification failed!';
      burnPhase.className = 'phase-text error';
    }
  });

  // Listen for menu events
  listen('menu-action', function(event) {
    const action = event.payload;
    switch (action) {
      case 'refresh':
        loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
        loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
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
      case 'start_burn':
        if (!burnBtn.disabled) burnBtn.click();
        break;
      case 'start_backup':
        if (!backupBtn.disabled) backupBtn.click();
        break;
      case 'cancel_action':
        if (!cancelBurnBtn.disabled) cancelBurnBtn.click();
        if (!cancelBackupBtn.disabled) cancelBackupBtn.click();
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
  loadDisks(burnDiskSelect, burnDiskInfo, logBurn);
  loadDisks(backupDiskSelect, backupDiskInfo, logBackup);
  
  // Start window state tracking
  initWindowStateTracking();
});
