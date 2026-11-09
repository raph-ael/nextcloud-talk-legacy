// Call tones, generated with WebAudio (no sound files): ringtone, ringback (German/European
// 425 Hz, 1 s on / 4 s off), connected and hang-up. Loops are scheduled far ahead on the audio
// clock, so a hidden (throttled) window cannot stretch the rhythm.

import { applySink, speakerId } from './engine'

let ctx: AudioContext | null = null
let sink = ''

function context(): AudioContext {
  if (!ctx) ctx = new AudioContext({ latencyHint: 'interactive' })
  if (sink !== speakerId()) {
    sink = speakerId()
    void applySink(ctx, sink)
  }
  if (ctx.state === 'suspended') void ctx.resume()
  return ctx
}

/** One soft note: sine with a short attack and an exponential release (no clicks). */
function note(out: AudioNode, freq: number, at: number, dur: number, peak: number, harmonic = 0): void {
  const c = out.context
  const env = c.createGain()
  env.gain.setValueAtTime(0, at)
  env.gain.linearRampToValueAtTime(peak, at + 0.012)
  env.gain.setValueAtTime(peak, at + Math.max(0.012, dur - 0.06))
  env.gain.exponentialRampToValueAtTime(0.0001, at + dur)
  env.connect(out)
  for (const [f, g] of harmonic ? [[freq, 1], [freq * 2, harmonic]] : [[freq, 1]]) {
    const osc = c.createOscillator()
    osc.type = 'sine'
    osc.frequency.value = f
    const level = c.createGain()
    level.gain.value = g
    osc.connect(level).connect(env)
    osc.start(at)
    osc.stop(at + dur + 0.02)
  }
}

interface Loop {
  out: GainNode
  timer: ReturnType<typeof setInterval>
}

let loop: { kind: string; l: Loop } | null = null

/**
 * Repeats `pattern(out, startTime)` every `period` s, scheduled up to 30 s ahead and topped up
 * every few seconds.
 */
function startLoop(kind: string, period: number, pattern: (out: AudioNode, at: number) => void): void {
  if (loop?.kind === kind) return
  stopTones()
  const c = context()
  const out = c.createGain()
  out.gain.value = 1
  out.connect(c.destination)
  let next = c.currentTime + 0.05
  const fill = () => {
    while (next < c.currentTime + 30) {
      pattern(out, next)
      next += period
    }
  }
  fill()
  loop = { kind, l: { out, timer: setInterval(fill, 5000) } }
}

export function stopTones(): void {
  if (!loop) return
  clearInterval(loop.l.timer)
  const { out } = loop.l
  // Fade instead of a hard cut.
  const t = out.context.currentTime
  out.gain.setValueAtTime(out.gain.value, t)
  out.gain.linearRampToValueAtTime(0, t + 0.05)
  setTimeout(() => out.disconnect(), 120)
  loop = null
}

/** Incoming call: a rising three-note chime, twice, every 3 s. */
export function playRingtone(): void {
  startLoop('ring', 3, (out, at) => {
    const notes = [659.25, 830.61, 987.77]
    for (let rep = 0; rep < 2; rep++) {
      notes.forEach((f, i) => note(out, f, at + rep * 0.62 + i * 0.16, 0.34, 0.16, 0.18))
    }
  })
}

/** Outgoing 1:1, the other side is notified: 425 Hz, 1 s on, 4 s off. */
export function playRingback(): void {
  startLoop('ringback', 5, (out, at) => note(out, 425, at, 1, 0.12))
}

function once(freqs: number[], step: number, dur: number): void {
  stopTones()
  const c = context()
  const t = c.currentTime + 0.03
  freqs.forEach((f, i) => note(c.destination, f, t + i * step, dur, 0.14, 0.1))
}

/** The call is up. */
export function playConnected(): void {
  once([523.25, 783.99], 0.1, 0.16)
}

/** The call ended. */
export function playHangup(): void {
  once([659.25, 440, 329.63], 0.13, 0.18)
}
