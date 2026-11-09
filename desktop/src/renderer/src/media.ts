// Attachments: turns picked, dropped or pasted files into staged files for the outbox.

import type { StagedFile } from '@shared/core'

/** A file waiting in the composer to be sent. */
export interface Draft {
  key: string
  file: StagedFile
  /** Object URL of the (prepared) image for the thumbnail rail; null for other files. */
  thumb: string | null
}

const MAX_EDGE = 2048
const JPEG_QUALITY = 0.85
/** Decoded and re-encoded like on Android; GIF (animation) and SVG go as they are. */
const PREPARED = /^image\/(jpeg|png|webp|bmp|avif)$/

function stamp(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}_${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`
}

/**
 * Downscales to at most 2048 px on the long side, applies the EXIF rotation and
 * re-encodes as JPEG. Re-encoding drops all metadata (GPS position, camera …), like Signal.
 */
async function prepareImage(file: File): Promise<{ blob: Blob; width: number; height: number }> {
  const bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' })
  try {
    const scale = Math.min(1, MAX_EDGE / Math.max(bitmap.width, bitmap.height))
    const width = Math.max(1, Math.round(bitmap.width * scale))
    const height = Math.max(1, Math.round(bitmap.height * scale))
    const canvas = new OffscreenCanvas(width, height)
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('no canvas')
    // JPEG has no transparency: white instead of black behind transparent PNGs.
    ctx.fillStyle = '#fff'
    ctx.fillRect(0, 0, width, height)
    ctx.drawImage(bitmap, 0, 0, width, height)
    const blob = await canvas.convertToBlob({ type: 'image/jpeg', quality: JPEG_QUALITY })
    return { blob, width, height }
  } finally {
    bitmap.close()
  }
}

let counter = 0

/** Stages one file; images are prepared first. */
export async function stage(file: File): Promise<Draft> {
  const key = `d${++counter}`
  if (PREPARED.test(file.type)) {
    const { blob, width, height } = await prepareImage(file)
    const base = file.name && file.name !== 'image.png' ? file.name.replace(/\.[^.]+$/, '') : `IMG_${stamp()}`
    const staged = await window.talk.stageData(
      `${base}.jpg`,
      'image/jpeg',
      new Uint8Array(await blob.arrayBuffer()),
      width,
      height,
    )
    return { key, file: staged, thumb: URL.createObjectURL(blob) }
  }
  const path = window.talk.pathForFile(file)
  const staged = path
    ? await window.talk.stagePath(path, file.name, file.type)
    : await window.talk.stageData(file.name || `Datei_${stamp()}`, file.type, new Uint8Array(await file.arrayBuffer()), 0, 0)
  const thumb = file.type.startsWith('image/') ? URL.createObjectURL(file) : null
  return { key, file: staged, thumb }
}

/** Removes a draft that will not be sent. */
export function dropDraft(d: Draft): void {
  if (d.thumb) URL.revokeObjectURL(d.thumb)
  void window.talk.unstage(d.file.path)
}

/** Releases the thumbnail of a draft that was handed to the outbox. */
export function releaseDraft(d: Draft): void {
  if (d.thumb) URL.revokeObjectURL(d.thumb)
}

export function isImage(mime: string): boolean {
  return mime.startsWith('image/')
}
