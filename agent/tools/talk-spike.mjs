// Spike: Kann ein normales Nextcloud-Konto (der Agent) in Talk 24
//   1. eine Statusnachricht still senden und per Bearbeiten "live" fortschreiben,
//   2. die Tipp-Anzeige ohne Browser über den Signaling-Server (HPB) auslösen,
//   3. und kommt beides bei einem zweiten Konto (Beobachter) an – wie schnell?
//
// Aufruf (Zugangsdaten nur über die Umgebung, nichts wird ausgegeben):
//   NC_URL=… AGENT_USER=… AGENT_PASS=… OBS_USER=… OBS_PASS=… node talk-spike.mjs
// Aufräumen: Der dabei angelegte 1:1-Chat wird am Ende von beiden Seiten verlassen.

const { NC_URL, AGENT_USER, AGENT_PASS, OBS_USER, OBS_PASS } = process.env;
for (const k of ['NC_URL', 'AGENT_USER', 'AGENT_PASS', 'OBS_USER', 'OBS_PASS']) {
  if (!process.env[k]) { console.error(`fehlt: ${k}`); process.exit(2); }
}
const EDITS = Number(process.env.EDITS ?? 8);
const EDIT_INTERVAL_MS = Number(process.env.EDIT_INTERVAL_MS ?? 700);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => performance.now();
const ms = (t) => `${Math.round(t)} ms`;

function client(user, pass) {
  const auth = 'Basic ' + Buffer.from(`${user}:${pass}`).toString('base64');
  return async function ocs(method, path, body, { timeoutMs = 20000 } = {}) {
    const res = await fetch(`${NC_URL}/ocs/v2.php/apps/spreed/api${path}`, {
      method,
      headers: {
        Authorization: auth, 'OCS-APIRequest': 'true', Accept: 'application/json',
        ...(body ? { 'Content-Type': 'application/json' } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
      signal: AbortSignal.timeout(timeoutMs),
    });
    let data = null;
    if (res.status !== 304 && res.status !== 204) {
      const text = await res.text();
      try { data = JSON.parse(text).ocs?.data ?? null; } catch { data = text.slice(0, 200); }
    }
    return { status: res.status, data, lastGiven: res.headers.get('x-chat-last-given') };
  };
}

// Signaling-Verbindung (Standalone/HPB), Hello v2 mit dem Token aus den Signaling-Einstellungen.
async function signaling(ocs, roomToken, label, onMessage) {
  const settings = await ocs('GET', `/v3/signaling/settings?token=${roomToken}`);
  if (settings.status !== 200) throw new Error(`${label}: signaling/settings ${settings.status}`);
  const s = settings.data;
  const wsUrl = s.server.replace(/^http/, 'ws').replace(/\/?$/, '/') + 'spreed';
  const active = await ocs('POST', `/v4/room/${roomToken}/participants/active`, { force: true });
  if (active.status !== 200) throw new Error(`${label}: participants/active ${active.status}`);
  const ncSession = active.data.sessionId;

  const ws = new WebSocket(wsUrl);
  const peers = new Map(); // Signaling-Session -> userid
  let mySession = null;
  let nextId = 1;
  const pending = new Map();
  const request = (msg) => new Promise((resolve, reject) => {
    const id = String(nextId++);
    pending.set(id, resolve);
    setTimeout(() => pending.has(id) && (pending.delete(id), reject(new Error(`${label}: Zeitüberschreitung bei ${msg.type}`))), 10000);
    ws.send(JSON.stringify({ id, ...msg }));
  });
  await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = () => reject(new Error(`${label}: WebSocket-Fehler`)); });
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
    if (m.type === 'event' && m.event?.target === 'room') {
      for (const j of m.event.join ?? []) peers.set(j.sessionid, j.userid);
      for (const l of m.event.leave ?? []) peers.delete(l);
    }
    if (m.type === 'message') onMessage?.(m.message);
  };
  const hello = await request({
    type: 'hello',
    hello: { version: '2.0', auth: { url: `${NC_URL}/ocs/v2.php/apps/spreed/api/v3/signaling/backend`, params: s.helloAuthParams['2.0'] } },
  });
  if (hello.type !== 'hello') throw new Error(`${label}: hello abgelehnt: ${JSON.stringify(hello.error ?? hello).slice(0, 200)}`);
  mySession = hello.hello.sessionid;
  const room = await request({ type: 'room', room: { roomid: roomToken, sessionid: ncSession } });
  if (room.type !== 'room') throw new Error(`${label}: Raumbeitritt abgelehnt: ${JSON.stringify(room.error ?? room).slice(0, 200)}`);
  return {
    mySession: () => mySession,
    peers,
    features: hello.hello.server?.features ?? [],
    sendTyping(type) {
      let n = 0;
      for (const [sid] of peers) {
        if (sid === mySession) continue;
        ws.send(JSON.stringify({ type: 'message', message: { recipient: { type: 'session', sessionid: sid }, data: { type, to: sid } } }));
        n++;
      }
      return n;
    },
    async close() {
      try { ws.close(); } catch {}
      await ocs('DELETE', `/v4/room/${roomToken}/participants/active`);
    },
  };
}

