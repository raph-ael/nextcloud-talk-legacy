// Messung: Bekommt ein Konto mit aktiver Sitzung im Raum (participants/active + Signaling-Beitritt)
// für neue Nachrichten in diesem Raum weiter eine Benachrichtigung? Testkonten A (Empfänger) und B (Absender).
import { createNcClient } from '../openclaw-nctalk/src/nc-client.js';
import { createRoomSignaling } from '../openclaw-nctalk/src/signaling.js';
const { NC_URL, A_USER, A_PASS, B_USER, B_PASS, ROOM } = process.env;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const a = createNcClient({ baseUrl: NC_URL.replace(/\/+$/, ''), user: A_USER, password: A_PASS });
const b = createNcClient({ baseUrl: NC_URL.replace(/\/+$/, ''), user: B_USER, password: B_PASS });
const hA = { Authorization: 'Basic ' + Buffer.from(`${A_USER}:${A_PASS}`).toString('base64'), 'OCS-APIRequest': 'true', Accept: 'application/json' };
const nUrl = `${NC_URL}/ocs/v2.php/apps/notifications/api/v2/notifications`;
const chatNotifs = async () => ((await (await fetch(nUrl, { headers: hA })).json()).ocs.data ?? []).filter((n) => n.app === 'spreed' && n.object_type === 'chat' && String(n.object_id).startsWith(ROOM));
const clear = async () => { for (const n of await chatNotifs()) await fetch(`${nUrl}/${n.notification_id}`, { method: 'DELETE', headers: hA }); };
async function round(label, silent) {
  await clear();
  // Lesestand von A auf den neuesten Stand, damit jede Runde mit "nichts ungelesen" beginnt.
  const latest = (await a.fetchRecentMessages(ROOM, 1))[0]?.id; if (latest) await a.setReadMarker(ROOM, latest);
  await sleep(500);
  const sent = await b.sendMessage(ROOM, { message: `Push-Messung: ${label}`, ...(silent ? { silent: true } : {}) });
  await sleep(4000);
  const n = await chatNotifs();
  console.log(`${label.padEnd(46)} -> Benachrichtigung: ${n.length ? 'JA' : 'nein'}`);
  return sent.id;
}
await round('1 A ohne Sitzung im Raum');
await round('2 A ohne Sitzung, zweite Nachricht');
const sig = createRoomSignaling({ client: a, token: ROOM });
await sig.start(); await sleep(1500);
await round('3 A mit aktiver Sitzung (Signaling im Raum)');
await sig.stop(); await sleep(1500);
await round('4 A 2 s nach Verlassen der Sitzung');
await sleep(40000);
await round('5 A 45 s nach Verlassen der Sitzung');
await round('6 A ohne Sitzung, stille Nachricht', true);
await clear();
const recent = await b.fetchRecentMessages(ROOM, 12);
for (const m of recent) if (m.actorId === B_USER && String(m.message).startsWith('Push-Messung')) await b.deleteMessage(ROOM, m.id).catch(() => {});
process.exit(0);
