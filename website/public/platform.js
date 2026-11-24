// Merkt sich das Betriebssystem als data-os am <html>-Element. styles.css zeigt damit den
// passenden Download als Hauptknopf und hebt dessen Kachel hervor. Das Skript steht im <head>,
// damit die Seite gleich richtig erscheint und nichts nachträglich verrutscht.
// Ohne dieses Skript (oder bei unbekanntem System) bleibt der allgemeine Knopf, und alle
// Kacheln sehen gleich aus. Läuft nur im Browser, sendet nichts.
(function () {
  'use strict';

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

  if (os) document.documentElement.setAttribute('data-os', os);
})();