const agent = client(AGENT_USER, AGENT_PASS);
const obs = client(OBS_USER, OBS_PASS);
const result = {};

// 0. Fähigkeiten des Servers aus Sicht des Agenten
{
  const res = await fetch(`${NC_URL}/ocs/v2.php/cloud/capabilities`, {
    headers: { Authorization: 'Basic ' + Buffer.from(`${AGENT_USER}:${AGENT_PASS}`).toString('base64'), 'OCS-APIRequest': 'true', Accept: 'application/json' },
  });
  const sp = (await res.json()).ocs.data.capabilities.spreed;
  result.talk = sp.version;
  result.canEdit = sp.features.includes('edit-messages');
  result.maxLength = sp.config.chat['max-length'];
  result.signalingMode = sp.config.signaling?.mode ?? '(nicht in den Fähigkeiten)';
}

// 1. Der Agent öffnet von sich aus ein 1:1 mit dem Beobachter
const room = await agent('POST', '/v4/room', { roomType: 1, invite: OBS_USER });
if (![200, 201].includes(room.status)) throw new Error(`1:1 anlegen: ${room.status} ${JSON.stringify(room.data).slice(0, 200)}`);
const token = room.data.token;
result.dmCreatedByAgent = room.status;
// Talk 24 nimmt die Gegenseite erst mit der ersten Nachricht in das 1:1 auf.
result.peerBeforeFirstMessage = (await obs('GET', `/v4/room/${token}`)).status;
await agent('POST', `/v1/chat/${token}`, { message: 'Hallo, ich bin der Agent. Das ist ein Test.', silent: true });
result.peerAfterFirstMessage = (await obs('GET', `/v4/room/${token}`)).status;
let cleanupDone = false;
async function cleanup(sigs = []) {
  if (cleanupDone) return; cleanupDone = true;
  for (const s of sigs) await s?.close().catch(() => {});
  await agent('DELETE', `/v4/room/${token}/participants/self`).catch(() => {});
  await obs('DELETE', `/v4/room/${token}/participants/self`).catch(() => {});
}

