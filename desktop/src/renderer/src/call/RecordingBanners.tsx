// Banners for finished recordings (docs/calls.md, „Nach dem Anruf“): in the conversation list
// all of them, in a chat those of that conversation.

import { useState } from 'react'
import { recordingNoticeTitle, type RecordingNotice } from '@shared/core'
import { openRoom } from '../store'
import { isFailure, noticeAction, useNoticeBusy, useRecordingNotices } from './recordings'
import { errorText, toast } from './store'

/** `token`: only this conversation (chat); without it all (conversation list, with names). */
export function RecordingBanners({ token }: { token?: string }) {
  const items = useRecordingNotices(token)
  const [all, setAll] = useState(false)
  if (items.length === 0) return null
  // Recordings are frequent: the newest few, the rest on request (the chats stay in view).
  const limit = token ? 3 : 2
  const shown = all || items.length <= limit + 1 ? items : items.slice(0, limit)
  const more = items.length - shown.length
  return (
    <div className={`rec-banners${token ? ' in-chat' : ''}`} role="region" aria-label="Aufnahmen">
      {shown.map((n) => (
        <Banner key={n.id} n={n} withRoom={!token} />
      ))}
      {more > 0 && (
        <button className="rec-banners-more" onClick={() => setAll(true)}>
          {more} weitere anzeigen
        </button>
      )}
      {all && items.length > limit + 1 && (
        <button className="rec-banners-more" onClick={() => setAll(false)}>
          Weniger anzeigen
        </button>
      )}
    </div>
  )
}

function Banner({ n, withRoom }: { n: RecordingNotice; withRoom: boolean }) {
  const busy = useNoticeBusy(n.id)
  const failed = isFailure(n)
  const run = (share: boolean) => () =>
    void noticeAction(n.id, share).then((err) => err && toast(errorText(err)))
  const title = recordingNoticeTitle(n)
  return (
    <div className={`rec-banner${failed ? ' failed' : ''}`}>
      <button
        className="rec-banner-text"
        onClick={() => void openRoom(n.token)}
        disabled={!withRoom}
        title={withRoom ? `${n.roomName} öffnen` : undefined}
      >
        <strong>
          {title}
          {n.fileName && <span className="rec-banner-file"> · {n.fileName}</span>}
        </strong>
        {withRoom && <span className="muted ellipsis">{n.roomName}</span>}
      </button>
      <div className="rec-banner-actions">
        {failed ? (
          <button className="rec-banner-btn" onClick={run(false)} disabled={busy}>
            OK
          </button>
        ) : (
          <>
            {n.canShare && (
              <button className="rec-banner-btn primary" onClick={run(true)} disabled={busy}>
                In Chat teilen
              </button>
            )}
            <button className="rec-banner-btn" onClick={run(false)} disabled={busy}>
              Verwerfen
            </button>
          </>
        )}
      </div>
    </div>
  )
}
