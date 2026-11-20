// Messung: Wann erzeugt eine Reaktion (B reagiert auf eine Nachricht von A) bei A eine Benachrichtigung?
// Variiert die Benachrichtigungsstufe von A im Raum und ob A eine aktive Sitzung hat. Stellt die Stufe wieder her.
import { createNcClient } from '../openclaw-nctalk/src/nc-client.js';
import { createRoomSignaling } from '../openclaw-nctalk/src/signaling.js';
const { NC_URL, A_USER, A_PASS, B_USER, B_PASS, ROOM } = process.env;
const base = NC_URL.replace(/\/+$/, '');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const a = createNcClient({ baseUrl: base, user: A_USER, password: A_PASS });
const b = createNcClient({ baseUrl: base, user: B_USER, password: B_PASS });
const hA = { Authorization: 'Basic ' + Buffer.from(`${A_USER}:${A_PASS}`).toString('base64'), 'OCS-APIRequest': 'true', Accept: 'application/json', 'Content-Type': 'application/json' };
const nUrl = `${base}/ocs/v2.php/apps/notifications/api/v2/notifications`;
const notifs = async () => ((await (await fetch(nUrl, { headers: hA })).json()).ocs.data ?? []).filter((n) => n.app === 'spreed' && String(n.object_id).startsWith(ROOM));
const clear = async () => { for (const n of await notifs()) await fetch(`${nUrl}/${n.notification_id}`, { method: 'DELETE', headers: hA }); };
const setLevel = async (level) => (await fetch(`${base}/ocs/v2.php/apps/spreed/api/v4/room/${ROOM}/notify`, { method: 'POST', headers: hA, body: JSON.stringify({ level }) })).status;
const original = (await a.getRoom(ROOM)).notificationLevel;
console.log('Stufe von A vorher:', original, '(0 Standard, 1 immer, 2 nur Erwähnungen, 3 nie)');
const mine = [];
async function round(label, emoji = '👀') {
  const msg = await a.sendMessage(ROOM, { message: `Reaktions-Messung: ${label}`, silent: true });
  mine.push(msg.id);
  await sleep(1500); // A ist danach nicht mehr "gerade aktiv" im Sinne einer Sitzung, hat aber eben geschrieben
  await clear();
  await b.addReaction(ROOM, msg.id, emoji);
  await sleep(3500);
  const n = await notifs();
  console.log(`${label.padEnd(52)} -> Benachrichtigung: ${n.length ? 'JA' : 'nein'}`);
  await b.removeReaction(ROOM, msg.id, emoji).catch(() => {});
}
try {
  await setLevel(1); await round('Stufe 1 (immer), keine Sitzung');
  await setLevel(2); await round('Stufe 2 (nur Erwähnungen), keine Sitzung');
  await setLevel(3); await round('Stufe 3 (nie), keine Sitzung');
  await setLevel(1);
  const sig = createRoomSignaling({ client: a, token: ROOM });
  await sig.start(); await sleep(1500);
  await round('Stufe 1 (immer), A mit aktiver Sitzung im Raum');
  await sig.stop();
} finally {
  await setLevel(original === 0 ? 1 : original);
  console.log('Stufe von A wiederhergestellt:', (await a.getRoom(ROOM)).notificationLevel);
  await clear();
  for (const id of mine) await a.deleteMessage(ROOM, id).catch(() => {});
}
process.exit(0);
