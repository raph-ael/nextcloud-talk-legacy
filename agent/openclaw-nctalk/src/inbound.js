// Eine eingehende Talk-Nachricht: Zugriff prüfen, Anhänge laden, Agentenlauf anstoßen,
// Live-Status und Antwort zustellen.
import { createChannelPairingController } from "openclaw/plugin-sdk/channel-pairing";
import { createChannelInboundEnvelopeBuilderAsync, logInboundDrop, toInboundMediaFacts } from "openclaw/plugin-sdk/channel-inbound";
import {
  bindIngressLifecycleToReplyOptions,
  createChannelDeliveryAccumulator,
  createChannelProgressDraftCompositor,
  createLivePreviewLifecycle,
  createPreviewMessageReceipt,
  resolveChannelStreamingPreviewToolProgress,
} from "openclaw/plugin-sdk/channel-outbound";
import { channelIngressRoutes } from "openclaw/plugin-sdk/channel-ingress-runtime";
import { deliverTextOrMediaReply, isReasoningReplyPayload, resolveSendableOutboundReplyParts } from "openclaw/plugin-sdk/reply-payload";
import { resolveAgentRoute } from "openclaw/plugin-sdk/routing";
import { resolveAllowlistProviderRuntimeGroupPolicy, resolveDefaultGroupPolicy } from "openclaw/plugin-sdk/runtime-group-policy";
import { getAgentScopedMediaLocalRoots } from "openclaw/plugin-sdk/media-local-roots";
import { sanitizeAssistantVisibleText } from "openclaw/plugin-sdk/text-chunking";
import { getGlobalHookRunner } from "openclaw/plugin-sdk/plugin-runtime";
import { normalizeUserId } from "./config.js";
import { createNctalkDraftStream } from "./draft.js";
import { CHANNEL_ID, getNctalkRuntime } from "./runtime.js";
import { sendMediaNctalk, sendTextNctalk } from "./send.js";

const NO_VISIBLE = { visibleReplySent: false, suppression: { reason: "no_visible_result" } };

function allowEntries(values) {
  return (values ?? []).map((v) => normalizeUserId(v)).filter(Boolean);
}

function resolveRoomConfig(rooms, token) {
  const map = rooms && typeof rooms === "object" ? rooms : {};
  const explicit = Object.keys(map).filter((k) => k !== "*");
  return {
    roomConfig: map[token] ?? undefined,
    wildcard: map["*"] ?? undefined,
    allowlistConfigured: explicit.length > 0 || Boolean(map["*"]),
    allowed: Boolean(map[token] || map["*"]),
  };
}

function mediaKind(contentType, isVoice) {
  const type = String(contentType ?? "");
  if (isVoice || type.startsWith("audio/")) return "audio";
  if (type.startsWith("image/")) return "image";
  if (type.startsWith("video/")) return "video";
  return "document";
}

async function loadAttachments({ message, client, account, core, runtime }) {
  const out = [];
  for (const file of message.files) {
    if (!file.path) {
      out.push({ fileName: file.name, contentType: file.mimetype, kind: mediaKind(file.mimetype, message.isVoice), messageId: message.messageId });
      continue;
    }
    try {
      if (file.size && file.size > account.mediaMaxBytes) throw new Error(`Datei ist zu groß (${file.size} Bytes)`);
      const downloaded = await client.downloadFile(file.path, { maxBytes: account.mediaMaxBytes });
      const contentType = file.mimetype || downloaded.contentType;
      const saved = await core.channel.media.saveMediaBuffer(downloaded.buffer, contentType, "inbound", account.mediaMaxBytes, file.name);
      out.push({
        path: saved.path,
        contentType: saved.contentType ?? contentType,
        fileName: file.name,
        kind: mediaKind(saved.contentType ?? contentType, message.isVoice),
        messageId: message.messageId,
      });
    } catch (err) {
      runtime.error?.(`nctalk: Anhang "${file.name}" konnte nicht geladen werden: ${err?.message ?? err}`);
      out.push({ fileName: file.name, contentType: file.mimetype, kind: mediaKind(file.mimetype, message.isVoice), messageId: message.messageId });
    }
  }
  return out;
}

