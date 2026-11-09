import { useEffect, useRef, useState, type PointerEvent, type ReactNode } from 'react'
import type { AudioInfo, VoiceDraft } from '@shared/core'
import type { UiMessage } from '../store'
import {
  audioInfoFor,
  bars,
  clock,
  cycleRate,
  draftSource,
  maxMinutesLeft,
  messageSource,
  seek,
  togglePlay,
  usePlayer,
  useRate,
  type Recording,
  type Source,
} from '../voice'

/** Signal Desktop: 47 bars of 2 px, 4..20 px high, a 6 px position dot. */
const BAR_COUNT = 47
const BAR_MIN = 4
const BAR_MAX = 20

function useInfo(src: Source, size: number, auto: boolean): AudioInfo | null {
  const [info, setInfo] = useState<AudioInfo | null>(null)
  const player = usePlayer(src.key)
  const wanted = auto || !!player
  useEffect(() => {
    let alive = true
    audioInfoFor(src, size, wanted)
      ?.then((i) => alive && setInfo(i))
      .catch(() => undefined)
    return () => {
      alive = false
    }
    // The source key identifies the file; `resolve` changes identity every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src.key, size, wanted])
  return info
}

/** Ring on the play button's edge: upload progress (0..1) or loading (-1). */
function EdgeRing({ progress }: { progress: number }) {
  const r = 17
  const c = 2 * Math.PI * r
  return (
    <svg className={`voice-ring${progress < 0 ? ' spin' : ''}`} viewBox="0 0 36 36" width="36" height="36" aria-hidden>
      <circle
        cx="18"
        cy="18"
        r={r}
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeDasharray={c}
        strokeDashoffset={progress < 0 ? c * 0.7 : c * (1 - Math.min(1, Math.max(0.03, progress)))}
        transform="rotate(-90 18 18)"
      />
    </svg>
  )
}

/**
 * Play button and waveform (click/drag to seek), then a footer with time, speed and `meta`
 * (message time and ticks) – for a message or a draft.
 */
function VoicePlayer({
  src,
  size,
  auto,
  fallbackMs,
  upload,
  meta,
}: {
  src: Source
  size: number
  auto: boolean
  fallbackMs?: number
  upload?: number
  meta?: ReactNode
}) {
  const player = usePlayer(src.key)
  const rate = useRate()
  const info = useInfo(src, size, auto)
  const wave = useRef<HTMLDivElement>(null)
  const dragging = useRef(false)

  const duration = info?.durationMs || player?.durationMs || fallbackMs || 0
  const position = player?.positionMs ?? 0
  const progress = duration > 0 ? Math.min(1, position / duration) : 0
  const levels = bars(info?.waveform ?? [], BAR_COUNT)
  const active = !!player && (player.playing || position > 0)

  function fraction(e: PointerEvent): number {
    const r = wave.current!.getBoundingClientRect()
    return (e.clientX - r.left) / r.width
  }

  return (
    <div
      className={`voice${active ? ' active' : ''}`}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      <button
        className="voice-play"
        aria-label={player?.playing ? 'Pause' : 'Abspielen'}
        title={player?.error ?? (player?.playing ? 'Pause' : 'Abspielen')}
        onClick={() => void togglePlay(src)}
      >
        {upload !== undefined ? <EdgeRing progress={upload} /> : player?.loading && <EdgeRing progress={-1} />}
        {player?.playing ? (
          <svg viewBox="0 0 24 24" width="18" height="18">
            <path fill="currentColor" d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z" />
          </svg>
        ) : (
          <svg viewBox="0 0 24 24" width="18" height="18">
            <path fill="currentColor" d="M8 5.5v13c0 .8.9 1.3 1.6.8l10-6.5c.6-.4.6-1.2 0-1.6l-10-6.5c-.7-.5-1.6 0-1.6.8z" />
          </svg>
        )}
      </button>
      <div className="voice-body">
        <div
          className="waveform"
          ref={wave}
          role="slider"
          aria-label="Position"
          aria-valuemin={0}
          aria-valuemax={Math.round(duration / 1000)}
          aria-valuenow={Math.round(position / 1000)}
          tabIndex={0}
          onPointerDown={(e) => {
            dragging.current = true
            e.currentTarget.setPointerCapture(e.pointerId)
            seek(src, fraction(e))
          }}
          onPointerMove={(e) => dragging.current && player && seek(src, fraction(e))}
          onPointerUp={() => (dragging.current = false)}
          onKeyDown={(e) => {
            if (!duration) return
            const step = 5000 / duration
            if (e.key === 'ArrowRight') seek(src, progress + step)
            else if (e.key === 'ArrowLeft') seek(src, progress - step)
          }}
        >
          {levels.map((v, i) => (
            <i
              key={i}
              className={(i + 0.5) / BAR_COUNT <= progress ? 'played' : ''}
              style={{ height: BAR_MIN + v * (BAR_MAX - BAR_MIN) }}
            />
          ))}
          <span className="voice-dot" style={{ left: `calc(${progress} * (100% - 6px))` }} />
        </div>
        <div className="voice-meta">
          <span className="voice-time">{clock(active ? duration - position : duration)}</span>
          {active && (
            <button className="voice-rate" title="Wiedergabetempo" onClick={cycleRate}>
              {String(rate).replace('.', ',')}×
            </button>
          )}
          {player?.error && <span className="voice-error">{player.error}</span>}
          {meta && <span className="voice-meta-end">{meta}</span>}
        </div>
      </div>
    </div>
  )
}

/** Voice message (or any audio file) inside a bubble; `meta` goes to the end of its footer. */
export function VoiceMessage({ msg, meta }: { msg: UiMessage; meta?: ReactNode }) {
  const a = msg.attachment!
  const src = messageSource(msg)
  const uploading = msg.status === 'sending' && (msg.progress ?? -1) >= 0 && (msg.progress ?? 0) < 1
  return (
    <div className="voice-msg">
      {!a.voiceMessage && <div className="voice-name ellipsis">{a.name}</div>}
      <VoicePlayer
        src={src}
        size={a.size}
        auto={!!msg.status || a.voiceMessage}
        upload={uploading ? msg.progress! : undefined}
        meta={meta}
      />
    </div>
  )
}

/** Composer while recording: cancel · ● timer · live level · send. */
export function RecordingBar({ rec, onCancel, onSend }: { rec: Recording; onCancel: () => void; onSend: () => void }) {
  const levels = [...new Array<number>(Math.max(0, 48 - rec.levels.length)).fill(0), ...rec.levels]
  return (
    <div className="voice-bar recording" role="group" aria-label="Sprachnachricht wird aufgenommen">
      <button className="icon-button" aria-label="Aufnahme abbrechen" title="Abbrechen (Esc)" onClick={onCancel}>
        <svg viewBox="0 0 24 24" width="20" height="20">
          <path
            fill="currentColor"
            d="M18.3 5.7 12 12l6.3 6.3-1.4 1.4L12 13.4l-6.3 6.3-1.4-1.4L10.6 12 4.3 5.7l1.4-1.4L12 10.6l6.3-6.3z"
          />
        </svg>
      </button>
      <div className="voice-pill">
        <span className="rec-dot" />
        <span className="voice-time rec-time">{clock(rec.durationMs)}</span>
        <div className="level-meter" aria-hidden>
          {levels.map((v, i) => (
            <i key={i} style={{ height: 3 + v * 17 }} />
          ))}
        </div>
        {rec.warn && <span className="rec-warn">Noch {maxMinutesLeft(rec.durationMs)} Min.</span>}
      </div>
      <button className="send" aria-label="Sprachnachricht senden" title="Senden" onClick={onSend} disabled={rec.starting}>
        <SendIcon />
      </button>
    </div>
  )
}

/** Composer with an unsent recording: delete · player · send. */
export function DraftBar({ draft, onDiscard, onSend }: { draft: VoiceDraft; onDiscard: () => void; onSend: () => void }) {
  return (
    <div className="voice-bar draft" role="group" aria-label="Entwurf einer Sprachnachricht">
      <button className="icon-button" aria-label="Entwurf löschen" title="Löschen" onClick={onDiscard}>
        <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.6">
          <path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12" strokeLinejoin="round" />
        </svg>
      </button>
      <div className="voice-pill">
        <VoicePlayer src={draftSource(draft)} size={draft.size} auto fallbackMs={draft.durationMs} />
      </div>
      <button className="send" aria-label="Sprachnachricht senden" title="Senden" onClick={onSend}>
        <SendIcon />
      </button>
    </div>
  )
}

export function MicIcon() {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" />
    </svg>
  )
}

function SendIcon() {
  return (
    <svg viewBox="0 0 24 24" width="20" height="20">
      <path
        fill="currentColor"
        d="M3.4 20.4l17.4-7.5c.8-.4.8-1.5 0-1.8L3.4 3.6c-.7-.3-1.4.3-1.2 1l1.7 6.2 9.1 1.2-9.1 1.2-1.7 6.2c-.2.7.5 1.3 1.2 1z"
      />
    </svg>
  )
}