let agentSig, obsSig;
try {
  // Beobachter: Chat per Long Polling mitlesen und Zeitpunkte je Ereignis festhalten
  const seen = []; // {t, kind, id, parentId, text}
  let stopPoll = false;
  const start = await obs('GET', `/v1/chat/${token}?lookIntoFuture=0&limit=1`);
  let last = Number(start.lastGiven ?? start.data?.[0]?.id ?? 0);
  const poller = (async () => {
    while (!stopPoll) {
      const r = await obs('GET', `/v1/chat/${token}?lookIntoFuture=1&timeout=20&lastKnownMessageId=${last}`, null, { timeoutMs: 30000 }).catch(() => null);
      if (!r) continue;
      if (r.lastGiven) last = Number(r.lastGiven);
      for (const m of r.data ?? []) {
        seen.push({ t: now(), kind: m.systemMessage || 'message', id: m.id, parentId: m.parent?.id, text: (m.parent?.message ?? m.message ?? '').slice(0, 40) });
      }
    }
  })();
  const waitSeen = async (pred, timeoutMs = 8000) => {
    const t0 = now();
    while (now() - t0 < timeoutMs) { const hit = seen.find(pred); if (hit) return hit; await sleep(20); }
    return null;
  };

  // 2. Tipp-Anzeige über den Signaling-Server
  const typing = [];
  obsSig = await signaling(obs, token, 'Beobachter', (msg) => {
    if (msg.data?.type === 'startedTyping' || msg.data?.type === 'stoppedTyping') typing.push({ t: now(), type: msg.data.type, from: msg.sender?.userid });
  });
  agentSig = await signaling(agent, token, 'Agent');
  result.hpbFeatures = agentSig.features.length;
  await sleep(1500); // Beitrittsereignisse abwarten
  result.agentSeesPeers = [...agentSig.peers.values()];
  const tTyping = now();
  result.typingSentTo = agentSig.sendTyping('startedTyping');
  for (let i = 0; i < 100 && typing.length === 0; i++) await sleep(20);
  result.typingReceived = typing.length > 0;
  result.typingLatency = typing.length ? ms(typing[0].t - tTyping) : '—';
  result.typingFrom = typing[0]?.from;

  // 3. Statusnachricht still senden, dann fortlaufend bearbeiten
  let t0 = now();
  const sent = await agent('POST', `/v1/chat/${token}`, { message: '⏳ *arbeitet …*', silent: true, referenceId: crypto.randomUUID().replace(/-/g, '') });
  result.sendStatus = sent.status;
  result.sendLatency = ms(now() - t0);
  const statusId = sent.data.id;
  const firstSeen = await waitSeen((e) => e.kind === 'message' && e.id === statusId);
  result.sendDelivery = firstSeen ? ms(firstSeen.t - t0) : 'nicht angekommen';

  const editLat = [], deliverLat = [], editStatus = new Set();
  let text = '';
  const words = 'Hier entsteht die Antwort des Agenten Stück für Stück, so wie sie das Modell gerade schreibt, damit niemand auf einen leeren Bildschirm wartet.'.split(' ');
  for (let i = 1; i <= EDITS; i++) {
    text = words.slice(0, Math.ceil((words.length * i) / EDITS)).join(' ');
    const before = seen.length;
    t0 = now();
    const e = await agent('PUT', `/v1/chat/${token}/${statusId}`, { message: text });
    editStatus.add(e.status);
    editLat.push(now() - t0);
    const hit = await waitSeen((x) => seen.indexOf(x) >= before && x.kind === 'message_edited' && x.parentId === statusId, 5000);
    if (hit) deliverLat.push(hit.t - t0);
    await sleep(Math.max(0, EDIT_INTERVAL_MS - (now() - t0)));
  }
  const avg = (a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : NaN);
  result.edits = EDITS;
  result.editStatus = [...editStatus];
  result.editLatencyAvg = ms(avg(editLat));
  result.editLatencyMax = ms(Math.max(...editLat));
  result.editsDelivered = `${deliverLat.length}/${EDITS}`;
  result.editDeliveryAvg = deliverLat.length ? ms(avg(deliverLat)) : '—';
  result.editDeliveryMax = deliverLat.length ? ms(Math.max(...deliverLat)) : '—';
  agentSig.sendTyping('stoppedTyping');

  // 4. Variante "Statusnachricht löschen, Antwort normal senden" (löst die eine Benachrichtigung aus)
  t0 = now();
  const del = await agent('DELETE', `/v1/chat/${token}/${statusId}`);
  result.deleteStatus = del.status;
  const delSeen = await waitSeen((x) => x.kind === 'message_deleted' && x.parentId === statusId, 5000);
  result.deleteDelivery = delSeen ? ms(delSeen.t - t0) : 'nicht angekommen';
  const fin = await agent('POST', `/v1/chat/${token}`, { message: text, referenceId: crypto.randomUUID().replace(/-/g, '') });
  result.finalStatus = fin.status;

  // 5. Wie sieht die bearbeitete Nachricht für den Beobachter aus? (Kennzeichen "bearbeitet")
  const hist = await obs('GET', `/v1/chat/${token}?lookIntoFuture=0&limit=20`);
  const finalMsg = (hist.data ?? []).find((m) => m.id === fin.data.id);
  const statusMsg = (hist.data ?? []).find((m) => m.id === statusId);
  result.finalHasEditedMark = Boolean(finalMsg?.lastEditTimestamp);
  result.statusAfterDelete = statusMsg ? statusMsg.messageType : 'nicht mehr im Verlauf';
  result.systemMessagesInHistory = (hist.data ?? []).filter((m) => m.systemMessage).length;

  stopPoll = true;
  await Promise.race([poller, sleep(1000)]);
} finally {
  await cleanup([agentSig, obsSig]);
}
console.log(JSON.stringify(result, null, 1));
process.exit(0);
