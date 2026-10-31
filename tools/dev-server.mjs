// SPDX-License-Identifier: AGPL-3.0-or-later
// A server to try everything by hand, in one command: `pnpm dev:server`.
// Builds the web pages and the server, then starts, next to it:
//   - a mail catcher on port 2525, which prints every mail (and its links)
//     here instead of sending it,
//   - a pretend single sign-on provider on port 9100, with one person.
// Data stays in .dev-data/server between runs, `--fresh` starts over.
// Ctrl+C stops everything. For trying things, never for real use.

import { execSync, spawn } from 'node:child_process'
import { createHash, createSign, generateKeyPairSync, randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { createServer as createTcpServer } from 'node:net'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const brand = JSON.parse(readFileSync(join(root, 'packages/brand/brand.json'), 'utf8'))
const data = join(root, '.dev-data', 'server')
const PORT = 8080
const MAIL_PORT = 2525
const SSO_PORT = 9100
const site = `http://localhost:${PORT}`
const bold = (/** @type {string} */ s) => `\x1b[1m${s}\x1b[0m`
const dim = (/** @type {string} */ s) => `\x1b[2m${s}\x1b[0m`

if (process.argv.includes('--fresh')) rmSync(data, { recursive: true, force: true })
mkdirSync(data, { recursive: true })

/** @param {string} what @param {string} command */
function build(what, command) {
  console.log(dim(`building ${what}…`))
  execSync(command, { cwd: root, stdio: 'inherit' })
}
build('the web pages', 'pnpm --filter @app/client build')
build('the server', 'cargo build -q -p server')

// mail catcher: just enough SMTP for the server to hand a message over
createTcpServer((socket) => {
  socket.setEncoding('utf8')
  let inData = false
  /** @type {string[]} */
  let lines = []
  let buffer = ''
  /** @param {string} line */
  const say = (line) => socket.write(line + '\r\n')
  say('220 dev mail catcher')
  socket.on('data', (chunk) => {
    buffer += chunk
    let at
    while ((at = buffer.indexOf('\r\n')) >= 0) {
      const line = buffer.slice(0, at)
      buffer = buffer.slice(at + 2)
      if (inData) {
        if (line === '.') {
          inData = false
          showMail(lines.join('\n'))
          lines = []
          say('250 kept')
        } else lines.push(line.startsWith('..') ? line.slice(1) : line)
        continue
      }
      const verb = line.slice(0, 4).toUpperCase()
      if (verb === 'DATA') {
        inData = true
        say('354 go on')
      } else if (verb === 'QUIT') {
        say('221 bye')
        socket.end()
      } else say('250 ok')
    }
  })
}).listen(MAIL_PORT, '127.0.0.1')

/** Prints a caught mail, decoding the quoted-printable body. @param {string} raw */
function showMail(raw) {
  const [head, ...rest] = raw.split('\n\n')
  const header = (/** @type {string} */ name) =>
    new RegExp(`^${name}: (.*)$`, 'mi').exec(head)?.[1] ?? ''
  const subject = header('Subject').replace(/=\?utf-8\?b\?([^?]+)\?=/gi, (_, b64) =>
    Buffer.from(b64, 'base64').toString('utf8'),
  )
  const body = Buffer.from(
    rest
      .join('\n\n')
      .replace(/=\n/g, '')
      .replace(/=([0-9A-F]{2})/g, (_, hex) => String.fromCharCode(parseInt(hex, 16))),
    'latin1',
  ).toString('utf8')
  console.log(`\n${bold('📧 mail to ' + header('To'))}  ${subject}\n${body.trim()}\n`)
}

// pretend sign-on provider: discovery, keys, a sign-in page, signed tokens
const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 })
const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'dev', use: 'sig', alg: 'RS256' }
const issuer = `http://localhost:${SSO_PORT}`
const codes = new Map()
const b64 = (/** @type {Buffer | string} */ v) => Buffer.from(v).toString('base64url')
createServer((req, res) => {
  const url = new URL(req.url ?? '/', issuer)
  const json = (/** @type {unknown} */ value, status = 200) => {
    res.writeHead(status, { 'content-type': 'application/json' })
    res.end(JSON.stringify(value))
  }
  if (url.pathname === '/.well-known/openid-configuration')
    return json({
      issuer,
      authorization_endpoint: `${issuer}/authorize`,
      token_endpoint: `${issuer}/token`,
      jwks_uri: `${issuer}/jwks`,
      response_types_supported: ['code'],
      subject_types_supported: ['public'],
      id_token_signing_alg_values_supported: ['RS256'],
    })
  if (url.pathname === '/jwks') return json({ keys: [jwk] })
  if (url.pathname === '/authorize') {
    const q = url.searchParams
    const code = b64(randomBytes(16))
    codes.set(code, { nonce: q.get('nonce'), challenge: q.get('code_challenge') })
    const back = `${q.get('redirect_uri')}?code=${code}&state=${encodeURIComponent(q.get('state') ?? '')}`
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' })
    return res.end(
      `<!doctype html><meta charset="utf-8"><title>Test sign-on</title>` +
        `<body style="font:16px system-ui;max-width:420px;margin:60px auto">` +
        `<h1>Test sign-on</h1><p>This stands in for a real provider (Google, Authentik…).</p>` +
        `<p><a href="${back}">Continue as Zoé Martin (zoe@example.org)</a></p></body>`,
    )
  }
  if (url.pathname === '/token' && req.method === 'POST') {
    let raw = ''
    req.on('data', (c) => (raw += c))
    req.on('end', () => {
      const form = new URLSearchParams(raw)
      const login = codes.get(form.get('code') ?? '')
      codes.delete(form.get('code') ?? '')
      const proof = b64(
        createHash('sha256')
          .update(form.get('code_verifier') ?? '')
          .digest(),
      )
      if (!login || proof !== login.challenge) return json({ error: 'invalid_grant' }, 400)
      const now = Math.floor(Date.now() / 1000)
      const head = b64(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'dev' }))
      const claims = b64(
        JSON.stringify({
          iss: issuer,
          aud: 'dev',
          sub: 'zoe-0001',
          iat: now,
          exp: now + 300,
          nonce: login.nonce,
          preferred_username: 'zoe',
          email: 'zoe@example.org',
          name: 'Zoé Martin',
        }),
      )
      const sign = createSign('RSA-SHA256')
      sign.update(`${head}.${claims}`)
      json({
        access_token: b64(randomBytes(16)),
        token_type: 'Bearer',
        expires_in: 300,
        id_token: `${head}.${claims}.${b64(sign.sign(privateKey))}`,
      })
    })
    return
  }
  res.writeHead(404).end()
}).listen(SSO_PORT, '127.0.0.1')

