// Agenten-Attrappe für Client-Tests: verhält sich im Chat genau wie das OpenClaw-Plugin "nctalk"
// (Tipp-Anzeige über den Signaling-Server, stille Statusnachricht, Bearbeitungen, Löschen, Antwort),
// aber ohne Gateway und ohne Modell. Läuft als ein beliebiges Konto in genau einem Raum.
//
//   NC_URL=… SIM_USER=… SIM_PASS=… node agent-sim.mjs <Raumtoken> [Sekunden]
//
// Stichworte in der eingehenden Nachricht steuern den Ablauf:
//   "kurz"     Tippen, dann sofort eine kurze Antwort (keine Statusnachricht)
//   "partial"  die Antwort wächst per Bearbeiten mit und ist am Ende die fertige Nachricht
//   "warte N"  N Sekunden nur tippen, dann antworten
//   "fehler"   wie der Standardablauf, aber statt der Antwort kommt eine Fehlermeldung (normale Nachricht);
//              die Statusnachricht wird danach ebenfalls gelöscht
//   "abbruch"  die Statusnachricht bleibt stehen und es kommt nichts mehr (Gateway mitten im Lauf beendet)
//   "quittung" sofort beim Annehmen eine stille Statusnachricht "👀", die dann zum Live-Status wird
//   sonst      "progress": stille Statusnachricht, die fortgeschrieben wird; Antwort als neue
//              Nachricht; Statusnachricht wird danach gelöscht
import { createNcClient } from '../openclaw-nctalk/src/nc-client.js';
import { createRoomSignaling } from '../openclaw-nctalk/src/signaling.js';
const { NC_URL, SIM_USER, SIM_PASS } = process.env;
const token = process.argv[2]; const seconds = Number(process.argv[3] ?? 120);
if (!NC_URL || !SIM_USER || !SIM_PASS || !token) { console.error('Aufruf: NC_URL SIM_USER SIM_PASS gesetzt; node agent-sim.mjs <Raumtoken> [Sekunden]'); process.exit(2); }
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const client = createNcClient({ baseUrl: NC_URL.replace(/\/+$/, ''), user: SIM_USER, password: SIM_PASS });
const self = await client.whoami();
let cursor = Number((await client.fetchRecentMessages(token, 1))[0]?.id ?? 0);
let busy = Promise.resolve();
const log = (...a) => console.log(new Date().toLocaleTimeString('de-DE'), ...a);
const sig = createRoomSignaling({ client, token, onRefresh: () => void sync(), onUp: () => log('Signaling verbunden') });
// Die Tipp-Anzeige verfällt bei den Clients nach etwa 15 s; deshalb alle 8 s auffrischen.
let typingTimer = null;
const typing = (on) => { clearInterval(typingTimer); typingTimer = null; sig.setTyping(on); if (on) typingTimer = setInterval(() => sig.setTyping(true), 8000); };

async function answer(text, id) {
  const t = text.toLowerCase();
  typing(true);
  const wait = /warte (\d+)/.exec(t);
  if (wait) await sleep(Number(wait[1]) * 1000);
  if (t.includes('kurz') || wait) {
    await sleep(1200);
    typing(false);
    await client.sendMessage(token, { message: 'Kurze Antwort.' });
    return;
  }
  const final = 'Das ist die fertige Antwort des Agenten.\n\n- **Punkt eins** ist erledigt\n- Punkt zwei auch\n\n`Code` geht ebenfalls.';
  if (t.includes('partial')) {
    await sleep(1500);
    const words = final.split(/(?<=\s)/);
    let shown = words.slice(0, 4).join('');
    const sent = await client.sendMessage(token, { message: shown });
    for (let i = 4; i < words.length; i += 5) { await sleep(2500); shown = words.slice(0, i + 5).join(''); await client.editMessage(token, sent.id, shown.trim()); }
    typing(false);
    return;
  }
  let status;
  if (t.includes('quittung')) { status = await client.sendMessage(token, { message: '👀', silent: true }); await sleep(1600); await client.editMessage(token, status.id, 'Arbeitet …'); }
  else { await sleep(1500); status = await client.sendMessage(token, { message: 'Arbeitet …', silent: true }); }
  const steps = ['Arbeitet …\n🛠️ Exec: sucht in den Bestellungen', 'Arbeitet …\n🛠️ Exec: sucht in den Bestellungen\n📖 Read: Bericht.md', 'Arbeitet …\n✅ Bestellungen geprüft\n▸ Antwort schreiben'];
  for (const step of steps) { await sleep(1600); await client.editMessage(token, status.id, step); }
  await sleep(1200);
  typing(false);
  if (t.includes('abbruch')) return;
  await client.sendMessage(token, { message: t.includes('fehler') ? '⚠️ Das hat nicht geklappt: Zeitüberschreitung beim Werkzeug.' : final });
  await client.deleteMessage(token, status.id);
}

async function sync() {
  busy = busy.then(async () => {
    const { messages, lastGiven } = await client.fetchNewMessages(token, { lastKnownMessageId: cursor });
    for (const m of messages) {
      cursor = Math.max(cursor, Number(m.id));
      if (m.systemMessage || m.actorId === self.id || m.messageType !== 'comment') continue;
      log('Nachricht von', m.actorId, `#${m.id}`);
      await client.setReadMarker(token, m.id).catch(() => {});
      await answer(String(m.message ?? ''), m.id).catch((err) => log('Fehler:', err.message));
    }
    if (lastGiven) cursor = Math.max(cursor, lastGiven);
  }).catch((err) => log('Fehler beim Lesen:', err.message));
  return busy;
}
// Wie das Plugin: Am Status-Emoji 🤖 erkennen Clients den Agenten. Am Ende wieder weg, damit das
// Testkonto für andere Tests ein gewöhnliches Konto bleibt. SIM_STATUS=0 lässt den Status in Ruhe.
const wearsStatus = process.env.SIM_STATUS !== '0' && (await client.setUserStatus({ icon: '🤖', message: 'KI-Agent' }).catch(() => false));
log(wearsStatus ? 'Status 🤖 gesetzt' : 'Status nicht gesetzt');
await sig.start();
log(`Agenten-Attrappe läuft als ${self.id} in ${token} für ${seconds} s`);
const poll = setInterval(() => void sync(), 5000);
await sleep(seconds * 1000);
clearInterval(poll); typing(false); await busy; await sig.stop();
if (wearsStatus) await client.clearUserStatus().catch(() => {});
log('beendet'); process.exit(0);
