// Probe: Was liefert der Signaling-Server (HPB) einer Sitzung, wenn im Raum geschrieben,
// bearbeitet, gelöscht oder reagiert wird? Einmal im Raum (joined), einmal nur verbunden (lobby).
// Aufruf wie talk-spike.mjs (Zugangsdaten nur über die Umgebung).
const { NC_URL, AGENT_USER, AGENT_PASS, OBS_USER, OBS_PASS } = process.env;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const t00 = performance.now();
const rel = () => Math.round(performance.now() - t00);
function client(user, pass) {
  const auth = 'Basic ' + Buffer.from(`${user}:${pass}`).toString('base64');
  return async (method, path, body) => {
    const res = await fetch(`${NC_URL}/ocs/v2.php/apps/spreed/api${path}`, {
      method, headers: { Authorization: auth, 'OCS-APIRequest': 'true', Accept: 'application/json', ...(body ? { 'Content-Type': 'application/json' } : {}) },
      body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(20000),
    });
    let data = null; const text = await res.text();
    try { data = JSON.parse(text).ocs?.data ?? null; } catch {}
    return { status: res.status, data };
  };
}
// Struktur eines Ereignisses ohne Inhalte: Schlüssel und Typen, Strings gekürzt
function shape(v, depth = 0) {
  if (v === null || typeof v !== 'object') return typeof v === 'string' ? (v.length > 24 ? `str(${v.length})` : v) : v;
  if (Array.isArray(v)) return depth > 4 ? '[…]' : v.slice(0, 2).map((x) => shape(x, depth + 1));
  const o = {};
  for (const [k, x] of Object.entries(v)) {
    if (/token|ticket|password|secret|sessionid|resumeid|nextcloud|userid|displayname|actorId|actorDisplayName|message$|^user$/i.test(k) && typeof x !== 'object') { o[k] = '…'; continue; }
    o[k] = depth > 5 ? '…' : shape(x, depth + 1);
  }
  return o;
}
async function connect(ocs, roomToken, label, join) {
  const s = (await ocs('GET', `/v3/signaling/settings?token=${roomToken}`)).data;
  const ws = new WebSocket(s.server.replace(/^http/, 'ws').replace(/\/?$/, '/') + 'spreed');
  await new Promise((res, rej) => { ws.onopen = res; ws.onerror = () => rej(new Error('ws')); });
  const pending = new Map(); let n = 1;
  const log = [];
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); return; }
    log.push({ t: rel(), ...shape(m) });
  };
  const req = (msg) => new Promise((res) => { const id = String(n++); pending.set(id, res); ws.send(JSON.stringify({ id, ...msg })); });
  const hello = await req({ type: 'hello', hello: { version: '2.0', auth: { url: `${NC_URL}/ocs/v2.php/apps/spreed/api/v3/signaling/backend`, params: s.helloAuthParams['2.0'] } } });
  const features = hello.hello?.server?.features ?? [];
  if (join) {
    const active = await ocs('POST', `/v4/room/${roomToken}/participants/active`, { force: true });
    await req({ type: 'room', room: { roomid: roomToken, sessionid: active.data.sessionId } });
  }
  return { log, features, label, async close() { try { ws.close(); } catch {} if (join) await ocs('DELETE', `/v4/room/${roomToken}/participants/active`); } };
}
const agent = client(AGENT_USER, AGENT_PASS), obs = client(OBS_USER, OBS_PASS);
const room = await agent('POST', '/v4/room', { roomType: 1, invite: OBS_USER });
const token = room.data.token;
await agent('POST', `/v1/chat/${token}`, { message: 'Probe: Start', silent: true });
let joined, lobby;
const marks = [];
try {
  joined = await connect(obs, token, 'joined', true);
  lobby = await connect(obs, token, 'lobby', false);
  await sleep(1500);
  const mark = (name) => marks.push({ t: rel(), name });
  mark('send'); const sent = await agent('POST', `/v1/chat/${token}`, { message: 'Probe: Nachricht', silent: true }); await sleep(1500);
  mark('edit'); await agent('PUT', `/v1/chat/${token}/${sent.data.id}`, { message: 'Probe: bearbeitet' }); await sleep(1500);
  mark('react'); await agent('POST', `/v1/reaction/${token}/${sent.data.id}`, { reaction: '👍' }); await sleep(1500);
  mark('delete'); await agent('DELETE', `/v1/chat/${token}/${sent.data.id}`); await sleep(1500);
  mark('send-loud'); await agent('POST', `/v1/chat/${token}`, { message: 'Probe: Ende' }); await sleep(1500);
  console.log(JSON.stringify({ features: joined.features, marks, joined: joined.log, lobby: lobby.log }, null, 1));
} finally {
  await joined?.close().catch(() => {}); await lobby?.close().catch(() => {});
  await agent('DELETE', `/v4/room/${token}/participants/self`).catch(() => {});
  await obs('DELETE', `/v4/room/${token}/participants/self`).catch(() => {});
}
process.exit(0);
