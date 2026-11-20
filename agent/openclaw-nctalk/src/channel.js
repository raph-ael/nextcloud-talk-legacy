// Kanal "nctalk": Nextcloud Talk, bei dem der Agent ein echtes Benutzerkonto ist.
import { buildChannelOutboundSessionRoute, createChatChannelPlugin } from "openclaw/plugin-sdk/channel-core";
import { createScopedDmSecurityResolver } from "openclaw/plugin-sdk/channel-config-helpers";
import { createLoggedPairingApprovalNotifier, createPairingPrefixStripper } from "openclaw/plugin-sdk/channel-pairing";
import { createAccountStatusSink, defineChannelMessageAdapter, runPassiveAccountLifecycle } from "openclaw/plugin-sdk/channel-outbound";
import { jsonResult, readStringParam, resolveReactionMessageId } from "openclaw/plugin-sdk/channel-actions";
import { sanitizeAssistantVisibleText } from "openclaw/plugin-sdk/text-chunking";
import {
  DEFAULT_ACCOUNT_ID,
  hasNctalkConfiguredState,
  inspectNctalkAccount,
  isNctalkAccountConfigured,
  looksLikeTargetId,
  normalizeMessagingTarget,
  normalizeUserId,
  parseTarget,
  resolveNctalkAccount,
  section,
} from "./config.js";
import { monitorNctalkProvider, setNctalkTyping } from "./monitor.js";
import { CHANNEL_ID, getNctalkRuntime } from "./runtime.js";
import { deleteNctalk, editNctalk, getNctalkClientEntry, reactNctalk, resolveRoomToken, sendMediaNctalk, sendTextNctalk } from "./send.js";

const TARGET_PREFIX_RE = /^(nctalk|talk):/i;

const meta = {
  id: CHANNEL_ID,
  label: "Nextcloud Talk (Benutzerkonto)",
  selectionLabel: "Nextcloud Talk (Agent als Benutzerkonto)",
  docsPath: "/channels/nextcloud-talk",
  docsLabel: "nctalk",
  blurb: "Agent als echtes Nextcloud-Konto: Live-Status, Tipp-Anzeige, Dateien, Sprachnachrichten.",
  aliases: ["talk"],
  order: 66,
};

// ---- Nachrichten senden -----------------------------------------------------
function sendOpts(ctx) {
  return {
    cfg: ctx.cfg,
    accountId: ctx.accountId ?? undefined,
    replyTo: ctx.replyToId ?? undefined,
    silent: ctx.silent === true,
    onPlatformSendDispatch: ctx.onPlatformSendDispatch,
    mediaLocalRoots: ctx.mediaLocalRoots ?? ctx.mediaAccess?.localRoots,
    mediaReadFile: ctx.mediaReadFile ?? ctx.mediaAccess?.readFile,
    workspaceDir: ctx.mediaAccess?.workspaceDir,
    audioAsVoice: ctx.audioAsVoice === true,
  };
}

const messageAdapter = defineChannelMessageAdapter({
  id: CHANNEL_ID,
  durableFinal: { capabilities: { text: true, media: true, replyTo: true } },
  send: {
    text: (ctx) => sendTextNctalk(ctx.to, ctx.text, sendOpts(ctx)),
    media: (ctx) => (ctx.mediaUrl ? sendMediaNctalk(ctx.to, ctx.text, ctx.mediaUrl, sendOpts(ctx)) : sendTextNctalk(ctx.to, ctx.text, sendOpts(ctx))),
  },
});

// ---- Aktionen des "message"-Werkzeugs ----------------------------------------
function configured(cfg, accountId) {
  const account = resolveNctalkAccount(cfg, accountId);
  return account.enabled && isNctalkAccountConfigured(account);
}