const config = join(data, brand.configFile)
if (!existsSync(config))
  writeFileSync(
    config,
    `# written by tools/dev-server.mjs, edit freely, --fresh rewrites it
[server]
bind = "127.0.0.1:${PORT}"
public_url = "${site}"
web_dir = "${join(root, 'apps/client/dist').replace(/\\/g, '/')}"

[storage]
data_dir = "${data.replace(/\\/g, '/')}"

[auth]
# cheap hashing, it is only for trying
argon2_memory_kib = 1024
argon2_iterations = 1

[smtp]
host = "127.0.0.1"
port = ${MAIL_PORT}
security = "none"
from = "${brand.displayName} <notes@example.org>"

[sso]
issuer = "${issuer}"
client_id = "dev"
label = "Test sign-on"
create_accounts = true

[privacy]
operator = "You, trying things out"
contact = "you@example.org"
`,
  )

const binary = join(root, 'target', 'debug', brand.serverBinary)
const server = spawn(binary, ['--config', config], { stdio: ['ignore', 'pipe', 'pipe'] })
/** @param {Buffer} chunk */
function relay(chunk) {
  const text = chunk.toString()
  process.stdout.write(dim(text))
  const code = /setup code: ([A-Z0-9-]+)/.exec(text)?.[1]
  if (code)
    console.log(
      `\n${bold('First admin:')} open ${site}/admin and enter the setup code ${bold(code)}\n`,
    )
}
server.stdout.on('data', relay)
server.stderr.on('data', relay)
server.on('exit', (code) => {
  console.log(`server stopped (${code})`)
  process.exit(code ?? 0)
})

console.log(`
${bold(`${brand.displayName} test server`)}  ${site}/admin
  mails are printed here, links included
  single sign-on: the "Test sign-on" button, signs in as Zoé
  privacy page: ${site}/privacy
  data: ${data}  (pnpm dev:server --fresh to start over)
  Ctrl+C stops everything
`)
process.on('SIGINT', () => server.kill())
