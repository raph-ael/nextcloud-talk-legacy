// Ausgehende Nachrichten: Text, Dateien, Reaktionen. Wird von der Antwortzustellung
// und vom gemeinsamen "message"-Werkzeug benutzt.
import { randomBytes } from "node:crypto";
import path from "node:path";
import { createMessageReceiptFromOutboundResults } from "openclaw/plugin-sdk/channel-outbound";
import { loadOutboundMediaFromUrl } from "openclaw/plugin-sdk/outbound-media";
import { createNcClient } from "./nc-client.js";
import { isNctalkAccountConfigured, parseTarget, resolveNctalkAccount } from "./config.js";
import { CHANNEL_ID, getOptionalNctalkRuntime } from "./runtime.js";

const clients = new Map(); // "baseUrl|user" -> { client, password, caps?, oneToOne: Map }

export function getNctalkClientEntry(account) {
  const key = `${account.baseUrl}|${account.user}`;
  let entry = clients.get(key);
  if (!entry || entry.password !== account.password) {
    entry = {
      client: createNcClient({ baseUrl: account.baseUrl, user: account.user, password: account.password }),
      password: account.password,
      caps: null,
      oneToOne: new Map(),
    };
    clients.set(key, entry);
  }
  return entry;
}

export async function getCapabilities(entry) {
  entry.caps ??= await entry.client.capabilities();
  return entry.caps;
}

function requireAccount(cfg, accountId) {
  if (!cfg) throw new Error("nctalk: Konfiguration fehlt");
  const account = resolveNctalkAccount(cfg, accountId);
  if (!account.enabled || !isNctalkAccountConfigured(account)) {
    throw new Error("nctalk: Kanal ist nicht eingerichtet (channels.nctalk.baseUrl, user und passwordFile setzen)");
  }
  if (!account.password) throw new Error("nctalk: App-Passwort ist konfiguriert, aber nicht lesbar");
  return account;
}

/** Löst ein Ziel in ein Raumtoken auf; 1:1-Räume werden bei Bedarf angelegt. */
export async function resolveRoomToken(entry, to) {
  const target = parseTarget(to);
  if (!target) throw new Error("nctalk: Ziel fehlt (Raumtoken oder user:<benutzer>)");
  if (target.kind === "room") return target.id;
  const userId = target.id;
  const cached = entry.oneToOne.get(userId.toLowerCase());
  if (cached) return cached;
  const room = await entry.client.ensureOneToOne(userId);
  entry.oneToOne.set(userId.toLowerCase(), room.token);
  return room.token;
}

function referenceId() {
  return randomBytes(32).toString("hex");
}

function recordOutbound(accountId) {
  getOptionalNctalkRuntime()?.channel.activity.record({ channel: CHANNEL_ID, accountId, direction: "outbound" });
}

function receiptFor(messageId, roomToken, kind, replyTo) {
  return createMessageReceiptFromOutboundResults({
    results: messageId ? [{ channel: CHANNEL_ID, messageId: String(messageId), conversationId: roomToken }] : [],
    kind,
    ...(replyTo ? { replyToId: String(replyTo) } : {}),
  });
}

function formatText(cfg, accountId, text) {
  const core = getOptionalNctalkRuntime();
  if (!core) return text;
  const tableMode = core.channel.text.resolveMarkdownTableMode({ cfg, channel: CHANNEL_ID, accountId });
  return core.channel.text.convertMarkdownTables(text, tableMode);
}

export async function sendTextNctalk(to, text, opts) {
  const account = requireAccount(opts.cfg, opts.accountId);
  const entry = getNctalkClientEntry(account);
  const caps = await getCapabilities(entry);
  const roomToken = await resolveRoomToken(entry, to);
  let message = formatText(opts.cfg, account.accountId, String(text ?? "").trim());
  if (!message) throw new Error("nctalk: leere Nachricht");
  if (message.length > caps.maxMessageLength) message = message.slice(0, caps.maxMessageLength - 1) + "…";
  await opts.onPlatformSendDispatch?.();
  const sent = await entry.client.sendMessage(roomToken, {
    message,
    replyTo: opts.replyTo,
    silent: opts.silent,
    referenceId: referenceId(),
  });
  recordOutbound(account.accountId);
  return {
    messageId: String(sent.id),
    roomToken,
    content: message,
    timestamp: typeof sent.timestamp === "number" ? sent.timestamp : undefined,
    receipt: receiptFor(sent.id, roomToken, "text", opts.replyTo),
  };
}