export async function handleNctalkInbound(params) {
  const { message, account, config, runtime, statusSink, client, setTyping, turnAdoptionLifecycle } = params;
  const core = getNctalkRuntime();
  const pairing = createChannelPairingController({ core, channel: CHANNEL_ID, accountId: account.accountId });
  const isGroup = message.kind === "group";
  const { senderId, senderName, roomToken, roomName } = message;
  const rawBody = message.text ?? "";
  if (!rawBody && message.files.length === 0) {
    logInboundDrop({ log: (m) => runtime.log?.(m), channel: CHANNEL_ID, reason: "leere Nachricht", target: senderId });
    return;
  }
  statusSink?.({ lastInboundAt: message.timestamp });

  const roomMatch = resolveRoomConfig(account.config.rooms, roomToken);
  const roomConfig = roomMatch.roomConfig ?? roomMatch.wildcard;
  const allowTextCommands = core.channel.commands.shouldHandleTextCommands({ cfg: config, surface: CHANNEL_ID });
  const hasControlCommand = rawBody ? core.channel.text.hasControlCommand(rawBody, config) : false;
  const shouldRequireMention = isGroup ? roomConfig?.requireMention !== false : false;
  const { groupPolicy } = resolveAllowlistProviderRuntimeGroupPolicy({
    providerConfigPresent: config.channels?.[CHANNEL_ID] !== undefined,
    groupPolicy: account.config.groupPolicy,
    defaultGroupPolicy: resolveDefaultGroupPolicy(config),
  });
  const allowFrom = allowEntries(account.config.allowFrom);
  const outerGroupAllowFrom = account.config.groupAllowFrom?.length ? allowEntries(account.config.groupAllowFrom) : allowFrom;
  const roomAllowFrom = allowEntries(roomConfig?.allowFrom);
  const senderKey = normalizeUserId(senderId);

  const resolveAccess = async (wasMentioned, contextBinding) =>
    await core.channel.inbound.ingress.resolveStable({
      channelId: CHANNEL_ID,
      accountId: account.accountId,
      identity: {
        key: "nctalk-user-id",
        normalize: (value) => normalizeUserId(value) || null,
        sensitivity: "pii",
        entryIdPrefix: "nctalk-entry",
      },
      cfg: config,
      readStoreAllowFrom: async () => await pairing.readStoreForDmPolicy(CHANNEL_ID, account.accountId),
      subject: { stableId: senderId },
      conversation: { kind: isGroup ? "group" : "direct", id: isGroup ? roomToken : senderId },
      contextBinding,
      route: isGroup
        ? channelIngressRoutes(
            roomMatch.allowlistConfigured && {
              id: "nctalk:room",
              allowed: roomMatch.allowed,
              precedence: 0,
              matchId: "nctalk-room",
              blockReason: "room_not_allowlisted",
            },
            roomConfig?.enabled === false && {
              id: "nctalk:room-enabled",
              enabled: false,
              precedence: 10,
              blockReason: "room_disabled",
            },
            groupPolicy === "allowlist" && roomAllowFrom.length > 0 && {
              id: "nctalk:room-sender",
              kind: "nestedAllowlist",
              precedence: 20,
              blockReason: "room_sender_not_allowlisted",
              ...(outerGroupAllowFrom.length === 0
                ? { senderPolicy: "replace", senderAllowFrom: roomAllowFrom }
                : { allowed: roomAllowFrom.includes(senderKey) || roomAllowFrom.includes("*"), matchId: "nctalk-room-sender" }),
            },
          )
        : [],
      dmPolicy: account.config.dmPolicy ?? "pairing",
      groupPolicy,
      policy: {
        groupAllowFromFallbackToAllowFrom: true,
        activation: { requireMention: isGroup && shouldRequireMention, allowTextCommands },
      },
      mentionFacts: isGroup && wasMentioned !== undefined ? { canDetectMention: true, wasMentioned, hasAnyMention: wasMentioned } : undefined,
      allowFrom,
      groupAllowFrom: account.config.groupAllowFrom,
      command: { allowTextCommands, hasControlCommand },
    });

  let access = await resolveAccess();
  const accessReason = access.ingress.reasonCode === "route_blocked" ? "route blocked" : access.senderAccess.reasonCode;
  if (isGroup) {
    if (access.routeAccess.reason || access.senderAccess.decision !== "allow") {
      runtime.log?.(`nctalk: Nachricht in Raum ${roomToken} verworfen (${access.routeAccess.reason ?? accessReason})`);
      return;
    }
  } else if (access.senderAccess.decision !== "allow") {
    if (access.senderAccess.decision === "pairing") {
      await pairing.issueChallenge({
        senderId,
        senderIdLine: `Deine Nextcloud-Benutzer-ID: ${senderId}`,
        meta: { name: senderName || undefined },
        sendPairingReply: async (text) => {
          await sendTextNctalk(roomToken, text, { cfg: config, accountId: account.accountId });
          statusSink?.({ lastOutboundAt: Date.now() });
        },
        onReplyError: (err) => runtime.error?.(`nctalk: Kopplungsantwort an ${senderId} fehlgeschlagen: ${String(err)}`),
      });
    }
    runtime.log?.(`nctalk: Direktnachricht von ${senderId} verworfen (${accessReason})`);
    return;
  }
  if (access.commandAccess.shouldBlockControlCommand) {
    logInboundDrop({ log: (m) => runtime.log?.(m), channel: CHANNEL_ID, reason: "Steuerbefehl ohne Berechtigung", target: senderId });
    return;
  }

  const mentionRegexes = core.channel.mentions.buildMentionRegexes(config);
  const patternMention = rawBody && mentionRegexes.length ? core.channel.mentions.matchesMentionPatterns(rawBody, mentionRegexes) : false;
  const wasMentioned = message.mentionedSelf || patternMention || Boolean(message.replyTo?.isSelf);
  const route = resolveAgentRoute({
    cfg: config,
    channel: CHANNEL_ID,
    accountId: account.accountId,
    peer: { kind: isGroup ? "group" : "direct", id: isGroup ? roomToken : senderId },
  });
  access = await resolveAccess(isGroup ? wasMentioned : undefined, {
    agentId: route.agentId,
    sessionKey: route.sessionKey,
    messageId: message.messageId,
    inboundEventKind: "user_request",
  });
  if (access.ingress.admission !== "dispatch") {
    runtime.log?.(
      isGroup && access.activationAccess.shouldSkip
        ? `nctalk: Nachricht in Raum ${roomToken} übergangen (Agent nicht erwähnt)`
        : `nctalk: Nachricht in ${roomToken} verworfen (Berechtigung geändert)`,
    );
    return;
  }

  const attachments = message.files.length ? await loadAttachments({ message, client, account, core, runtime }) : [];
  const media = attachments.length ? toInboundMediaFacts(attachments, { messageId: message.messageId }) : undefined;

  const fromLabel = isGroup ? `room:${roomName || roomToken}` : senderName || `user:${senderId}`;
  const body = (await createChannelInboundEnvelopeBuilderAsync({ cfg: config, route }))({
    channel: "Nextcloud Talk",
    from: fromLabel,
    timestamp: message.timestamp,
    body: rawBody,
  });
  const ctxPayload = core.channel.inbound.buildContext({
    channelIngress: access,
    channel: CHANNEL_ID,
    accountId: route.accountId,
    messageId: message.messageId,
    timestamp: message.timestamp,
    from: isGroup ? `${CHANNEL_ID}:room:${roomToken}` : `${CHANNEL_ID}:${senderId}`,
    sender: { id: senderId, name: senderName || undefined },
    conversation: {
      kind: isGroup ? "group" : "direct",
      id: isGroup ? roomToken : senderId,
      label: fromLabel,
      nativeChannelId: roomToken,
    },
    route: {
      agentId: route.agentId,
      dmScope: route.dmScope,
      accountId: route.accountId,
      routeSessionKey: route.sessionKey,
    },
    reply: {
      to: `${CHANNEL_ID}:${roomToken}`,
      originatingTo: `${CHANNEL_ID}:${roomToken}`,
      nativeChannelId: roomToken,
      ...(message.replyTo ? { replyToId: message.replyTo.id } : {}),
    },
    message: {
      body,
      bodyForAgent: rawBody,
      rawBody,
      commandBody: rawBody,
      ...(message.isVoice ? { sourceModality: "voice" } : {}),
    },
    access: {
      commands: { authorized: access.commandAccess.authorized },
      mentions: { canDetectMention: isGroup, wasMentioned: isGroup && wasMentioned },
    },
    ...(media ? { media } : {}),
    ...(message.replyTo
      ? {
          supplemental: {
            quote: {
              id: message.replyTo.id,
              body: message.replyTo.text,
              sender: message.replyTo.senderName || message.replyTo.senderId,
              isSelf: message.replyTo.isSelf,
            },
          },
        }
      : {}),
    extra: {
      GroupSubject: isGroup ? roomName || roomToken : undefined,
      GroupSystemPrompt: isGroup ? roomConfig?.systemPrompt?.trim() || undefined : undefined,
    },
  });

  await dispatchNctalkTurn({ core, config, account, runtime, statusSink, client, setTyping, turnAdoptionLifecycle, route, ctxPayload, roomToken, roomConfig, isGroup, triggerMessageId: message.messageId });
}

