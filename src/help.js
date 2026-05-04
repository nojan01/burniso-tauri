// Sprachumschaltung im Hilfe-Fenster (extern wegen strikter CSP).
(function () {
  function switchLang(lang) {
    document.querySelectorAll('.content').forEach(function (c) {
      c.classList.remove('active');
    });
    document.querySelectorAll('.lang-btn').forEach(function (b) {
      b.classList.remove('active');
    });
    var content = document.getElementById('content-' + lang);
    if (content) content.classList.add('active');
    var btn = document.querySelector('.lang-btn[data-lang="' + lang + '"]');
    if (btn) btn.classList.add('active');
  }

  document.addEventListener('DOMContentLoaded', function () {
    document.querySelectorAll('.lang-btn').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var lang = btn.getAttribute('data-lang');
        if (lang) switchLang(lang);
      });
    });
  });
})();
