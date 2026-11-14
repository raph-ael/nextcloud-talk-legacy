// Startseite: zeigt den Download für das eigene Betriebssystem als Hauptknopf.
// Ohne dieses Skript (oder bei unbekanntem System) bleiben alle Plattformen sichtbar.
// Läuft nur im Browser, sendet nichts.
(function () {
  'use strict';
  var box = document.getElementById('get');
  if (!box) return;

  // userAgentData gibt es nur in Chromium-Browsern; sonst bleibt der User-Agent.
  var uad = navigator.userAgentData && navigator.userAgentData.platform;
  var ua = navigator.userAgent || '';
  var s = uad || ua;
  var os = null;

  if (/android/i.test(s)) os = 'android';
  // iPadOS meldet sich als Mac, hat aber einen Touchscreen.
  else if (/iphone|ipad|ipod|^ios$/i.test(s) || (/mac/i.test(s) && navigator.maxTouchPoints > 1)) os = 'ios';
  else if (/cros|chrome ?os|chromium ?os/i.test(s)) os = null; // Android oder Linux: selbst wählen
  else if (/win/i.test(s)) os = 'windows';
  else if (/mac/i.test(s)) os = 'mac';
  else if (/linux/i.test(s)) os = 'linux'; // nicht X11: das melden auch die BSDs

  var item = os && box.querySelector('[data-platform="' + os + '"]');
  if (!item) return;
  item.classList.add('is-match');
  box.classList.add('is-detected');
  var more = box.querySelector('[data-more]');
  if (more) more.textContent = more.getAttribute('data-more');
})();