// Erst ab so vielen Zeichen wird die mitlaufende Antwort sichtbar – ihre ersten Worte stehen in der Benachrichtigung.
const PARTIAL_MIN_INITIAL_CHARS = 40;

async function dispatchNctalkTurn({ core, config, account, runtime, statusSink, client, setTyping, turnAdoptionLifecycle, route, ctxPayload, roomToken, roomConfig, isGroup, triggerMessageId }) {
  // In Gruppen zitiert die Antwort die auslösende Nachricht; im 1:1 nur, wenn der Agent es ausdrücklich will.
  const draftReplyTo = isGroup ? triggerMessageId : undefined;
  const resolveReplyTo = (payload) => (payload.replyToId && (isGroup || payload.replyToTag === true) ? String(payload.replyToId) : undefined);
  const mode = account.streamingMode;
  const tableMode = core.channel.text.resolveMarkdownTableMode({ cfg: config, channel: CHANNEL_ID, accountId: account.accountId });
  const textLimit = core.channel.text.resolveTextChunkLimit(config, CHANNEL_ID, account.accountId, { fallbackLimit: account.textChunkLimit });
  const chunkMode = core.channel.text.resolveChunkMode(config, CHANNEL_ID, account.accountId);
  const chunk = (value) => core.channel.text.chunkMarkdownTextWithMode(value, textLimit, chunkMode);
  const warn = (m) => runtime.log?.(m);

  const hookRunner = getGlobalHookRunner();
  const hooksRewriteReplies = (hookRunner?.hasHooks("reply_payload_sending") ?? false) || (hookRunner?.hasHooks("message_sending") ?? false);
  const draftPreviewEnabled = mode !== "off" && !hooksRewriteReplies;
  // In "progress" ist die Statusnachricht still und wird nach der Antwort entfernt;
  // in "partial" ist sie die entstehende Antwort und löst die Benachrichtigung aus.
  const toolProgressInDraft = resolveChannelStreamingPreviewToolProgress(account.config, false, mode);
  const draftProgressEnabled = draftPreviewEnabled && (mode === "progress" || toolProgressInDraft);
  const draftStream = draftPreviewEnabled
    ? createNctalkDraftStream({
        client,
        token: roomToken,
        replyTo: draftReplyTo,
        silent: mode === "progress",
        maxChars: Math.min(textLimit, 4000),
        throttleMs: account.editIntervalMs,
        warn,
        renderText: (value) => core.channel.text.convertMarkdownTables(value, tableMode),
      })
    : undefined;

  const progressConfig = account.config.streaming?.progress;
  const progressEntry =
    progressConfig?.label === undefined && progressConfig?.labels === undefined
      ? { ...account.config, streaming: { ...account.config.streaming, progress: { ...progressConfig, label: "Arbeitet …" } } }
      : account.config;
  const progressDraft = createChannelProgressDraftCompositor({
    preparedItems: true,
    showWorkStatus: true,
    toolIcons: true,
    entry: progressEntry,
    mode,
    active: draftProgressEnabled,
    seed: `${account.accountId}:${roomToken}`,
    shouldStartNow: (line) => typeof line === "object" && line?.kind === "item",
    update: async (previewText, options) => {
      draftStream?.update(previewText);
      if (options?.flush) await draftStream?.flush();
    },
    deleteCurrent: () => draftStream?.deleteCurrent(),
  });

  const previewLifecycle = createLivePreviewLifecycle({
    draft: draftStream
      ? {
          flush: draftStream.flush,
          id: draftStream.messageId,
          seal: draftStream.seal,
          discardPending: draftStream.discardPending,
          clear: draftStream.clear,
        }
      : undefined,
    cleanupUndelivered: true,
    onFinalStarted: () => progressDraft.markFinalReplyStarted(),
    onFinalDelivered: () => progressDraft.markFinalReplyDelivered(),
    onCleanupFailure: (err) => warn(`nctalk: Aufräumen des Live-Status fehlgeschlagen: ${String(err)}`),
  });

  let lastPartialText = "";
  const updateDraftFromPartial = (text) => {
    const cleaned = text?.trim();
    if (!cleaned || cleaned === lastPartialText) return false;
    if (lastPartialText && lastPartialText.startsWith(cleaned) && cleaned.length < lastPartialText.length) return false;
    progressDraft.resetActivity({ suppressed: true });
    lastPartialText = cleaned;
    if (!draftStream?.messageId() && cleaned.length < PARTIAL_MIN_INITIAL_CHARS) return false;
    draftStream?.update(cleaned);
    return true;
  };

  const mediaLocalRoots = getAgentScopedMediaLocalRoots(config, route.agentId);
  const deliverNormally = async (payload) => {
    const reply = resolveSendableOutboundReplyParts(payload, {
      text: core.channel.text.convertMarkdownTables(sanitizeAssistantVisibleText(payload.text ?? ""), tableMode),
    });
    const replyTo = resolveReplyTo(payload);
    const accepted = createChannelDeliveryAccumulator({
      kind: reply.mediaUrls.length > 0 ? "media" : "text",
      ...(replyTo ? { replyToId: replyTo } : {}),
    });
    const base = { cfg: config, accountId: account.accountId, replyTo };
    try {
      await deliverTextOrMediaReply({
        payload,
        text: reply.text,
        chunkText: chunk,
        sendText: async (text) => {
          const sent = await sendTextNctalk(roomToken, text, base);
          accepted.add({ receipt: sent.receipt }, sent.content);
        },
        sendMedia: async ({ mediaUrl, caption }) => {
          const sent = await sendMediaNctalk(roomToken, caption ?? "", mediaUrl, { ...base, mediaLocalRoots, audioAsVoice: payload.audioAsVoice === true });
          accepted.add({ receipt: sent.receipt }, sent.content);
        },
      });
    } catch (error) {
      throw accepted.partialError(error);
    }
    const result = accepted.result();
    if (result.visibleReplySent) statusSink?.({ lastOutboundAt: Date.now() });
    return result;
  };

  // Stille Quittung: zeigt sofort, dass die Nachricht angenommen ist. Sie ist die spätere Statusnachricht;
  // kommt die Antwort ohne Zwischenstand, räumt das Aufräumen am Ende sie wieder ab.
  if (mode === "progress" && draftStream && account.ackText) {
    draftStream.update(account.ackText);
    void draftStream.flush().catch((err) => warn(`nctalk: Quittung fehlgeschlagen: ${String(err)}`));
  }

  const typingEnabled = account.typing && typeof setTyping === "function";
  try {
    await core.channel.inbound.dispatch({
      cfg: config,
      channel: CHANNEL_ID,
      accountId: account.accountId,
      route: { agentId: route.agentId, dmScope: route.dmScope, sessionKey: route.sessionKey },
      ctxPayload,
      delivery: {
        deliver: async (payload, info) => {
          if (isReasoningReplyPayload(payload)) return NO_VISIBLE;
          const outcome = await previewLifecycle.deliver({
            kind: info.kind,
            payload,
            isError: payload.isError,
            adapter:
              mode === "partial" && draftStream
                ? {
                    // Die entstehende Antwort wird am Ende an Ort und Stelle fertiggestellt.
                    buildFinalEdit: (p) => {
                      const hasMedia = Boolean(p.mediaUrl) || (p.mediaUrls?.length ?? 0) > 0;
                      if (hasMedia || p.isError || typeof p.text !== "string") return undefined;
                      // Ein Zitat lässt sich nicht nachträglich anbringen.
                      if ((resolveReplyTo(p) ?? undefined) !== (draftReplyTo ?? undefined)) return undefined;
                      const formatted = core.channel.text.convertMarkdownTables(sanitizeAssistantVisibleText(p.text), tableMode).trim();
                      const chunks = chunk(formatted);
                      if (chunks.length !== 1 || !chunks[0]?.trim()) return undefined;
                      return { message: chunks[0].trim() };
                    },
                    editFinal: async (id, edit) => {
                      if (draftStream.lastSentText() !== edit.message) await client.editMessage(roomToken, id, edit.message);
                      const receipt = createPreviewMessageReceipt({ id: String(id) });
                      statusSink?.({ lastOutboundAt: Date.now() });
                      return { messageIds: receipt.platformMessageIds, receipt, visibleReplySent: true, content: edit.message };
                    },
                    logPreviewEditFailure: (err) => warn(`nctalk: Fertigstellen an Ort und Stelle fehlgeschlagen, sende neu (${String(err)})`),
                  }
                : undefined,
            deliverNormally,
          });
          return outcome.deliveryResult ?? NO_VISIBLE;
        },
        onError: (err, info) => runtime.error?.(`nctalk: Zustellung (${info.kind}) fehlgeschlagen: ${String(err)}`),
      },
      replyPipeline: typingEnabled
        ? {
            typing: {
              start: async () => {
                setTyping(roomToken, true);
              },
              stop: async () => {
                setTyping(roomToken, false);
              },
              onStartError: (err) => runtime.log?.(`nctalk: Tipp-Anzeige fehlgeschlagen: ${String(err)}`),
              keepaliveIntervalMs: 8000,
              maxDurationMs: 20 * 60 * 1000,
            },
          }
        : {},
      replyOptions: {
        ...(turnAdoptionLifecycle ? bindIngressLifecycleToReplyOptions(turnAdoptionLifecycle) : {}),
        skillFilter: roomConfig?.skills,
        progressPreambleEnabled: draftProgressEnabled,
        commentaryProgressEnabled: progressDraft.commentaryProgressEnabled,
        allowProgressCallbacksWhenSourceDeliverySuppressed: draftProgressEnabled ? true : undefined,
        preserveProgressCallbackStartOrder: draftPreviewEnabled ? true : undefined,
        onObservedReplyDelivery: draftPreviewEnabled ? () => previewLifecycle.observeDelivery({ visibleReplySent: true }) : undefined,
        disableBlockStreaming: draftPreviewEnabled ? true : undefined,
        ...(draftPreviewEnabled ? { suppressDefaultToolProgressMessages: true } : {}),
        ...(draftPreviewEnabled
          ? {
              onPartialReply: (partial) => (mode === "progress" ? false : updateDraftFromPartial(partial.text)),
              onAssistantMessageStart: () => {
                lastPartialText = "";
                progressDraft.beginAssistantMessage();
                return false;
              },
              onReasoningEnd: () => {
                progressDraft.resetReasoningProgress();
                return false;
              },
              onReasoningStream: async (partial) =>
                mode === "progress"
                  ? await progressDraft.pushReasoningProgress(partial.text || "Denkt nach …", { snapshot: partial.isReasoningSnapshot === true })
                  : false,
              onPlanUpdate: async (plan) => {
                if (!draftProgressEnabled || plan.phase !== "update") return false;
                return await progressDraft.pushPlanProgress(plan.steps, { explanation: plan.explanation, explanationFormat: plan.explanationFormat });
              },
              onToolStart: (tool) => progressDraft.pushToolEvent(tool),
              onItemEvent: async (item) => {
                if (!draftProgressEnabled) return false;
                return await progressDraft.pushItemEvent(item);
              },
              onApprovalEvent: async (approval) => (draftProgressEnabled ? await progressDraft.pushApprovalEvent(approval) : false),
            }
          : {}),
      },
      record: {
        onRecordError: (err) => runtime.error?.(`nctalk: Sitzungsdaten konnten nicht aktualisiert werden: ${String(err)}`),
      },
    });
  } finally {
    try {
      await draftStream?.stop();
      await previewLifecycle.cleanup();
    } catch (err) {
      warn(`nctalk: Aufräumen des Live-Status fehlgeschlagen: ${String(err)}`);
    }
    if (typingEnabled) setTyping(roomToken, false);
  }
}
