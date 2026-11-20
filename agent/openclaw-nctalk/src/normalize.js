// Übersetzt eine rohe Talk-Nachricht in das, was der Agent braucht.

/** Raumtypen in Talk: 1 = 1:1, 2 = Gruppe, 3 = öffentlich, 4 = Änderungsprotokoll, 5 = ehemaliges 1:1, 6 = Notizen an mich */
export function roomKindFromType(type) {
  // "Notizen an mich" (6) enthält nur eigene Nachrichten und wird nicht beobachtet.
  if (type === 1 || type === 5) return "direct";
  if (type === 2 || type === 3) return "group";
  return undefined;
}

function renderText(message, params, { dropFiles }) {
  let text = String(message ?? "");
  if (!params || typeof params !== "object" || Array.isArray(params)) return text;
  return text.replace(/\{([a-zA-Z0-9_.-]+)\}/g, (whole, key) => {
    const p = params[key];
    if (!p || typeof p !== "object") return whole;
    switch (p.type) {
      case "user":
      case "guest":
      case "email":
      case "federated_user":
      case "user-group":
      case "group":
      case "circle":
      case "team":
        return `@${p.name ?? p.id ?? key}`;
      case "call":
        return "@alle";
      case "file":
        return dropFiles ? "" : String(p.name ?? "Datei");
      default:
        return String(p.name ?? whole);
    }
  });
}

function collectFiles(params) {
  if (!params || typeof params !== "object" || Array.isArray(params)) return [];
  const files = [];
  for (const [key, p] of Object.entries(params)) {
    if (!p || p.type !== "file") continue;
    if (key !== "file" && !/^file\d*$/.test(key)) continue;
    files.push({
      id: p.id != null ? String(p.id) : undefined,
      name: String(p.name ?? "datei"),
      path: typeof p.path === "string" ? p.path : undefined,
      mimetype: typeof p.mimetype === "string" ? p.mimetype : undefined,
      size: Number(p.size) || undefined,
    });
  }
  return files;
}

/**
 * @returns null, wenn die Nachricht den Agenten nichts angeht (eigene, System-, gelöschte Nachrichten).
 */
export function normalizeTalkMessage(msg, room, selfId) {
  if (!msg || typeof msg !== "object") return null;
  if (msg.systemMessage) return null;
  if (!["comment", "voice-message", "record-audio", "record-video"].includes(msg.messageType)) return null;
  if (msg.actorType !== "users") return null;
  const senderId = String(msg.actorId ?? "");
  if (!senderId || senderId === selfId) return null;
  const kind = roomKindFromType(Number(room?.type));
  if (!kind) return null;

  const params = msg.messageParameters;
  const files = collectFiles(params);
  const text = renderText(msg.message, params, { dropFiles: true }).trim();
  let mentionedSelf = false;
  let mentionsAll = false;
  if (params && typeof params === "object" && !Array.isArray(params)) {
    for (const [key, p] of Object.entries(params)) {
      if (!key.startsWith("mention-") || !p) continue;
      if (p.type === "user" && String(p.id) === selfId) mentionedSelf = true;
      if (p.type === "call") mentionsAll = true;
    }
  }

  let replyTo;
  const parent = msg.parent;
  if (parent && typeof parent === "object" && parent.id && !parent.deleted && parent.messageType !== "comment_deleted") {
    replyTo = {
      id: String(parent.id),
      senderId: String(parent.actorId ?? ""),
      senderName: String(parent.actorDisplayName ?? ""),
      text: renderText(parent.message, parent.messageParameters, { dropFiles: false }).trim(),
      isSelf: parent.actorType === "users" && String(parent.actorId) === selfId,
    };
  }

  return {
    messageId: String(msg.id),
    roomToken: String(room.token),
    roomName: String(room.displayName || room.name || room.token),
    kind,
    senderId,
    senderName: String(msg.actorDisplayName || senderId),
    text,
    timestamp: (Number(msg.timestamp) || Math.floor(Date.now() / 1000)) * 1000,
    files,
    isVoice: msg.messageType === "voice-message" || msg.messageType === "record-audio",
    mentionedSelf,
    mentionsAll,
    replyTo,
  };
}
