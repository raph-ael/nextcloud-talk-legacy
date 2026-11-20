// Eine Statusnachricht, die per Bearbeiten fortgeschrieben wird ("Live-Status").
// Erstes Update sendet die Nachricht, alle weiteren bearbeiten sie – gedrosselt,
// weil Talk für jede Bearbeitung eine (unsichtbare) Systemnachricht anlegt.
import { randomBytes } from "node:crypto";
import { createFinalizableDraftLifecycle } from "openclaw/plugin-sdk/channel-outbound";

const DEFAULT_THROTTLE_MS = 1200;

function clip(text, maxChars) {
  const trimmed = String(text ?? "").trim();
  if (trimmed.length <= maxChars) return trimmed;
  return trimmed.slice(0, Math.max(0, maxChars - 1)).trimEnd() + "…";
}

export function createNctalkDraftStream({ client, token, replyTo, silent, maxChars, throttleMs, warn, renderText }) {
  const state = { stopped: false, final: false };
  let messageId;
  let lastSentText = "";

  const sendOrEditStreamMessage = async (text) => {
    const normalized = clip(renderText ? renderText(text) : text, maxChars);
    if (!normalized) return false;
    if (state.stopped && !state.final) return false;
    if (normalized === lastSentText) return true;
    try {
      if (messageId) {
        await client.editMessage(token, messageId, normalized);
      } else {
        const sent = await client.sendMessage(token, {
          message: normalized,
          replyTo,
          silent,
          referenceId: randomBytes(32).toString("hex"),
        });
        messageId = Number(sent.id);
      }
      lastSentText = normalized;
      return true;
    } catch (err) {
      state.stopped = true;
      warn?.(`nctalk: Live-Status konnte nicht aktualisiert werden: ${err?.message ?? err}`);
      return false;
    }
  };

  const lifecycle = createFinalizableDraftLifecycle({
    throttleMs: Math.max(500, throttleMs ?? DEFAULT_THROTTLE_MS),
    coalesceInFlight: true,
    state,
    sendOrEditStreamMessage,
    readMessageId: () => messageId,
    clearMessageId: () => {
      messageId = undefined;
    },
    isValidMessageId: (value) => typeof value === "number" && Number.isFinite(value) && value > 0,
    deleteMessage: (id) => client.deleteMessage(token, id),
    warn,
    warnPrefix: "nctalk: Statusnachricht konnte nicht entfernt werden",
  });

  return {
    update: (text) => lifecycle.update(text),
    flush: () => lifecycle.loop.flush(),
    messageId: () => messageId,
    lastSentText: () => lastSentText,
    seal: () => lifecycle.seal(),
    discardPending: () => lifecycle.stopForClear(),
    clear: () => lifecycle.clear(),
    stop: () => lifecycle.stop(),
    /** Entfernt die aktuelle Statusnachricht, lässt den Strom aber für eine neue offen. */
    async deleteCurrent() {
      lifecycle.loop.resetPending();
      await lifecycle.loop.waitForInFlight();
      const id = messageId;
      if (!id) return;
      messageId = undefined;
      lastSentText = "";
      lifecycle.loop.resetThrottleWindow();
      try {
        await client.deleteMessage(token, id);
      } catch (err) {
        warn?.(`nctalk: Statusnachricht konnte nicht entfernt werden: ${err?.message ?? err}`);
      }
    },
  };
}