const messageActions = {
  describeMessageTool: ({ cfg, accountId }) => {
    if (!configured(cfg, accountId)) return null;
    return { actions: ["send", "react", "edit", "delete"] };
  },
  supportsAction: ({ action }) => action === "react" || action === "edit" || action === "delete",
  handleAction: async ({ action, params, cfg, accountId, toolContext }) => {
    if (action === "send") throw new Error("Senden läuft über die Zustellung, nicht über die Aktionen.");
    if (!configured(cfg, accountId)) throw new Error("nctalk: Kanal ist nicht eingerichtet.");
    const to = readStringParam(params, "to", { required: true, label: "to (Raumtoken oder user:<benutzer>)" });
    const opts = { cfg, accountId: accountId ?? undefined };
    if (action === "react") {
      const messageId = resolveReactionMessageId({ args: params, toolContext });
      if (messageId == null) throw new Error("messageId wird benötigt");
      const emoji = readStringParam(params, "emoji", { required: true });
      const remove = params.remove === true;
      await reactNctalk(to, String(messageId), emoji, { ...opts, remove });
      return jsonResult({ ok: true, ...(remove ? { removed: emoji } : { added: emoji }) });
    }
    const messageId = readStringParam(params, "messageId", { required: true });
    if (action === "edit") {
      const text = readStringParam(params, "message", { required: true, allowEmpty: false }) ?? readStringParam(params, "text", { required: true });
      await editNctalk(to, messageId, text, opts);
      return jsonResult({ ok: true, edited: messageId });
    }
    if (action === "delete") {
      await deleteNctalk(to, messageId, opts);
      return jsonResult({ ok: true, deleted: messageId });
    }
    throw new Error(`Aktion ${action} wird von ${CHANNEL_ID} nicht unterstützt.`);
  },
};

// ---- Gateway: Konto starten/stoppen ---------------------------------------------
const gatewayAdapter = {
  startAccount: async (ctx) => {
    const account = ctx.account;
    if (!isNctalkAccountConfigured(account) || !account.password) {
      throw new Error("nctalk: nicht eingerichtet (channels.nctalk.baseUrl, user und passwordFile setzen)");
    }
    ctx.log?.info(`[${account.accountId}] nctalk: Empfang wird gestartet (${account.user})`);
    const statusSink = createAccountStatusSink({ accountId: ctx.accountId, setStatus: ctx.setStatus });
    await runPassiveAccountLifecycle({
      abortSignal: ctx.abortSignal,
      start: async () =>
        await monitorNctalkProvider({
          account,
          cfg: ctx.cfg,
          runtime: ctx.runtime,
          abortSignal: ctx.abortSignal,
          statusSink,
          log: ctx.log,
        }),
      stop: async (monitor) => {
        await monitor.stop();
      },
    });
  },
};

// ---- Status -------------------------------------------------------------------
async function probeAccount({ account, timeoutMs }) {
  if (!isNctalkAccountConfigured(account) || !account.password) return { ok: false, code: "not_configured", message: "nctalk ist nicht eingerichtet." };
  try {
    const entry = getNctalkClientEntry(account);
    const self = await Promise.race([
      entry.client.whoami(),
      new Promise((_, reject) => setTimeout(() => reject(new Error("Zeitüberschreitung")), timeoutMs ?? 8000)),
    ]);
    return { ok: true, code: "ok", message: `angemeldet als ${self.id}`, user: self.id };
  } catch (err) {
    return { ok: false, code: "login_failed", message: `Anmeldung fehlgeschlagen: ${err?.message ?? err}` };
  }
}

