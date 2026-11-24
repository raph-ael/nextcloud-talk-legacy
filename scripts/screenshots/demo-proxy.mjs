// Gives demo servers invented host names without touching the system: a plain HTTP proxy that
// sends each name to a local port. The app reaches it through `http_proxy` (the core reads it),
// so the names never have to resolve:
//   node demo-proxy.mjs 8097 cloud.familie-lange.example=8099 cloud.werkhof.example=8098
//   http_proxy=http://127.0.0.1:8097 TALK_DEV_SERVER=http://cloud.familie-lange.example …
// The account menu then shows the name instead of 127.0.0.1:8099 (scripts/screenshots/desktop.sh).
import http from 'node:http'

const [listen, ...pairs] = process.argv.slice(2)
const ports = new Map(pairs.map((p) => p.split('=')))
if (!listen || !ports.size) {
  console.error('usage: demo-proxy.mjs <port> <name>=<port> …')
  process.exit(2)
}

http
  .createServer((req, res) => {
    // A proxy request carries the whole address in its first line.
    const url = URL.parse(req.url ?? '')
    const port = url && ports.get(url.hostname)
    if (!port) {
      res.writeHead(502).end()
      return
    }
    const upstream = http.request(
      { host: '127.0.0.1', port, method: req.method, path: url.pathname + url.search, headers: req.headers },
      (answer) => {
        res.writeHead(answer.statusCode ?? 502, answer.headers)
        answer.pipe(res)
      },
    )
    upstream.on('error', () => res.writeHead(502).end())
    req.pipe(upstream)
  })
  .listen(Number(listen), '127.0.0.1')
