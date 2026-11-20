// Empfang für ein Agenten-Konto:
//  - Raumliste regelmäßig abgleichen (neue Räume, verpasste Nachrichten),
//  - je beobachtetem Raum eine Signaling-Sitzung (sofortiges Signal bei neuen Nachrichten, Tipp-Anzeige),
//    ersatzweise Long Polling, wenn kein Signaling-Server da ist,
//  - neue Nachrichten in die dauerhafte Eingangswarteschlange von OpenClaw geben (kein Verlust bei Neustart).
import { createChannelIngressMonitor } from "openclaw/plugin-sdk/channel-outbound";
import { channelReadyPatch } from "openclaw/plugin-sdk/gateway-runtime";
import { handleNctalkInbound } from "./inbound.js";
import { normalizeTalkMessage, roomKindFromType } from "./normalize.js";
import { CHANNEL_ID, getNctalkRuntime } from "./runtime.js";
import { getCapabilities, getNctalkClientEntry } from "./send.js";
import { createRoomSignaling } from "./signaling.js";

const INGRESS_POLL_INTERVAL_MS = 500;
const SAFETY_SYNC_MS = 60_000;

class NctalkEventError extends Error {}

const sleep = (ms, signal) =>
  new Promise((resolve) => {
    if (signal?.aborted) return resolve();
    const timer = setTimeout(resolve, ms);
    signal?.addEventListener("abort", () => { clearTimeout(timer); resolve(); }, { once: true });
  });

// Laufende Empfänger, damit Zustellung und Werkzeuge die Tipp-Anzeige erreichen.
const monitors = new Map(); // accountId -> { setTyping }
export function setNctalkTyping(accountId, roomToken, active) {
  return monitors.get(accountId)?.setTyping(roomToken, active) ?? false;
}

