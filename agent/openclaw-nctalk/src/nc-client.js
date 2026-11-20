// Schlanker Client für die Nextcloud-APIs, die ein normales Benutzerkonto in Talk braucht:
// OCS (Talk, Freigaben) und WebDAV (Dateien). Anmeldung per App-Passwort (Basic Auth).

const TALK = "/ocs/v2.php/apps/spreed/api";

export class NcHttpError extends Error {
  constructor(message, status) {
    super(message);
    this.name = "NcHttpError";
    this.status = status;
  }
}

function encodeDavPath(p) {
  // "." und ".." werden verworfen, damit kein Pfad aus dem Dateibereich des Kontos herausführt.
  return String(p).split("/").filter((seg) => seg && seg !== "." && seg !== "..").map(encodeURIComponent).join("/");
}

export function createNcClient({ baseUrl, user, password, fetchImpl = fetch }) {
  if (!baseUrl || !user || !password) throw new Error("nctalk: baseUrl, user und App-Passwort werden benötigt");
  const auth = "Basic " + Buffer.from(`${user}:${password}`, "utf8").toString("base64");
  const redact = (text) => String(text).replaceAll(password, "***").replaceAll(auth, "***");

  async function request(method, url, { body, headers, timeoutMs = 30000, signal, raw } = {}) {
    const signals = [AbortSignal.timeout(timeoutMs)];
    if (signal) signals.push(signal);
    let res;
    try {
      res = await fetchImpl(url, {
        method,
        headers: {
          Authorization: auth,
          "OCS-APIRequest": "true",
          Accept: "application/json",
          ...(body !== undefined && !raw ? { "Content-Type": "application/json" } : {}),
          ...headers,
        },
        body: body === undefined ? undefined : raw ? body : JSON.stringify(body),
        signal: AbortSignal.any(signals),
      });
    } catch (err) {
      throw new Error(redact(`nctalk: ${method} ${new URL(url).pathname} fehlgeschlagen: ${err?.cause?.message ?? err?.message ?? err}`), { cause: err });
    }
    return res;
  }

  /** OCS-Aufruf; gibt {status, data, headers} zurück und wirft nur bei Netzwerkfehlern. */
  async function ocs(method, path, opts = {}) {
    const query = opts.query
      ? "?" + new URLSearchParams(Object.entries(opts.query).filter(([, v]) => v !== undefined && v !== null).map(([k, v]) => [k, String(v)])).toString()
      : "";
    const res = await request(method, `${baseUrl}${path}${query}`, opts);
    let data = null;
    if (res.status !== 304 && res.status !== 204) {
      const text = await res.text();
      try {
        data = JSON.parse(text)?.ocs?.data ?? null;
      } catch {
        data = null;
      }
    } else {
      await res.body?.cancel().catch(() => {});
    }
    return { status: res.status, data, headers: res.headers };
  }

  function expect(res, what, ok = [200, 201]) {
    if (!ok.includes(res.status)) {
      const detail = res.data && typeof res.data === "object" && res.data.error ? ` (${res.data.error})` : "";
      throw new NcHttpError(`nctalk: ${what} fehlgeschlagen: HTTP ${res.status}${detail}`, res.status);
    }
    return res.data;
  }

  return {
    baseUrl,
    user,
    host: new URL(baseUrl).hostname,

    async whoami() {
      const res = await ocs("GET", "/ocs/v2.php/cloud/user");
      const data = expect(res, "Anmeldung", [200]);
      return { id: String(data.id), displayName: data.displayname || data["display-name"] || String(data.id) };
    },

    async capabilities() {
      const res = await ocs("GET", "/ocs/v2.php/cloud/capabilities");
      const caps = expect(res, "Fähigkeiten lesen", [200])?.capabilities ?? {};
      const spreed = caps.spreed ?? {};
      return {
        talkVersion: spreed.version,
        features: new Set(spreed.features ?? []),
        maxMessageLength: Number(spreed.config?.chat?.["max-length"] ?? 1000),
        attachmentFolder: spreed.config?.attachments?.folder || "/Talk",
        typingPrivacy: spreed.config?.chat?.["typing-privacy"],
      };
    },

    /**
     * Eigener Status mit Emoji und Text (App user_status), ohne Ablauf. Daran erkennen Clients
     * das Konto als KI-Agenten (docs/agents.md). Gibt false zurück, wenn der Server das nicht kann.
     */
    async setUserStatus({ icon, message }) {
      const res = await ocs("PUT", "/ocs/v2.php/apps/user_status/api/v1/user_status/message/custom", {
        body: { statusIcon: icon, message, clearAt: null },
      });
      return res.status === 200;
    },

    async clearUserStatus() {
      const res = await ocs("DELETE", "/ocs/v2.php/apps/user_status/api/v1/user_status/message");
      return res.status === 200;
    },

    /** Räume des Kontos; mit modifiedSince nur geänderte. */
    async listRooms({ modifiedSince, signal } = {}) {
      const res = await ocs("GET", `${TALK}/v4/room`, { query: { modifiedSince, includeStatus: "false" }, signal });
      const rooms = expect(res, "Raumliste lesen", [200]) ?? [];
      // Der Server nennt den Zeitpunkt, bis zu dem die Liste vollständig ist (für den nächsten Abgleich).
      return { rooms, modifiedBefore: Number(res.headers.get("x-nextcloud-talk-modified-before")) || undefined };
    },

    async getRoom(token) {
      const res = await ocs("GET", `${TALK}/v4/room/${encodeURIComponent(token)}`);
      return expect(res, "Raum lesen", [200]);
    },

    /** Gibt das 1:1 mit dem Benutzer zurück und legt es bei Bedarf an. */
    async ensureOneToOne(userId) {
      const res = await ocs("POST", `${TALK}/v4/room`, { body: { roomType: 1, invite: userId } });
      return expect(res, `1:1 mit ${userId} öffnen`, [200, 201]);
    },

    /**
     * Neue Nachrichten nach lastKnownMessageId (aufsteigend).
     * timeout > 0 hält die Anfrage bis zu so vielen Sekunden offen (Long Polling).
     */
    async fetchNewMessages(token, { lastKnownMessageId, timeout = 0, limit = 100, signal } = {}) {
      const res = await ocs("GET", `${TALK}/v1/chat/${encodeURIComponent(token)}`, {
        query: {
          lookIntoFuture: 1,
          lastKnownMessageId,
          timeout,
          limit,
          setReadMarker: 0,
          includeLastKnown: 0,
          markNotificationsAsRead: 0,
        },
        timeoutMs: (timeout + 20) * 1000,
        signal,
      });
      if (res.status === 304) return { messages: [], lastGiven: undefined };
      const messages = expect(res, "Nachrichten lesen", [200]) ?? [];
      const lastGiven = Number(res.headers.get("x-chat-last-given")) || undefined;
      return { messages, lastGiven };
    },

    /** Jüngste Nachrichten eines Raums (neueste zuerst). */
    async fetchRecentMessages(token, limit = 20) {
      const res = await ocs("GET", `${TALK}/v1/chat/${encodeURIComponent(token)}`, {
        query: { lookIntoFuture: 0, limit, setReadMarker: 0, markNotificationsAsRead: 0 },
      });
      if (res.status === 304) return [];
      return expect(res, "Verlauf lesen", [200]) ?? [];
    },

    async sendMessage(token, { message, replyTo, silent, referenceId } = {}) {
      const res = await ocs("POST", `${TALK}/v1/chat/${encodeURIComponent(token)}`, {
        body: {
          message,
          ...(replyTo ? { replyTo: Number(replyTo) } : {}),
          ...(silent ? { silent: true } : {}),
          ...(referenceId ? { referenceId } : {}),
        },
      });
      return expect(res, "Nachricht senden", [201]);
    },

    async editMessage(token, messageId, message) {
      const res = await ocs("PUT", `${TALK}/v1/chat/${encodeURIComponent(token)}/${messageId}`, { body: { message } });
      return expect(res, "Nachricht bearbeiten", [200, 202]);
    },

    async deleteMessage(token, messageId) {
      const res = await ocs("DELETE", `${TALK}/v1/chat/${encodeURIComponent(token)}/${messageId}`);
      expect(res, "Nachricht löschen", [200, 202]);
      return true;
    },

    async setReadMarker(token, lastReadMessage) {
      const res = await ocs("POST", `${TALK}/v1/chat/${encodeURIComponent(token)}/read`, { body: { lastReadMessage: Number(lastReadMessage) } });
      return res.status === 200;
    },

    async addReaction(token, messageId, reaction) {
      const res = await ocs("POST", `${TALK}/v1/reaction/${encodeURIComponent(token)}/${messageId}`, { body: { reaction } });
      expect(res, "Reaktion setzen", [200, 201]);
      return true;
    },

    async removeReaction(token, messageId, reaction) {
      const res = await ocs("DELETE", `${TALK}/v1/reaction/${encodeURIComponent(token)}/${messageId}`, { query: { reaction } });
      expect(res, "Reaktion entfernen", [200]);
      return true;
    },

    // ---- Signaling ---------------------------------------------------------
    async signalingSettings(token) {
      const res = await ocs("GET", `${TALK}/v3/signaling/settings`, { query: { token } });
      return expect(res, "Signaling-Einstellungen lesen", [200]);
    },

    async joinRoom(token) {
      const res = await ocs("POST", `${TALK}/v4/room/${encodeURIComponent(token)}/participants/active`, { body: { force: true } });
      return expect(res, "Raum betreten", [200]);
    },

    async leaveRoom(token) {
      const res = await ocs("DELETE", `${TALK}/v4/room/${encodeURIComponent(token)}/participants/active`, { timeoutMs: 8000 });
      return res.status === 200;
    },

    signalingBackendUrl() {
      return `${baseUrl}${TALK}/v3/signaling/backend`;
    },

    // ---- Dateien -------------------------------------------------------------
    /** Lädt eine Datei aus dem Dateibereich des Kontos (Pfad wie in messageParameters.file.path). */
    async downloadFile(filePath, { maxBytes, signal } = {}) {
      const res = await request("GET", `${baseUrl}/remote.php/dav/files/${encodeURIComponent(user)}/${encodeDavPath(filePath)}`, { timeoutMs: 120000, signal, headers: { Accept: "*/*" } });
      if (!res.ok) {
        await res.body?.cancel().catch(() => {});
        throw new NcHttpError(`nctalk: Datei laden fehlgeschlagen: HTTP ${res.status}`, res.status);
      }
      const declared = Number(res.headers.get("content-length"));
      if (maxBytes && declared && declared > maxBytes) {
        await res.body?.cancel().catch(() => {});
        throw new Error(`nctalk: Datei ist zu groß (${declared} Bytes, erlaubt ${maxBytes})`);
      }
      const buffer = Buffer.from(await res.arrayBuffer());
      if (maxBytes && buffer.length > maxBytes) throw new Error(`nctalk: Datei ist zu groß (${buffer.length} Bytes, erlaubt ${maxBytes})`);
      return { buffer, contentType: res.headers.get("content-type")?.split(";")[0]?.trim() || undefined };
    },

    async ensureFolder(folder) {
      const res = await request("MKCOL", `${baseUrl}/remote.php/dav/files/${encodeURIComponent(user)}/${encodeDavPath(folder)}`, { timeoutMs: 20000 });
      await res.body?.cancel().catch(() => {});
      // 201 angelegt, 405 existiert bereits
      if (![201, 405].includes(res.status)) throw new NcHttpError(`nctalk: Ordner anlegen fehlgeschlagen: HTTP ${res.status}`, res.status);
    },

    async uploadFile(filePath, buffer, contentType) {
      const res = await request("PUT", `${baseUrl}/remote.php/dav/files/${encodeURIComponent(user)}/${encodeDavPath(filePath)}`, {
        body: buffer,
        raw: true,
        timeoutMs: 300000,
        headers: { "Content-Type": contentType || "application/octet-stream" },
      });
      await res.body?.cancel().catch(() => {});
      if (![201, 204].includes(res.status)) throw new NcHttpError(`nctalk: Datei hochladen fehlgeschlagen: HTTP ${res.status}`, res.status);
    },

    /** Teilt eine hochgeladene Datei in einen Raum; das erzeugt die Datei-Nachricht im Chat. */
    async shareFileToRoom(filePath, token, { caption, messageType, replyTo, silent, referenceId } = {}) {
      const meta = {
        ...(caption ? { caption } : {}),
        ...(messageType ? { messageType } : {}),
        ...(replyTo ? { replyTo: Number(replyTo) } : {}),
        ...(silent ? { silent: true } : {}),
      };
      const res = await ocs("POST", "/ocs/v2.php/apps/files_sharing/api/v1/shares", {
        body: {
          shareType: 10,
          shareWith: token,
          path: filePath.startsWith("/") ? filePath : `/${filePath}`,
          ...(referenceId ? { referenceId } : {}),
          ...(Object.keys(meta).length ? { talkMetaData: JSON.stringify(meta) } : {}),
        },
      });
      return expect(res, "Datei in den Raum teilen", [200]);
    },
  };
}
