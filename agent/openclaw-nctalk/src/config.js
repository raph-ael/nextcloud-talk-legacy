// Kontoauflösung für den Kanal "nctalk".
// Ein Gateway = ein Agent = ein Nextcloud-Konto; es gibt deshalb nur das Konto "default".
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { CHANNEL_ID } from "./runtime.js";

export const DEFAULT_ACCOUNT_ID = "default";
export const PASSWORD_ENV = "NCTALK_APP_PASSWORD";
const TARGET_PREFIX_RE = /^(nctalk|talk):/i;

export function section(cfg) {
  return cfg?.channels?.[CHANNEL_ID] ?? {};
}

function expandHome(p) {
  if (!p) return p;
  if (p === "~") return os.homedir();
  if (p.startsWith("~/")) return path.join(os.homedir(), p.slice(2));
  return p;
}

/** Liest das App-Passwort, ohne es je zu protokollieren. */
function resolvePassword(sec) {
  if (typeof sec.password === "string" && sec.password.trim()) {
    return { password: sec.password.trim(), source: "config", status: "available" };
  }
  if (sec.password && typeof sec.password === "object") {
    // SecretRef, der vom Gateway nicht aufgelöst wurde
    return { password: "", source: "config", status: "configured_unavailable" };
  }
  const file = expandHome(typeof sec.passwordFile === "string" ? sec.passwordFile.trim() : "");
  if (file) {
    try {
      const value = fs.readFileSync(file, "utf8").trim();
      return value
        ? { password: value, source: "file", status: "available" }
        : { password: "", source: "file", status: "configured_unavailable" };
    } catch {
      return { password: "", source: "file", status: "configured_unavailable" };
    }
  }
  const env = process.env[PASSWORD_ENV]?.trim();
  if (env) return { password: env, source: "env", status: "available" };
  return { password: "", source: "none", status: "missing" };
}

export function normalizeBaseUrl(value) {
  return typeof value === "string" ? value.trim().replace(/\/+$/, "") : "";
}

export function normalizeUserId(raw) {
  return String(raw ?? "").trim().replace(TARGET_PREFIX_RE, "").replace(/^user:/i, "").trim().toLowerCase();
}

export function resolveStreamingMode(sec) {
  const mode = sec?.streaming?.mode;
  return mode === "off" || mode === "partial" || mode === "progress" ? mode : "progress";
}

export function resolveNctalkAccount(cfg, accountId) {
  const sec = section(cfg);
  const secret = resolvePassword(sec);
  const baseUrl = normalizeBaseUrl(sec.baseUrl);
  const user = typeof sec.user === "string" ? sec.user.trim() : "";
  return {
    accountId: accountId || DEFAULT_ACCOUNT_ID,
    enabled: sec.enabled !== false,
    name: typeof sec.name === "string" && sec.name.trim() ? sec.name.trim() : undefined,
    baseUrl,
    user,
    password: secret.password,
    passwordSource: secret.source,
    tokenStatus: secret.status,
    streamingMode: resolveStreamingMode(sec),
    // Abstand zwischen zwei Bearbeitungen der Statusnachricht. Jede Bearbeitung legt in Talk eine
    // unsichtbare Systemnachricht an, die alle Clients mitladen – deshalb nicht zu dicht.
    editIntervalMs: Math.max(
      500,
      Number.isFinite(sec.streaming?.editIntervalMs) ? sec.streaming.editIntervalMs : resolveStreamingMode(sec) === "partial" ? 2500 : 1500,
    ),
    // Stille Quittung im Modus "progress": sofort beim Annehmen, wird zum Live-Status und am Ende gelöscht.
    ackText: sec.streaming?.ack === false ? "" : typeof sec.streaming?.ack === "string" && sec.streaming.ack.trim() ? sec.streaming.ack.trim() : "👀",
    typing: sec.typing !== false,
    // Status des Kontos: Am Emoji erkennen Clients den Agenten. `userStatus: false` lässt den Status in Ruhe.
    userStatus:
      sec.userStatus === false
        ? null
        : {
            icon: typeof sec.userStatus?.icon === "string" && sec.userStatus.icon.trim() ? sec.userStatus.icon.trim() : "🤖",
            message: typeof sec.userStatus?.message === "string" ? sec.userStatus.message.trim() : "KI-Agent",
          },
    statusCleanup: sec.statusCleanup === "keep" ? "keep" : "delete",
    textChunkLimit: Number.isInteger(sec.textChunkLimit) ? sec.textChunkLimit : 8000,
    mediaMaxBytes: Math.round((typeof sec.mediaMaxMb === "number" ? sec.mediaMaxMb : 25) * 1024 * 1024),
    discoveryIntervalMs: Math.max(2, typeof sec.discoveryIntervalSeconds === "number" ? sec.discoveryIntervalSeconds : 5) * 1000,
    maxWatchedRooms: Number.isInteger(sec.maxWatchedRooms) ? sec.maxWatchedRooms : 12,
    backlogMaxAgeSeconds: Number.isInteger(sec.backlogMaxAgeSeconds) ? sec.backlogMaxAgeSeconds : 86400,
    config: sec,
  };
}

export function isNctalkAccountConfigured(account) {
  return Boolean(account?.baseUrl && account?.user && account?.tokenStatus !== "missing");
}

export function hasNctalkConfiguredState(cfg) {
  const sec = section(cfg);
  if (sec.enabled === false) return false;
  return isNctalkAccountConfigured(resolveNctalkAccount(cfg));
}

/** Statussichere Sicht auf das Konto (ohne Geheimnisse). */
export function inspectNctalkAccount(cfg, accountId) {
  const account = resolveNctalkAccount(cfg, accountId);
  return {
    accountId: account.accountId,
    enabled: account.enabled,
    name: account.name,
    configured: isNctalkAccountConfigured(account),
    tokenStatus: account.tokenStatus,
    passwordSource: account.passwordSource,
    baseUrl: account.baseUrl ? "[set]" : "[missing]",
    user: account.user || undefined,
    mode: "user-account",
  };
}

// ---- Ziele ---------------------------------------------------------------
// "nctalk:<raumtoken>" oder "nctalk:user:<benutzer>" (1:1 wird bei Bedarf angelegt).

export function stripTargetPrefix(raw) {
  return String(raw ?? "").trim().replace(TARGET_PREFIX_RE, "").trim();
}

export function parseTarget(raw) {
  const stripped = stripTargetPrefix(raw);
  if (!stripped) return null;
  const user = /^user:(.+)$/i.exec(stripped);
  if (user) return { kind: "user", id: user[1].trim() };
  const room = stripped.replace(/^room:/i, "").trim();
  return room ? { kind: "room", id: room } : null;
}

export function normalizeMessagingTarget(raw) {
  const target = parseTarget(raw);
  if (!target) return undefined;
  return target.kind === "user" ? `${CHANNEL_ID}:user:${target.id.toLowerCase()}` : `${CHANNEL_ID}:${target.id}`;
}

export function looksLikeTargetId(raw) {
  const trimmed = String(raw ?? "").trim();
  if (!trimmed) return false;
  if (/^(nctalk|talk|room|user):/i.test(trimmed)) return true;
  return /^[a-z0-9]{8,}$/i.test(trimmed);
}
