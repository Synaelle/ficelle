// SPDX-License-Identifier: AGPL-3.0-or-later
// A server to try everything by hand, in one command: `pnpm dev:server`.
// Builds the web pages and the server, then starts, next to it, a pretend
// single sign-on provider on port 9100, with one person.
// Data stays in .dev-data/server between runs, `--fresh` starts over.
// `--lan` opens it to the home network, for a phone on the same Wi-Fi.
// Ctrl+C stops everything. For trying things, never for real use.

import { execSync, spawn } from 'node:child_process'
import { createHash, createSign, generateKeyPairSync, randomBytes } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { networkInterfaces } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const brand = JSON.parse(readFileSync(join(root, 'packages/brand/brand.json'), 'utf8'))
const data = join(root, '.dev-data', 'server')
const PORT = 8080
const SSO_PORT = 9100

/** This computer's address on the home network, for `--lan`. */
function lanAddress() {
  const all = Object.values(networkInterfaces())
    .flat()
    .filter((a) => a && a.family === 'IPv4' && !a.internal)
    .map((a) => a?.address ?? '')
  return all.find((a) => a.startsWith('192.168.')) ?? all[0]
}
const lan = process.argv.includes('--lan') ? lanAddress() : undefined
if (process.argv.includes('--lan') && !lan) {
  console.error('no home network address found, is this computer on Wi-Fi or a cable?')
  process.exit(1)
}
const site = `http://${lan ?? 'localhost'}:${PORT}`
const bold = (/** @type {string} */ s) => `\x1b[1m${s}\x1b[0m`
const dim = (/** @type {string} */ s) => `\x1b[2m${s}\x1b[0m`

if (process.argv.includes('--fresh')) rmSync(data, { recursive: true, force: true })
mkdirSync(data, { recursive: true })

/** @param {string} what @param {string} command */
function build(what, command) {
  console.log(dim(`building ${what}…`))
  execSync(command, { cwd: root, stdio: 'inherit' })
}
build('the admin page', 'pnpm --filter @app/client build:admin')
build('the server', 'cargo build -q -p server')

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
// the saved config stays local, the home network one is given for this run
const overrides = lan
  ? {
      [`${brand.envPrefix}_SERVER__BIND`]: `0.0.0.0:${PORT}`,
      [`${brand.envPrefix}_SERVER__PUBLIC_URL`]: site,
    }
  : {}
const server = spawn(binary, ['--config', config], {
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, ...overrides },
})
/** @param {Buffer} chunk */
function relay(chunk) {
  const text = chunk.toString()
  process.stdout.write(dim(text))
  const link = /(https?:\/\/\S+\/setup#token=\S+)/.exec(text)?.[1]
  if (link) console.log(`\n${bold('First admin:')} open ${bold(link)} (once, within the hour)\n`)
}
server.stdout.on('data', relay)
server.stderr.on('data', relay)
server.on('exit', (code) => {
  console.log(`server stopped (${code})`)
  process.exit(code ?? 0)
})

console.log(`
${bold(`${brand.displayName} test server`)}  ${site}/admin
  single sign-on: the "Test sign-on" button, signs in as Zoé
  privacy page: ${site}/privacy
  data: ${data}  (pnpm dev:server --fresh to start over)
  ${lan ? `phone: in the app, sign in to ${site} (same Wi-Fi), single sign-on stays on this computer` : 'for a phone on the same Wi-Fi: pnpm dev:server --lan'}
  Ctrl+C stops everything
`)
process.on('SIGINT', () => server.kill())
