// Verbindung zum Signaling-Server (HPB, "standalone signaling") für genau einen Raum.
// Wozu: (1) sofortiges Signal bei neuen/bearbeiteten/gelöschten Nachrichten ("chat refresh"),
//       (2) Tipp-Anzeige des Agenten an alle, die den Chat offen haben.
// Die Verbindung baut sich bei Abbruch selbst wieder auf.

const HELLO_TIMEOUT_MS = 15000;
const BACKOFF_MS = [1000, 2000, 5000, 10000, 30000, 60000];

export function createRoomSignaling({ client, token, log, onRefresh, onUp, onDown, WebSocketImpl = globalThis.WebSocket }) {
  let ws = null;
  let stopped = false;
  let up = false;
  let mySession = null;
  let attempt = 0;
  let reconnectTimer = null;
  let nextId = 1;
  let typingActive = false;
  const pending = new Map();
  const peers = new Map(); // Signaling-Session -> Benutzer-ID (ohne die eigene)

  const debug = (msg) => log?.debug?.(`nctalk signaling ${token}: ${msg}`);

  function send(obj) {
    if (ws && ws.readyState === 1) ws.send(JSON.stringify(obj));
  }

  function request(msg) {
    return new Promise((resolve, reject) => {
      const id = String(nextId++);
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`Zeitüberschreitung bei "${msg.type}"`));
      }, HELLO_TIMEOUT_MS);
      pending.set(id, (reply) => {
        clearTimeout(timer);
        resolve(reply);
      });
      send({ id, ...msg });
    });
  }

  function sendTypingTo(sessionId, type) {
    send({ type: "message", message: { recipient: { type: "session", sessionid: sessionId }, data: { type, to: sessionId } } });
  }

  function handleMessage(raw) {
    let m;
    try {
      m = JSON.parse(raw);
    } catch {
      return;
    }
    if (m.id && pending.has(m.id)) {
      const resolve = pending.get(m.id);
      pending.delete(m.id);
      resolve(m);
      return;
    }
    if (m.type === "event" && m.event?.target === "room") {
      const ev = m.event;
      if (ev.type === "join") {
        for (const j of ev.join ?? []) {
          if (!j?.sessionid || j.sessionid === mySession) continue;
          const isNew = !peers.has(j.sessionid);
          peers.set(j.sessionid, j.userid ?? "");
          // Wer den Chat erst jetzt öffnet, soll die laufende Tipp-Anzeige trotzdem sehen.
          if (isNew && typingActive) sendTypingTo(j.sessionid, "startedTyping");
        }
      } else if (ev.type === "leave") {
        for (const sid of ev.leave ?? []) peers.delete(sid);
      } else if (ev.type === "message") {
        const data = ev.message?.data;
        if (data?.type === "chat" && data.chat?.refresh) onRefresh?.();
      }
      return;
    }
    if (m.type === "bye" || (m.type === "error" && !m.id)) {
      debug(`Server beendet die Sitzung (${m.type})`);
      try {
        ws?.close();
      } catch {}
    }
  }

  async function connectOnce() {
    const settings = await client.signalingSettings(token);
    if (!settings?.server || !settings?.helloAuthParams?.["2.0"]) {
      const err = new Error("kein externer Signaling-Server eingerichtet");
      err.permanent = true;
      throw err;
    }
    const wsUrl = settings.server.replace(/^http/, "ws").replace(/\/?$/, "/") + "spreed";
    const joined = await client.joinRoom(token);
    const socket = new WebSocketImpl(wsUrl);
    ws = socket;
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("WebSocket-Verbindung: Zeitüberschreitung")), HELLO_TIMEOUT_MS);
      socket.addEventListener("open", () => { clearTimeout(timer); resolve(); }, { once: true });
      socket.addEventListener("error", () => { clearTimeout(timer); reject(new Error("WebSocket-Fehler beim Verbinden")); }, { once: true });
    });
    socket.addEventListener("message", (ev) => handleMessage(typeof ev.data === "string" ? ev.data : String(ev.data)));
    socket.addEventListener("close", () => {
      if (ws !== socket) return;
      ws = null;
      const wasUp = up;
      up = false;
      mySession = null;
      peers.clear();
      for (const [, resolve] of pending) resolve({ type: "error", error: { code: "closed" } });
      pending.clear();
      if (wasUp) onDown?.();
      scheduleReconnect();
    });
    const hello = await request({
      type: "hello",
      hello: { version: "2.0", auth: { url: client.signalingBackendUrl(), params: settings.helloAuthParams["2.0"] } },
    });
    if (hello.type !== "hello") throw new Error(`Anmeldung abgelehnt (${hello.error?.code ?? hello.type})`);
    mySession = hello.hello.sessionid;
    const room = await request({ type: "room", room: { roomid: token, sessionid: joined.sessionId } });
    if (room.type !== "room") throw new Error(`Raumbeitritt abgelehnt (${room.error?.code ?? room.type})`);
    up = true;
    attempt = 0;
    onUp?.();
  }

  function scheduleReconnect() {
    if (stopped || reconnectTimer) return;
    const delay = BACKOFF_MS[Math.min(attempt, BACKOFF_MS.length - 1)];
    attempt += 1;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      void start();
    }, delay);
    reconnectTimer.unref?.();
  }

  async function start() {
    if (stopped || up || ws) return;
    try {
      await connectOnce();
      debug("verbunden");
    } catch (err) {
      debug(`Verbindung fehlgeschlagen: ${err?.message ?? err}`);
      const socket = ws;
      ws = null;
      try {
        socket?.close();
      } catch {}
      if (err?.permanent) {
        stopped = true;
        onDown?.({ permanent: true });
        return;
      }
      scheduleReconnect();
    }
  }

  return {
    start,
    isUp: () => up,
    peerCount: () => peers.size,
    /** Tipp-Anzeige an alle, die den Chat gerade offen haben. Wiederholt aufrufbar (Auffrischen). */
    setTyping(active) {
      typingActive = active;
      if (!up) return false;
      const type = active ? "startedTyping" : "stoppedTyping";
      for (const sid of peers.keys()) sendTypingTo(sid, type);
      return peers.size > 0;
    },
    async stop() {
      stopped = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      reconnectTimer = null;
      const socket = ws;
      const wasJoined = up;
      ws = null;
      up = false;
      try {
        socket?.close();
      } catch {}
      if (wasJoined) await client.leaveRoom(token).catch(() => {});
    },
  };
}
