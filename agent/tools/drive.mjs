// Testfahrer: schreibt als Testkonto in das 1:1 mit dem Agenten und protokolliert,
// was ein Mensch im Chat sehen würde (Tippen, Statusnachricht, Bearbeitungen, Antwort).
// Aufruf: OBS_USER=… OBS_PASS=… AGENT_USER=… NC_URL=… node drive.mjs "<Nachricht>" [Sekunden]
const { NC_URL, OBS_USER, OBS_PASS, AGENT_USER } = process.env;
const text = process.argv[2] ?? 'Hallo';
const watchMs = Number(process.argv[3] ?? 25) * 1000;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const auth = 'Basic ' + Buffer.from(`${OBS_USER}:${OBS_PASS}`).toString('base64');
async function ocs(method, path, body, timeoutMs = 40000) {
  const res = await fetch(`${NC_URL}/ocs/v2.php/apps/spreed/api${path}`, { method, headers: { Authorization: auth, 'OCS-APIRequest': 'true', Accept: 'application/json', ...(body ? { 'Content-Type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(timeoutMs) });
  let data = null; if (res.status !== 304) { try { data = (await res.json()).ocs?.data ?? null; } catch {} }
  return { status: res.status, data, lastGiven: res.headers.get('x-chat-last-given') };
}
let token = process.env.ROOM_TOKEN;
if (process.env.MAKE_GROUP) {
  const g = await ocs('POST', '/v4/room', { roomType: 2, roomName: process.env.MAKE_GROUP });
  token = g.data.token;
  const add = await ocs('POST', `/v4/room/${token}/participants`, { newParticipant: AGENT_USER, source: 'users' });
  console.log('Gruppe angelegt', token, 'Agent hinzugefügt: HTTP', add.status);
}
if (process.env.DELETE_ROOM) { const d = await ocs('DELETE', `/v4/room/${process.env.DELETE_ROOM}`); console.log('Raum gelöscht: HTTP', d.status); process.exit(0); }
if (!token) { const room = await ocs('POST', '/v4/room', { roomType: 1, invite: AGENT_USER }); token = room.data.token; }
const start = await ocs('GET', `/v1/chat/${token}?lookIntoFuture=0&limit=1`);
let last = Number(start.lastGiven ?? start.data?.[0]?.id ?? 0);
let t0 = performance.now();
const rel = () => ((performance.now() - t0) / 1000).toFixed(2).padStart(6) + 's';
const events = [];
const log = (kind, detail) => { events.push(`${rel()}  ${kind.padEnd(10)} ${detail}`); };
// Signaling für die Tipp-Anzeige
const s = (await ocs('GET', `/v3/signaling/settings?token=${token}`)).data;
const active = await ocs('POST', `/v4/room/${token}/participants/active`, { force: true });
const ws = new WebSocket(s.server.replace(/^http/, 'ws').replace(/\/?$/, '/') + 'spreed');
await new Promise((r, j) => { ws.onopen = r; ws.onerror = () => j(new Error('ws')); });
ws.onmessage = (ev) => {
  const m = JSON.parse(ev.data);
  if (m.type === 'hello') ws.send(JSON.stringify({ id: '2', type: 'room', room: { roomid: token, sessionid: active.data.sessionId } }));
  if (m.type === 'message' && /Typing$/.test(m.message?.data?.type ?? '')) log('TIPPEN', m.message.data.type === 'startedTyping' ? 'an' : 'aus');
};
ws.send(JSON.stringify({ id: '1', type: 'hello', hello: { version: '2.0', auth: { url: `${NC_URL}/ocs/v2.php/apps/spreed/api/v3/signaling/backend`, params: s.helloAuthParams['2.0'] } } }));
await sleep(1200);
let stop = false;
const snip = (t) => JSON.stringify(String(t ?? '').replace(/\s+/g, ' ').slice(0, 110)) + (String(t ?? '').length > 110 ? `… (${String(t).length} Zeichen)` : '');
const poller = (async () => {
  while (!stop) {
    const r = await ocs('GET', `/v1/chat/${token}?lookIntoFuture=1&timeout=5&lastKnownMessageId=${last}`).catch(() => null);
    if (!r) continue; if (r.lastGiven) last = Number(r.lastGiven);
    for (const m of r.data ?? []) {
      if (m.actorId === OBS_USER && !m.systemMessage) continue;
      if (m.systemMessage === 'message_edited') log('BEARBEITET', `#${m.parent?.id} ${snip(m.parent?.message)}`);
      else if (m.systemMessage === 'message_deleted') log('GELÖSCHT', `#${m.parent?.id}`);
      else if (m.systemMessage) log('system', m.systemMessage);
      else log('NACHRICHT', `#${m.id} ${m.silent ? '(still) ' : ''}${m.messageType !== 'comment' ? '[' + m.messageType + '] ' : ''}${snip(m.message)}${m.messageParameters?.file ? ' [Datei: ' + m.messageParameters.file.name + ']' : ''}`);
    }
  }
})();
t0 = performance.now();
if (text.startsWith('@datei:')) {
  // Datei hochladen und in den Raum teilen
  const name = text.slice(7); const { readFileSync } = await import('node:fs'); const buf = readFileSync(name); const base = name.split('/').pop();
  await fetch(`${NC_URL}/remote.php/dav/files/${OBS_USER}/Talk`, { method: 'MKCOL', headers: { Authorization: auth } });
  await fetch(`${NC_URL}/remote.php/dav/files/${OBS_USER}/Talk/${encodeURIComponent(base)}`, { method: 'PUT', headers: { Authorization: auth }, body: buf });
  const sh = await fetch(`${NC_URL}/ocs/v2.php/apps/files_sharing/api/v1/shares`, { method: 'POST', headers: { Authorization: auth, 'OCS-APIRequest': 'true', Accept: 'application/json', 'Content-Type': 'application/json' }, body: JSON.stringify({ shareType: 10, shareWith: token, path: `/Talk/${base}`, talkMetaData: JSON.stringify({ caption: process.argv[4] ?? 'Was steht in dieser Datei?' }) }) });
  log('GESENDET', `Datei ${base} (HTTP ${sh.status})`);
} else {
  const sent = await ocs('POST', `/v1/chat/${token}`, { message: text });
  log('GESENDET', `#${sent.data?.id} ${snip(text)}`);
}
if (process.env.SECOND) { await sleep(Number(process.env.SECOND_DELAY ?? 2500)); const s2 = await ocs('POST', `/v1/chat/${token}`, { message: process.env.SECOND }); log('GESENDET', `#${s2.data?.id} ${snip(process.env.SECOND)}`); }
await sleep(watchMs);
stop = true; await Promise.race([poller, sleep(6000)]);
try { ws.close(); } catch {}
await ocs('DELETE', `/v4/room/${token}/participants/active`).catch(() => {});
console.log(`Raum ${token}`); console.log(events.join('\n'));
process.exit(0);