function safeFileName(name, contentType) {
  const base = path.basename(String(name || "").trim()).replace(/[\\/:*?"<>|\u0000-\u001f]/g, "_").slice(0, 120);
  if (base && base !== "." && base !== "..") return base;
  const ext = { "image/png": ".png", "image/jpeg": ".jpg", "image/webp": ".webp", "image/gif": ".gif", "audio/ogg": ".ogg", "audio/mpeg": ".mp3", "application/pdf": ".pdf", "text/plain": ".txt" }[contentType] ?? "";
  return `datei${ext}`;
}

/** Lädt eine Datei in den Dateibereich des Agenten und teilt sie in den Raum. */
export async function sendMediaNctalk(to, text, mediaUrl, opts) {
  const account = requireAccount(opts.cfg, opts.accountId);
  const entry = getNctalkClientEntry(account);
  const caps = await getCapabilities(entry);
  const roomToken = await resolveRoomToken(entry, to);
  const media = await loadOutboundMediaFromUrl(mediaUrl, {
    maxBytes: account.mediaMaxBytes,
    mediaLocalRoots: opts.mediaLocalRoots,
    mediaReadFile: opts.mediaReadFile,
    workspaceDir: opts.workspaceDir,
  });
  const fileName = safeFileName(media.fileName, media.contentType);
  const root = caps.attachmentFolder.replace(/^\/+|\/+$/g, "") || "Talk";
  const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\..*$/, "").replace("T", "-");
  const folder = `${root}/Agent/${stamp}-${randomBytes(2).toString("hex")}`;
  await entry.client.ensureFolder(root);
  await entry.client.ensureFolder(`${root}/Agent`);
  await entry.client.ensureFolder(folder);
  const filePath = `${folder}/${fileName}`;
  await entry.client.uploadFile(filePath, media.buffer, media.contentType);
  const caption = formatText(opts.cfg, account.accountId, String(text ?? "").trim()).slice(0, caps.maxMessageLength);
  const isVoice = Boolean(opts.audioAsVoice) && String(media.contentType ?? "").startsWith("audio/");
  await opts.onPlatformSendDispatch?.();
  const ref = referenceId();
  await entry.client.shareFileToRoom(filePath, roomToken, {
    caption: caption || undefined,
    messageType: isVoice ? "voice-message" : undefined,
    replyTo: opts.replyTo,
    silent: opts.silent,
    referenceId: ref,
  });
  recordOutbound(account.accountId);
  // Die Freigabe-API nennt die Nachrichten-ID nicht; sie wird über die Referenz im Verlauf nachgeschlagen.
  let messageId;
  for (let attempt = 0; attempt < 3 && !messageId; attempt++) {
    const recent = await entry.client.fetchRecentMessages(roomToken, 15).catch(() => []);
    messageId = recent.find((m) => m.referenceId === ref)?.id;
    if (!messageId) await new Promise((resolve) => setTimeout(resolve, 300));
  }
  return {
    messageId: messageId ? String(messageId) : undefined,
    roomToken,
    content: caption || fileName,
    receipt: receiptFor(messageId, roomToken, "media", opts.replyTo),
  };
}

export async function reactNctalk(to, messageId, emoji, opts) {
  const account = requireAccount(opts.cfg, opts.accountId);
  const entry = getNctalkClientEntry(account);
  const roomToken = await resolveRoomToken(entry, to);
  if (opts.remove) await entry.client.removeReaction(roomToken, messageId, emoji);
  else await entry.client.addReaction(roomToken, messageId, emoji);
  return { ok: true };
}

export async function editNctalk(to, messageId, text, opts) {
  const account = requireAccount(opts.cfg, opts.accountId);
  const entry = getNctalkClientEntry(account);
  const roomToken = await resolveRoomToken(entry, to);
  await entry.client.editMessage(roomToken, messageId, formatText(opts.cfg, account.accountId, String(text ?? "").trim()));
  return { ok: true };
}

export async function deleteNctalk(to, messageId, opts) {
  const account = requireAccount(opts.cfg, opts.accountId);
  const entry = getNctalkClientEntry(account);
  const roomToken = await resolveRoomToken(entry, to);
  await entry.client.deleteMessage(roomToken, messageId);
  return { ok: true };
}
