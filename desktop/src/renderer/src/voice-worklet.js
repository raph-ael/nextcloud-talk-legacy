// AudioWorklet: turns the microphone signal into 16-bit little-endian mono PCM blocks of
// 100 ms and posts them to the page, which hands them to the core (MP3 on disk).

const BLOCK_SECONDS = 0.1

class PcmCapture extends AudioWorkletProcessor {
  constructor() {
    super()
    this.size = Math.round(sampleRate * BLOCK_SECONDS)
    this.block = new Int16Array(this.size)
    this.fill = 0
    this.port.onmessage = (e) => {
      if (e.data === 'flush') {
        this.post()
        this.port.postMessage('flushed')
      }
    }
  }

  post() {
    if (this.fill === 0) return
    const out = this.block.slice(0, this.fill)
    this.port.postMessage(out.buffer, [out.buffer])
    this.fill = 0
  }

  process(inputs) {
    const channel = inputs[0] && inputs[0][0]
    if (!channel) return true
    for (let i = 0; i < channel.length; i++) {
      const s = Math.max(-1, Math.min(1, channel[i]))
      this.block[this.fill++] = s < 0 ? s * 0x8000 : s * 0x7fff
      if (this.fill === this.size) this.post()
    }
    return true
  }
}

registerProcessor('pcm-capture', PcmCapture)
