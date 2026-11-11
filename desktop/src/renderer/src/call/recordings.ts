// Finished recordings waiting for the user (docs/calls.md, „Aufnahme“): the core keeps the
// list, `recordingsChanged` always brings all of it.

import { useSyncExternalStore } from 'react'
import type { RecordingNotice } from '@shared/core'

let notices: RecordingNotice[] = []
/** Notices with an action on the way (buttons disabled, no double share). */
let busy = new Set<number>()
const listeners = new Set<() => void>()
let started = false

function emit(): void {
  listeners.forEach((l) => l())
}

function subscribe(l: () => void): () => void {
  listeners.add(l)
  return () => listeners.delete(l)
}

function setNotices(items: RecordingNotice[]): void {
  notices = [...items].sort((a, b) => b.timestamp - a.timestamp)
  emit()
}

export function startRecordingNotices(): void {
  if (started) return
  started = true
  window.talk.onEvent((e) => {
    if (e.type === 'recordingsChanged') setNotices(e.items)
    else if (e.type === 'session' && e.state === 'loggedOut') setNotices([])
    else if (e.type === 'session' && e.state === 'ready') void load()
  })
  void load()
}

async function load(): Promise<void> {
  const items = await window.talk.recordingNotices().catch(() => null)
  if (items) setNotices(items)
}

/** All notices, or those of one conversation. */
export function useRecordingNotices(token?: string): RecordingNotice[] {
  const all = useSyncExternalStore(subscribe, () => notices)
  return token === undefined ? all : all.filter((n) => n.token === token)
}

export function useNoticeBusy(id: number): boolean {
  return useSyncExternalStore(subscribe, () => busy.has(id))
}

/** Runs a share/dismiss once; resolves to an error message (or null). */
export async function noticeAction(id: number, share: boolean): Promise<string | null> {
  if (busy.has(id)) return null
  busy = new Set(busy).add(id)
  emit()
  try {
    await (share ? window.talk.shareRecording(id) : window.talk.dismissRecording(id))
    return null
  } catch (e) {
    return String((e as Error)?.message ?? e)
  } finally {
    busy = new Set(busy)
    busy.delete(id)
    emit()
  }
}

export function isFailure(n: RecordingNotice): boolean {
  return n.kind.endsWith('Failed')
}