export async function monitorNctalkProvider({ account, cfg, runtime, abortSignal, statusSink, log }) {
  const core = getNctalkRuntime();
  const entry = getNctalkClientEntry(account);
  const client = entry.client;
  const self = await client.whoami();
  const caps = await getCapabilities(entry);
  if (!caps.features.has("edit-messages") && account.streamingMode !== "off") {
    runtime.log?.("nctalk: Diese Talk-Version kann Nachrichten nicht bearbeiten; Live-Status ist damit wirkungslos (streaming.mode: off empfohlen).");
  }

  const cursors = core.state.openKeyedStore({ namespace: `cursors.${account.accountId}`, maxEntries: 5000 });
  const rooms = new Map(); // token -> Raumzustand
  const stopController = new AbortController();
  const stopSignal = abortSignal ? AbortSignal.any([abortSignal, stopController.signal]) : stopController.signal;
  let signalingAvailable = true;

  // ---- dauerhafte Eingangswarteschlange -------------------------------------
  let queue;
  const getQueue = () => (queue ??= core.state.openChannelIngressQueue({ accountId: account.accountId }));
  const ingress = createChannelIngressMonitor({
    queue: getQueue,
    inspect: (rawEvent) => {
      const event = JSON.parse(rawEvent);
      return { eventId: String(event.message.messageId), laneKey: `room:${event.message.roomToken}` };
    },
    payload: {
      version: 1,
      serialize: (rawEvent, { receivedAt }) => ({ receivedAt, rawEvent }),
      deserialize: (body) => body.rawEvent,
      encode: ({ body }) => ({ version: 1, ...body }),
      decode: (payload) => ({ version: payload.version, body: { receivedAt: payload.receivedAt, rawEvent: payload.rawEvent } }),
      createClaimError: (kind, claim) => new NctalkEventError(`nctalk: Eintrag ${claim.id} in der Eingangswarteschlange ist ungültig (${kind})`),
    },
    deliver: async (rawEvent, lifecycle) => {
      const { message } = JSON.parse(rawEvent);
      core.channel.activity.record({ channel: CHANNEL_ID, accountId: account.accountId, direction: "inbound", at: message.timestamp });
      await handleNctalkInbound({
        message,
        account,
        config: cfg,
        runtime,
        statusSink,
        client,
        setTyping,
        turnAdoptionLifecycle: lifecycle,
      });
    },
    pollIntervalMs: INGRESS_POLL_INTERVAL_MS,
    retention: { completedMaxEntries: 10000, failedMaxEntries: 10000 },
    drain: {
      startLimit: 32,
      resolveNonRetryableFailure: (error) => (error instanceof NctalkEventError || error instanceof SyntaxError ? { reason: "invalid-event", message: error.message } : null),
      onLog: (message) => runtime.log?.(`nctalk ${message}`),
    },
    abortSignal: stopSignal,
    createStoppedError: () => new Error("nctalk: Empfang wurde beendet"),
    onError: (error) => runtime.error?.(`nctalk: Eingangswarteschlange: ${error?.message ?? error}`),
  });

  // ---- Raumzustand ----------------------------------------------------------
  function setTyping(roomToken, active) {
    const room = rooms.get(roomToken);
    return room?.signaling?.setTyping(active) ?? false;
  }

  async function loadCursor(room, listed) {
    const stored = await cursors.lookup(room.token).catch(() => undefined);
    if (typeof stored === "number" && stored > 0) return stored;
    const lastRead = Number(listed?.lastReadMessage);
    const lastId = Number(listed?.lastMessage?.id) || 0;
    const lastTs = Number(listed?.lastMessage?.timestamp) || 0;
    const tooOld = account.backlogMaxAgeSeconds > 0 && lastTs > 0 && Date.now() / 1000 - lastTs > account.backlogMaxAgeSeconds;
    if (lastRead > 0) return tooOld ? Math.max(lastRead, lastId) : lastRead;
    // Unbekannter Lesestand: bei alten Räumen nichts nachholen, bei frischen von vorn lesen.
    return tooOld ? lastId : 0;
  }

  async function syncRoom(room) {
    if (room.syncing) {
      room.dirty = true;
      return room.syncing;
    }
    room.syncing = (async () => {
      try {
        do {
          room.dirty = false;
          for (;;) {
            const { messages, lastGiven } = await client.fetchNewMessages(room.token, { lastKnownMessageId: room.cursor, signal: stopSignal });
            if (messages.length === 0) break;
            let maxId = room.cursor;
            for (const raw of messages) {
              maxId = Math.max(maxId, Number(raw.id) || 0);
              const normalized = normalizeTalkMessage(raw, room, self.id);
              if (!normalized) continue;
              const ageSeconds = (Date.now() - normalized.timestamp) / 1000;
              if (account.backlogMaxAgeSeconds > 0 && ageSeconds > account.backlogMaxAgeSeconds) continue;
              await ingress.admit(JSON.stringify({ message: normalized }));
            }
            room.cursor = Math.max(maxId, lastGiven ?? 0);
            await cursors.register(room.token, room.cursor).catch((err) => runtime.error?.(`nctalk: Lesestand für ${room.token} nicht gespeichert: ${err?.message ?? err}`));
            // Lesebestätigung: Der Absender sieht, dass die Nachricht beim Agenten angekommen ist.
            void client.setReadMarker(room.token, room.cursor).catch(() => {});
            if (messages.length < 100) break;
          }
        } while (room.dirty && !stopSignal.aborted);
      } catch (err) {
        if (!stopSignal.aborted) runtime.error?.(`nctalk: Nachrichten aus ${room.token} konnten nicht gelesen werden: ${err?.message ?? err}`);
      } finally {
        room.syncing = null;
      }
    })();
    return room.syncing;
  }

  function startLongPoll(room) {
    if (room.longPoll) return;
    room.longPoll = (async () => {
      while (!stopSignal.aborted && room.watched && !room.signaling?.isUp()) {
        try {
          const { messages } = await client.fetchNewMessages(room.token, { lastKnownMessageId: room.cursor, timeout: 30, limit: 1, signal: stopSignal });
          if (messages.length > 0) await syncRoom(room);
        } catch {
          await sleep(5000, stopSignal);
        }
      }
      room.longPoll = null;
    })();
  }

  function watch(room) {
    if (room.watched) return;
    room.watched = true;
    if (!signalingAvailable) return startLongPoll(room);
    room.signaling = createRoomSignaling({
      client,
      token: room.token,
      log,
      onRefresh: () => void syncRoom(room),
      onUp: () => void syncRoom(room),
      onDown: (info) => {
        if (info?.permanent) {
          if (signalingAvailable) runtime.log?.("nctalk: Kein Signaling-Server erreichbar – Empfang per Long Polling, Tipp-Anzeige entfällt.");
          signalingAvailable = false;
          room.signaling = null;
        }
        if (room.watched && !stopSignal.aborted) startLongPoll(room);
      },
    });
    void room.signaling.start();
  }

  async function unwatch(room) {
    if (!room.watched) return;
    room.watched = false;
    const signaling = room.signaling;
    room.signaling = null;
    await signaling?.stop().catch(() => {});
  }

  // ---- Raumliste abgleichen -------------------------------------------------
  let modifiedSince = 0;
  let lastFullSync = 0;
  async function discover() {
    const full = Date.now() - lastFullSync > SAFETY_SYNC_MS;
    const startedAt = Math.floor(Date.now() / 1000);
    const { rooms: listed, modifiedBefore } = await client.listRooms({ modifiedSince: full ? undefined : modifiedSince || undefined, signal: stopSignal });
    if (full) lastFullSync = Date.now();
    modifiedSince = modifiedBefore ?? startedAt - 30;
    for (const item of listed) {
      if (!item?.token || !roomKindFromType(Number(item.type))) continue;
      let room = rooms.get(item.token);
      if (!room) {
        room = { token: item.token, type: item.type, name: item.name, displayName: item.displayName, cursor: 0, watched: false, signaling: null, syncing: null, dirty: false, longPoll: null, lastActivity: 0 };
        room.cursor = await loadCursor(room, item);
        rooms.set(item.token, room);
      }
      room.type = item.type;
      room.name = item.name;
      room.displayName = item.displayName;
      room.lastActivity = Number(item.lastActivity) || 0;
      if ((Number(item.lastMessage?.id) || 0) > room.cursor) void syncRoom(room);
    }
    if (full) {
      const present = new Set(listed.map((item) => item.token));
      for (const [token, room] of rooms) {
        if (present.has(token)) continue;
        rooms.delete(token);
        void unwatch(room);
      }
    }
    // Die zuletzt aktiven Räume bekommen eine Signaling-Sitzung, der Rest läuft über diesen Abgleich.
    const ranked = [...rooms.values()].sort((a, b) => b.lastActivity - a.lastActivity);
    ranked.forEach((room, index) => {
      if (index < account.maxWatchedRooms) watch(room);
      else void unwatch(room);
    });
  }

  ingress.start();
  let failures = 0;
  const loop = (async () => {
    while (!stopSignal.aborted) {
      try {
        await discover();
        if (failures > 0) statusSink?.({ connected: true, lastError: null });
        failures = 0;
      } catch (err) {
        if (stopSignal.aborted) break;
        failures += 1;
        if (failures === 1 || failures % 12 === 0) runtime.error?.(`nctalk: Raumliste konnte nicht gelesen werden: ${err?.message ?? err}`);
        statusSink?.({ connected: false, lastError: String(err?.message ?? err) });
      }
      await sleep(Math.min(account.discoveryIntervalMs * (failures > 3 ? 6 : 1), 60000), stopSignal);
    }
  })();

  monitors.set(account.accountId, { setTyping });
  statusSink?.({ ...channelReadyPatch(), connected: true, lastError: null });
  runtime.log?.(`nctalk: angemeldet als ${self.id} an ${client.host} (Talk ${caps.talkVersion ?? "?"}, Live-Status: ${account.streamingMode})`);

  let stopPromise;
  const stop = () =>
    (stopPromise ??= (async () => {
      stopController.abort();
      monitors.delete(account.accountId);
      await loop.catch(() => {});
      await Promise.allSettled([...rooms.values()].map((room) => unwatch(room)));
      const pause = ingress.pause();
      await ingress.stop().catch(() => {});
      await pause.catch?.(() => {});
    })());
  abortSignal?.addEventListener("abort", () => void stop(), { once: true });
  return { stop, self };
}