export const nctalkPlugin = createChatChannelPlugin({
  base: {
    id: CHANNEL_ID,
    meta,
    capabilities: {
      chatTypes: ["direct", "group"],
      reactions: true,
      edit: true,
      unsend: true,
      reply: true,
      threads: false,
      media: true,
      nativeCommands: false,
      blockStreaming: true,
    },
    reload: { configPrefixes: [`channels.${CHANNEL_ID}`] },
    config: {
      listAccountIds: () => [DEFAULT_ACCOUNT_ID],
      defaultAccountId: () => DEFAULT_ACCOUNT_ID,
      resolveAccount: (cfg, accountId) => resolveNctalkAccount(cfg, accountId),
      inspectAccount: (cfg, accountId) => inspectNctalkAccount(cfg, accountId),
      isEnabled: (account) => account.enabled,
      isConfigured: (account) => isNctalkAccountConfigured(account),
      hasConfiguredState: ({ cfg }) => hasNctalkConfiguredState(cfg),
      describeAccount: (account) => ({
        accountId: account.accountId,
        name: account.name,
        enabled: account.enabled,
        configured: isNctalkAccountConfigured(account),
        tokenStatus: account.tokenStatus,
        mode: "user-account",
      }),
      resolveAllowFrom: ({ cfg }) => section(cfg).allowFrom ?? [],
      formatAllowFrom: ({ allowFrom }) => (allowFrom ?? []).map((entry) => normalizeUserId(entry)).filter(Boolean),
    },
    setup: {
      applyAccountConfig: ({ cfg, input }) => ({
        ...cfg,
        channels: { ...cfg.channels, [CHANNEL_ID]: { ...section(cfg), ...input } },
      }),
    },
    groups: {
      resolveRequireMention: ({ cfg, groupId }) => {
        const rooms = section(cfg).rooms ?? {};
        return (rooms[groupId] ?? rooms["*"])?.requireMention !== false;
      },
    },
    messaging: {
      targetPrefixes: [CHANNEL_ID, "talk"],
      normalizeTarget: normalizeMessagingTarget,
      inferTargetChatType: ({ to }) => {
        const target = parseTarget(to);
        if (!target) return undefined;
        return target.kind === "user" ? "direct" : undefined;
      },
      resolveOutboundSessionRoute: (params) => {
        const target = parseTarget(params.target);
        if (!target) return null;
        const isUser = target.kind === "user";
        return buildChannelOutboundSessionRoute({
          cfg: params.cfg,
          agentId: params.agentId,
          channel: CHANNEL_ID,
          accountId: params.accountId,
          recipientSessionExact: false,
          peer: { kind: isUser ? "direct" : "group", id: target.id },
          chatType: isUser ? "direct" : "group",
          from: isUser ? `${CHANNEL_ID}:${target.id}` : `${CHANNEL_ID}:room:${target.id}`,
          to: isUser ? `${CHANNEL_ID}:user:${target.id}` : `${CHANNEL_ID}:${target.id}`,
        });
      },
      targetResolver: { looksLikeId: looksLikeTargetId, hint: "<raumtoken> oder user:<benutzer>" },
    },
    agentPrompt: {
      inboundFormattingHints: () => ({
        text_markup: "markdown",
        rules: [
          "Nextcloud Talk stellt Markdown dar: **fett**, *kursiv*, `Code`, Codeblöcke, Listen, Zitate, Überschriften und Links.",
          "Erwähnungen schreibst du als @benutzername.",
        ],
      }),
    },
    heartbeat: {
      sendTyping: async ({ to, accountId, cfg }) => {
        const account = resolveNctalkAccount(cfg, accountId);
        if (!account.typing) return;
        const token = await resolveRoomToken(getNctalkClientEntry(account), to).catch(() => undefined);
        if (token) setNctalkTyping(account.accountId, token, true);
      },
      clearTyping: async ({ to, accountId, cfg }) => {
        const account = resolveNctalkAccount(cfg, accountId);
        const token = await resolveRoomToken(getNctalkClientEntry(account), to).catch(() => undefined);
        if (token) setNctalkTyping(account.accountId, token, false);
      },
    },
    status: {
      defaultRuntime: { accountId: DEFAULT_ACCOUNT_ID, running: false, lastStartAt: null, lastStopAt: null, lastError: null },
      probeAccount,
      buildAccountSnapshot: ({ account, runtime, probe }) => ({
        accountId: account.accountId,
        name: account.name,
        enabled: account.enabled,
        configured: isNctalkAccountConfigured(account),
        running: runtime?.running ?? false,
        connected: runtime?.connected,
        lastStartAt: runtime?.lastStartAt ?? null,
        lastStopAt: runtime?.lastStopAt ?? null,
        lastError: runtime?.lastError ?? null,
        lastInboundAt: runtime?.lastInboundAt ?? null,
        lastOutboundAt: runtime?.lastOutboundAt ?? null,
        mode: "user-account",
        probe,
      }),
    },
    gateway: gatewayAdapter,
    message: messageAdapter,
    actions: messageActions,
  },
  pairing: {
    text: {
      idLabel: "nextcloudUserId",
      message: "OpenClaw: Dein Zugang wurde freigegeben.",
      normalizeAllowEntry: createPairingPrefixStripper(TARGET_PREFIX_RE, (entry) => normalizeUserId(entry)),
      notify: createLoggedPairingApprovalNotifier(({ id }) => `[nctalk] Benutzer ${id} wurde freigegeben`),
    },
  },
  security: {
    resolveDmPolicy: createScopedDmSecurityResolver({
      channelKey: CHANNEL_ID,
      resolvePolicy: (account) => account.config.dmPolicy,
      resolveAllowFrom: (account) => account.config.allowFrom,
      policyPathSuffix: "dmPolicy",
      normalizeEntry: (raw) => normalizeUserId(raw),
    }),
  },
  outbound: {
    base: {
      deliveryMode: "direct",
      chunker: (text, limit) => getNctalkRuntime().channel.text.chunkMarkdownText(text, limit),
      chunkerMode: "markdown",
      textChunkLimit: 8000,
      sanitizeText: ({ text }) => sanitizeAssistantVisibleText(text),
    },
    attachedResults: {
      channel: CHANNEL_ID,
      sendText: messageAdapter.send.text,
      sendMedia: (ctx) => messageAdapter.send.media({ ...ctx, mediaUrl: ctx.mediaUrl ?? "", onDeliveryResult: undefined }),
    },
  },
});
