// Linux-only steps for scripts/screenshots/desktop.sh over the Chrome DevTools protocol (renderer
// on $CDP_PAGE_PORT, main process inspector on $CDP_MAIN_PORT). Theme, opening a chat and waiting
// come from mac-drive.mjs, which works the same here.
//   node desktop-drive.mjs size W H     window content size in px
//   node desktop-drive.mjs shot out.png screenshot of the page (= the window content)
//   node desktop-drive.mjs pointer X Y  move the pointer there (off the messages: no hover bar)
import { writeFileSync } from 'node:fs'

const PAGE = Number(process.env.CDP_PAGE_PORT ?? 9342)
const MAIN = Number(process.env.CDP_MAIN_PORT ?? 9343)

async function call(port, method, params) {
  const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
  const target = port === MAIN ? list[0] : list.find((t) => t.type === 'page' && !t.url.startsWith('data:'))
  if (!target) throw new Error(`no target on ${port}`)
  const ws = new WebSocket(target.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve)
    ws.addEventListener('error', reject)
  })
  const reply = await new Promise((resolve) => {
    ws.addEventListener('message', (e) => {
      const m = JSON.parse(e.data)
      if (m.id === 1) resolve(m)
    })
    ws.send(JSON.stringify({ id: 1, method, params }))
  })
  ws.close()
  if (reply.error) throw new Error(reply.error.message)
  if (reply.result?.exceptionDetails) throw new Error(reply.result.exceptionDetails.exception?.description)
  return reply.result
}

const [cmd, ...args] = process.argv.slice(2)
switch (cmd) {
  case 'size': {
    const [w, h] = args.map(Number)
    const r = await call(MAIN, 'Runtime.evaluate', {
      expression: `(() => { const { BrowserWindow } = process.mainModule.require('electron');
        const win = BrowserWindow.getAllWindows()[0]; win.setContentSize(${w}, ${h}); win.focus();
        return win.getContentSize().join('x') })()`,
      returnByValue: true,
    })
    if (r.result.value !== `${w}x${h}`) throw new Error(`window is ${r.result.value}`)
    break
  }
  case 'shot': {
    const r = await call(PAGE, 'Page.captureScreenshot', { format: 'png' })
    writeFileSync(args[0], Buffer.from(r.data, 'base64'))
    break
  }
  case 'pointer': {
    const [x, y] = args.map(Number)
    await call(PAGE, 'Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
    break
  }
  default:
    console.error('usage: desktop-drive.mjs size W H | shot out.png | pointer X Y')
    process.exit(2)
}
