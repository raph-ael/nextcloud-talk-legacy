// Drives the desktop app on the Mac for scripts/screenshots/mac.sh over the Chrome DevTools
// protocol (renderer on $CDP_PAGE_PORT, main process inspector on $CDP_MAIN_PORT, both tunnelled):
//   node mac-drive.mjs prepare X Y W H   window bounds in points, backdrop behind it, no shadow
//   node mac-drive.mjs theme light|dark
//   node mac-drive.mjs backdrop '#000000'
//   node mac-drive.mjs open 'Chat name'   opens the chat from the list (exact name)
//   node mac-drive.mjs ready              waits until avatars and images have loaded
//   node mac-drive.mjs bounds             window bounds as JSON
//   node mac-drive.mjs finish             removes the backdrop
const PAGE = Number(process.env.CDP_PAGE_PORT ?? 9342)
const MAIN = Number(process.env.CDP_MAIN_PORT ?? 9343)

async function evaluate(port, expression) {
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
    ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }))
  })
  ws.close()
  const { result, exceptionDetails } = reply.result
  if (exceptionDetails) throw new Error(exceptionDetails.exception?.description ?? exceptionDetails.text)
  return result.value
}

// Main process: the app window is the one that is not our backdrop.
const main = (body) =>
  evaluate(
    MAIN,
    `(async () => { const { app, BrowserWindow, nativeTheme } = process.mainModule.require('electron');
      const win = BrowserWindow.getAllWindows().find((w) => w !== globalThis.__backdrop); ${body} })()`,
  )
const page = (body) => evaluate(PAGE, `(async () => { ${body} })()`)
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const [cmd, ...args] = process.argv.slice(2)
switch (cmd) {
  case 'prepare': {
    const [x, y, width, height] = args.map(Number)
    await main(`
      if (!globalThis.__backdrop) {
        const b = new BrowserWindow({ x: 0, y: 0, width: 4000, height: 4000, frame: false, hasShadow: false,
          focusable: false, enableLargerThanScreen: true, backgroundColor: '#000000', show: false });
        await b.loadURL('data:text/html,<body style="margin:0;background:%23000">');
        b.setAlwaysOnTop(true, 'floating');
        b.showInactive();
        globalThis.__backdrop = b;
      }
      win.setAlwaysOnTop(true, 'pop-up-menu');
      win.setHasShadow(false);
      win.setBounds({ x: ${x}, y: ${y}, width: ${width}, height: ${height} });
      app.focus({ steal: true });
      win.focus();`)
    break
  }
  case 'theme':
    await main(`nativeTheme.themeSource = ${JSON.stringify(args[0])}; win.focus();`)
    break
  case 'backdrop':
    await main(`
      const c = ${JSON.stringify(args[0])};
      globalThis.__backdrop.setBackgroundColor(c);
      await globalThis.__backdrop.webContents.executeJavaScript('document.body.style.background = ' + JSON.stringify(c));
      win.focus();`)
    break
  case 'open': {
    const opened = await page(`
      const name = ${JSON.stringify(args[0])};
      // The smallest element whose first line is the chat name: the row in the list.
      const row = [...document.querySelectorAll('aside *, nav *, [role=listitem], li, button')]
        .filter((e) => e.innerText?.split('\\n')[0].trim() === name && e.innerText.length < 300)
        .sort((a, b) => a.innerText.length - b.innerText.length)
        .map((e) => e.closest('button, [role=button], li, a') ?? e)[0];
      if (!row) return false;
      row.click();
      return true;`)
    if (!opened) throw new Error(`chat ${args[0]} not found`)
    await sleep(1500)
    // No blinking caret in the picture.
    await page(`document.activeElement?.blur(); return true`)
    break
  }
  case 'ready': {
    for (let i = 0; i < 60; i++) {
      const ok = await page(`
        const imgs = [...document.images];
        return document.body.innerText.length > 100 && imgs.length > 0 && imgs.every((i) => i.complete && i.naturalWidth > 0)
          && !document.querySelector('[class*=spinner], [class*=loading], [aria-busy=true]');`)
      if (ok) break
      if (i === 59) throw new Error('page did not settle')
      await sleep(500)
    }
    await sleep(800)
    break
  }
  case 'bounds':
    console.log(JSON.stringify(await main(`return win.getBounds();`)))
    break
  case 'finish':
    await main(`globalThis.__backdrop?.destroy(); globalThis.__backdrop = null;
      win.setAlwaysOnTop(false); win.setHasShadow(true); nativeTheme.themeSource = 'system';`)
    break
  default:
    console.error('usage: mac-drive.mjs prepare|theme|backdrop|open|ready|bounds|finish')
    process.exit(2)
}
