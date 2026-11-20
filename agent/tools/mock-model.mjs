// Mini-Modellserver (OpenAI-kompatibel) für Plugin-Tests: antwortet deterministisch,
// gestreamt und bei Bedarf mit einem Werkzeugaufruf. Kein echtes Modell, keine Kosten.
import http from 'node:http';
const PORT = Number(process.env.MOCK_PORT ?? 19112);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const textOf = (c) => (typeof c === 'string' ? c : Array.isArray(c) ? c.map((p) => p.text ?? '').join(' ') : '');
http.createServer(async (req, res) => {
  if (req.method === 'GET' && req.url.startsWith('/v1/models')) {
    res.writeHead(200, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ object: 'list', data: [{ id: 'mock-1', object: 'model' }] }));
  }
  if (req.method !== 'POST') { res.writeHead(404); return res.end(); }
  let raw = ''; for await (const c of req) raw += c;
  const body = JSON.parse(raw || '{}');
  const msgs = body.messages ?? [];
  const last = msgs[msgs.length - 1] ?? {};
  // Die eigentliche Nutzernachricht steht in einer der letzten user-Nachrichten (dazwischen liegt Laufzeitkontext).
  const lastToolIdx = msgs.map((m) => m.role).lastIndexOf('tool');
  const lastAsstIdx = msgs.map((m) => m.role).lastIndexOf('assistant');
  const userText = msgs.slice(Math.max(lastAsstIdx, 0)).filter((m) => m.role === 'user').map((m) => textOf(m.content)).join('\n');
  if (process.env.MOCK_DEBUG) (await import('node:fs')).writeFileSync(process.env.MOCK_DEBUG_FILE ?? '/tmp/mock-last-user.txt', userText);
  if (process.env.MOCK_DEBUG) console.log('tools:', (body.tools ?? []).map((t) => t.function?.name ?? t.name).join(','));
  const tools = (body.tools ?? []).map((t) => t.function?.name ?? t.name);
  const wantsTool = /Werkzeug \(tool\)/.test(userText) && last.role !== 'tool' && tools.includes('exec');
  const id = 'chatcmpl-' + Date.now();
  const chunk = (delta, finish = null) => `data: ${JSON.stringify({ id, object: 'chat.completion.chunk', created: Math.floor(Date.now() / 1000), model: 'mock-1', choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
  console.log(new Date().toISOString(), 'request', { stream: body.stream, msgs: msgs.length, lastRole: last.role, wantsTool, user: userText.slice(-80) });
  let answer;
  if (/antworte kurz/i.test(userText)) answer = 'Kurze Antwort.';
  else if (last.role === 'tool') answer = 'Das Werkzeug ist durchgelaufen. Ergebnis: ' + textOf(last.content).slice(0, 80) + '\n\n**Fertig.** Hier ist noch eine kleine Tabelle:\n\n| Schritt | Status |\n|---|---|\n| Befehl | ok |';
  else answer = 'Das ist eine längere Testantwort des Agenten, die Wort für Wort entsteht. ' + 'Sie soll zeigen, wie die Vorschau in Talk mitläuft, während das Modell noch schreibt. '.repeat(3) + 'Ende der Antwort.';
  if (!body.stream) {
    res.writeHead(200, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ id, object: 'chat.completion', model: 'mock-1', choices: [{ index: 0, message: { role: 'assistant', content: answer }, finish_reason: 'stop' }], usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 } }));
  }
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
  res.write(chunk({ role: 'assistant', content: '' }));
  if (wantsTool) {
    for (const w of 'Ich prüfe das kurz mit einem Befehl. '.split(/(?<= )/)) { res.write(chunk({ content: w })); await sleep(40); }
    res.write(chunk({ tool_calls: [{ index: 0, id: 'call_' + Date.now(), type: 'function', function: { name: 'exec', arguments: JSON.stringify({ command: 'sleep 5; echo werkzeug-ok' }) } }] }));
    res.write(chunk({}, 'tool_calls'));
  } else {
    for (const w of answer.split(/(?<= )/)) { res.write(chunk({ content: w })); await sleep(Number(process.env.MOCK_WORD_MS ?? 70)); }
    res.write(chunk({}, 'stop'));
  }
  res.write(`data: ${JSON.stringify({ id, object: 'chat.completion.chunk', model: 'mock-1', choices: [], usage: { prompt_tokens: 10, completion_tokens: 10, total_tokens: 20 } })}\n\n`);
  res.write('data: [DONE]\n\n'); res.end();
}).listen(PORT, '127.0.0.1', () => console.log('mock model on', PORT));
