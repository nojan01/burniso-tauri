// Internationalization & Theme Module
const i18n = {
  currentLang: 'de',
  currentTheme: 'dark',
  translations: {},
  
  async init() {
    // Load saved language preference
    const savedLang = localStorage.getItem('language');
    this.currentLang = savedLang || (navigator.language.startsWith('de') ? 'de' : 'en');
    await this.loadTranslations(this.currentLang);
    
    // Load saved theme preference (default: system preference or dark)
    const savedTheme = localStorage.getItem('theme');
    if (savedTheme) {
      this.currentTheme = savedTheme;
    } else {
      // Check system preference
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      this.currentTheme = prefersDark ? 'dark' : 'light';
    }
    this.applyTheme(this.currentTheme);
    
    // Set menu language on startup
    this.updateMenuLanguage(this.currentLang);
  },
  
  async loadTranslations(lang) {
    try {
      const response = await fetch(`i18n/${lang}.json`);
      this.translations = await response.json();
      this.currentLang = lang;
      localStorage.setItem('language', lang);
    } catch (e) {
      console.error('Failed to load translations:', e);
      // Fallback to German
      if (lang !== 'de') {
        await this.loadTranslations('de');
      }
    }
  },
  
  t(key) {
    const keys = key.split('.');
    let value = this.translations;
    for (const k of keys) {
      value = value?.[k];
    }
    return value || key;
  },
  
  updateMenuLanguage(lang) {
    if (window.__TAURI__?.core?.invoke) {
      window.__TAURI__.core.invoke('set_menu_language', { lang: lang });
    }
  },
  
  async setLanguage(lang) {
    await this.loadTranslations(lang);
    this.applyTranslations();
    // Rebuild menu with new language
    this.updateMenuLanguage(lang);
  },
  
  applyTranslations() {
    // Apply translations to all elements with data-i18n attribute
    document.querySelectorAll('[data-i18n]').forEach(el => {
      const key = el.getAttribute('data-i18n');
      const text = this.t(key);
      if (text !== key) {
        el.textContent = text;
      }
    });
    
    // Apply to placeholders
    document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
      const key = el.getAttribute('data-i18n-placeholder');
      const text = this.t(key);
      if (text !== key) {
        el.placeholder = text;
      }
    });
    
    // Apply to titles
    document.querySelectorAll('[data-i18n-title]').forEach(el => {
      const key = el.getAttribute('data-i18n-title');
      const text = this.t(key);
      if (text !== key) {
        el.title = text;
      }
    });
    
    // Update select dropdowns - find first option (placeholder) and update
    document.querySelectorAll('select').forEach(select => {
      const firstOption = select.querySelector('option[value=""]');
      if (firstOption) {
        // Use burn.selectUsbPlaceholder for USB selects
        firstOption.textContent = this.t('burn.selectUsbPlaceholder');
      }
    });
    
    // Update document title
    document.title = this.t('app.title');
  },
  
  applyTheme(theme) {
    this.currentTheme = theme;
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('theme', theme);
  },
  
  setTheme(theme) {
    this.applyTheme(theme);
  },
  
  toggleTheme() {
    const newTheme = this.currentTheme === 'dark' ? 'light' : 'dark';
    this.applyTheme(newTheme);
    return newTheme;
  }
};

// Export for use in main.js
window.i18n = i18n;
