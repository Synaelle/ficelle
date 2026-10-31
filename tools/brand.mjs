// SPDX-License-Identifier: AGPL-3.0-or-later
// brand.json is the single source for every name, identifier and url that
// would change on a rename. Typescript imports brand.json directly. This
// script copies the values into the places that cannot read it (rust consts,
// tauri and cargo configs, the Dockerfile, the compose files). Run with
// --check in ci to catch drift.
//
// Renaming is meant to be easy only before the first public release. After
// it, run with --freeze once. That writes brand.lock.json and from then on any
// change to a frozen value fails, because installed apps depend on them
// (updates, keychain entries, data folders, env vars, server configs).

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const brand = JSON.parse(readFileSync(join(root, 'packages/brand/brand.json'), 'utf8'))
const check = process.argv.includes('--check')
const lockPath = join(root, 'brand.lock.json')

// the feedback, catalog and release list addresses can move between hosts,
// their keys can be replaced, and the dev identity never ships: changing them
// breaks no installed app
const unfrozen = new Set([
  'feedbackUrl',
  'catalogUrl',
  'fontsUrl',
  'catalogKey',
  'releasesUrl',
  'releasesKey',
  'appUpdatesUrl',
  'appUpdatesKey',
  'dev',
])
const frozenValues = Object.fromEntries(Object.entries(brand).filter(([key]) => !unfrozen.has(key)))

if (process.argv.includes('--freeze')) {
  if (existsSync(lockPath)) {
    console.error('brand.lock.json already exists, the brand is frozen')
    process.exit(1)
  }
  writeFileSync(lockPath, JSON.stringify(frozenValues, null, 2) + '\n')
  console.log('brand frozen in brand.lock.json, commit it with the release')
  process.exit(0)
}

if (existsSync(lockPath)) {
  const lock = JSON.parse(readFileSync(lockPath, 'utf8'))
  const changed = Object.keys(lock).filter((key) => lock[key] !== frozenValues[key])
  if (changed.length > 0) {
    console.error(`frozen brand values changed: ${changed.join(', ')}`)
    console.error('the app is public, these values are tied to installed apps and servers.')
    console.error('a rename now needs a migration plan.')
    process.exit(1)
  }
}

const header =
  'Written by tools/brand.mjs from packages/brand/brand.json. Edit that file, then run pnpm brand.'

/**
 * Identity consts, emitted once per build kind.
 * @param {{ displayName: string, slug: string, identifier: string }} values
 * @param {string} cfg
 * @param {string} kind
 */
function identity(values, cfg, kind) {
  return `#[cfg(${cfg})]
mod identity {
    //! ${kind} identity.

    /// Name shown to users.
    pub const DISPLAY_NAME: &str = ${JSON.stringify(values.displayName)};
    /// Lowercase name used for folders, files and binaries.
    pub const SLUG: &str = ${JSON.stringify(values.slug)};
    /// Reverse domain app identifier (bundle id, package name).
    pub const IDENTIFIER: &str = ${JSON.stringify(values.identifier)};
}`
}

const rust = `// SPDX-License-Identifier: AGPL-3.0-or-later
// ${header}

//! Product names and identifiers shared by every rust crate.
//!
//! Release and preview builds (\`APP_RELEASE=1\`, \`APP_PREVIEW=1\`) use the
//! real identity. Every other build uses the dev identity so it installs next
//! to the real app with its own data.

${identity(brand, 'app_release', 'Release')}

${identity(brand.dev, 'not(app_release)', 'Dev')}

pub use identity::{DISPLAY_NAME, IDENTIFIER, SLUG};

/// Prefix of every environment variable, without the trailing underscore.
pub const ENV_PREFIX: &str = ${JSON.stringify(brand.envPrefix)};
/// Server executable name.
pub const SERVER_BINARY: &str = ${JSON.stringify(brand.serverBinary)};
/// Server configuration file name.
pub const CONFIG_FILE: &str = ${JSON.stringify(brand.configFile)};
/// Extension of backup archives, the same for every build so any of them
/// restores the others' backups.
pub const BACKUP_EXTENSION: &str = ${JSON.stringify(brand.backupExtension)};
/// Default instance that receives feedback reports.
pub const FEEDBACK_URL: &str = ${JSON.stringify(brand.feedbackUrl)};
/// The official catalog of themes: the folder holding
/// \`catalog.json\`, ending with a slash.
pub const CATALOG_URL: &str = ${JSON.stringify(brand.catalogUrl)};
/// Where the apps download fonts first: the project's own host, which
/// mirrors them (tools/fonts-mirror.mjs). jsDelivr only when it fails.
pub const FONTS_URL: &str = ${JSON.stringify(brand.fontsUrl)};
/// The hex Ed25519 public key the catalog's entries are signed with. Empty:
/// no catalog in this build.
pub const CATALOG_KEY: &str = ${JSON.stringify(brand.catalogKey)};
/// The signed list of releases servers check for update notices.
pub const RELEASES_URL: &str = ${JSON.stringify(brand.releasesUrl)};
/// The hex Ed25519 public key its entries are signed with (the catalog's
/// may serve, the messages differ). Empty: no update check in this build.
pub const RELEASES_KEY: &str = ${JSON.stringify(brand.releasesKey)};
/// Where release apps look for a newer version of themselves: tauri's
/// \`latest.json\`, attached to the newest GitHub release.
pub const APP_UPDATES_URL: &str = ${JSON.stringify(brand.appUpdatesUrl)};
/// The minisign public key (base64, as tauri writes it) the app files in it
/// are signed with. Empty: no app updates in this build.
pub const APP_UPDATES_KEY: &str = ${JSON.stringify(brand.appUpdatesKey)};

/// Version shown to users: \`major.minor.micro\` for releases,
/// \`preview-{commit}\` for a preview, \`dev-{commit}\` (maybe \`-dirty\`) otherwise.
pub const VERSION: &str = env!("APP_VERSION");
/// Date of the built commit, \`YYYY-MM-DD\`, empty outside git.
pub const BUILD_DATE: &str = env!("APP_BUILD_DATE");
`
  .split('\n')
  .map(rustfmtLine)
  .join('\n')

/**
 * A long top level `pub const X: &str = "...";` the way rustfmt writes it:
 * the value on the next line when it then fits in 100 columns.
 * @param {string} line
 */
function rustfmtLine(line) {
  const match = /^(pub const \w+: &str =) (".*";)$/.exec(line)
  if (!match || line.length <= 100) return line
  const head = match[1] ?? ''
  const value = match[2] ?? ''
  return 4 + value.length <= 100 ? `${head}\n    ${value}` : line
}

/**
 * Applies an identity to a tauri config.
 * @param {any} conf
 * @param {{ displayName: string, slug: string, identifier: string }} values
 */
function tauriIdentity(conf, values) {
  conf.productName = values.displayName
  conf.mainBinaryName = values.slug
  conf.identifier = values.identifier
  conf.app.windows[0].title = values.displayName
  return conf
}

/** @param {unknown} value */
const toJson = (value) => JSON.stringify(value, null, 2) + '\n'

const tauriConf = 'apps/client/src-tauri/tauri.conf.json'

/** The image tag compose files follow: the major version, or before 1.0 the
 * minor one, which is where breaking changes go until then. */
function followedTag() {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const version = cargo.match(/\[workspace\.package\][^[]*?\nversion = "(\d+)\.(\d+)\./)
  if (!version) throw new Error('workspace version not found in Cargo.toml')
  return version[1] === '0' ? `0.${version[2]}` : version[1]
}

/**
 * A compose file for one instance on the machine its reverse proxy runs on.
 * @param {{ name: string, tag: string, port: number, subnet: number }} o
 */
function compose(o) {
  const env = brand.envPrefix
  const gateway = `172.30.${o.subnet}.1`
  const about = `# ${brand.displayName} behind a reverse proxy on this machine.`
  const feedback = `\${FEEDBACK_RECEIVE:-false}`
  return `# ${header}
#
${about}
#
# Next to this file a .env file says where people reach it (copy
# .env.example). Every secret is made on the first start, inside the volume:
# none goes here.
name: ${o.name}

services:
  server:
    image: ${brand.dockerImage}:${o.tag}
    restart: unless-stopped
    # only the proxy on this machine reaches it
    ports:
      - '127.0.0.1:\${PORT:-${o.port}}:8080'
    environment:
      ${env}_SERVER__PUBLIC_URL: \${PUBLIC_URL:?set PUBLIC_URL in .env}
      # the proxy on the host arrives from this network's gateway
      ${env}_SERVER__TRUSTED_PROXIES: '["${gateway}"]'
      ${env}_FEEDBACK__RECEIVE: ${feedback}
      ${env}_PRIVACY__OPERATOR: \${PRIVACY_OPERATOR:-}
      ${env}_PRIVACY__CONTACT: \${PRIVACY_CONTACT:-}
    volumes:
      - data:/data
    read_only: true
    tmpfs:
      - /tmp
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges:true
    logging:
      options:
        max-size: 10m
        max-file: '3'

networks:
  default:
    ipam:
      config:
        - subnet: 172.30.${o.subnet}.0/24
          gateway: ${gateway}

volumes:
  data:
`
}

/** Traefik and the server in one compose project, Traefik getting the
 * certificate. */
function traefik() {
  const env = brand.envPrefix
  return `# ${header}
#
# ${brand.displayName} with Traefik in front, both in Docker, for a machine
# where nothing else answers on ports 80 and 443. Traefik gets and renews
# the certificate from Let's Encrypt. Next to this file a .env file gives
# DOMAIN (the name people use, its DNS pointing here) and ACME_EMAIL (where
# Let's Encrypt writes about the certificate).
name: ${brand.slug}-traefik

services:
  traefik:
    image: traefik:v3.7
    restart: unless-stopped
    command:
      - --providers.docker=true
      # Docker through the proxy below, which answers container listings
      # only: Traefik never holds the Docker socket itself
      - --providers.docker.endpoint=tcp://docker-proxy:2375
      # only the containers labelled traefik.enable are published
      - --providers.docker.exposedbydefault=false
      - --entrypoints.web.address=:80
      - --entrypoints.web.http.redirections.entrypoint.to=websecure
      - --entrypoints.websecure.address=:443
      - --certificatesresolvers.le.acme.httpchallenge=true
      - --certificatesresolvers.le.acme.httpchallenge.entrypoint=web
      - --certificatesresolvers.le.acme.email=\${ACME_EMAIL:?set ACME_EMAIL in .env}
      - --certificatesresolvers.le.acme.storage=/letsencrypt/acme.json
    ports:
      - '80:80'
      - '443:443'
    volumes:
      - certificates:/letsencrypt
    security_opt:
      - no-new-privileges:true
    # a fixed address, the one the server trusts
    networks:
      default:
        ipv4_address: 172.30.82.2
      docker:
    depends_on:
      - docker-proxy

  # The Docker socket gives whoever holds it the whole machine, read only
  # or not. This proxy holds it and answers the container listings Traefik
  # reads the labels from, nothing else, on a network of their own.
  docker-proxy:
    image: ghcr.io/tecnativa/docker-socket-proxy:v0.5.0
    restart: unless-stopped
    environment:
      CONTAINERS: '1'
      EVENTS: '1'
      POST: '0'
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock:ro
    read_only: true
    tmpfs:
      - /run
      - /tmp
    security_opt:
      - no-new-privileges:true
    networks:
      - docker

  server:
    image: ${brand.dockerImage}:${followedTag()}
    restart: unless-stopped
    environment:
      # BASE_PATH in .env serves it under a path of the name, like /notes
      ${env}_SERVER__PUBLIC_URL: https://\${DOMAIN:?set DOMAIN in .env}\${BASE_PATH:-}
      # Traefik only: not the network's gateway, which is how a client
      # reaching Traefik through Docker's own proxy arrives
      ${env}_SERVER__TRUSTED_PROXIES: '["172.30.82.2"]'
      ${env}_PRIVACY__OPERATOR: \${PRIVACY_OPERATOR:-}
      ${env}_PRIVACY__CONTACT: \${PRIVACY_CONTACT:-}
    labels:
      - traefik.enable=true
      - traefik.http.routers.${brand.slug}.rule=Host(\`\${DOMAIN}\`) && PathPrefix(\`\${BASE_PATH:-/}\`)
      - traefik.http.routers.${brand.slug}.entrypoints=websecure
      - traefik.http.routers.${brand.slug}.tls.certresolver=le
      - traefik.http.services.${brand.slug}.loadbalancer.server.port=8080
    volumes:
      - data:/data
    read_only: true
    tmpfs:
      - /tmp
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges:true
    logging:
      options:
        max-size: 10m
        max-file: '3'

networks:
  default:
    ipam:
      config:
        - subnet: 172.30.82.0/24
  # Traefik and the Docker proxy only, no way out
  docker:
    internal: true

volumes:
  data:
  certificates:
`
}

/** The server as a system service, without Docker. */
function systemd() {
  const env = brand.envPrefix
  return `# ${header}
#
# ${brand.displayName} as a system service, without Docker. It runs as its own
# user, ${brand.slug}, and may write only to its data folder,
# /var/lib/${brand.slug} (made by systemd). The config file is
# /etc/${brand.slug}/${brand.configFile}, owned by ${brand.slug}, mode 600. The web pages
# go in /usr/local/share/${brand.slug}/web.
#
#   sudo cp ${brand.serverBinary}.service /etc/systemd/system/
#   sudo systemctl daemon-reload
#   sudo systemctl enable --now ${brand.serverBinary}
#
# A backup folder elsewhere on this machine needs a ReadWritePaths= line
# (sudo systemctl edit ${brand.serverBinary}).

[Unit]
Description=${brand.displayName} server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${brand.slug}
Group=${brand.slug}
ExecStart=/usr/local/bin/${brand.serverBinary} --config /etc/${brand.slug}/${brand.configFile}
WorkingDirectory=/var/lib/${brand.slug}
Environment=${env}_STORAGE__DATA_DIR=/var/lib/${brand.slug}
Environment=${env}_SERVER__WEB_DIR=/usr/local/share/${brand.slug}/web
StateDirectory=${brand.slug}
StateDirectoryMode=0700
UMask=0077
Restart=on-failure
RestartSec=5

# the server needs its port, its folder and the network, nothing more
NoNewPrivileges=yes
CapabilityBoundingSet=
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectKernelLogs=yes
ProtectControlGroups=yes
ProtectClock=yes
ProtectHostname=yes
ProtectProc=invisible
RestrictNamespaces=yes
RestrictRealtime=yes
RestrictSUIDSGID=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
SystemCallArchitectures=native
SystemCallFilter=@system-service
SystemCallFilter=~@privileged @resources

[Install]
WantedBy=multi-user.target
`
}

/**
 * Each target returns the new file content from the current one.
 * @type {Array<[string, (current: string) => string]>}
 */
const targets = [
  ['crates/brand/src/lib.rs', () => rust],
  // the base config carries the dev identity so a plain tauri build never
  // overwrites an installed release
  [tauriConf, (current) => toJson(tauriIdentity(JSON.parse(current), brand.dev))],
  [
    // release overlay, merged by tauri with --config. Merging replaces arrays
    // whole, so the full window list is copied from the base config.
    'apps/client/src-tauri/tauri.release.conf.json',
    () => {
      const base = JSON.parse(readFileSync(join(root, tauriConf), 'utf8'))
      const conf = tauriIdentity({ app: { windows: base.app.windows } }, brand)
      // the installer comes with the signature the app checks updates
      // against (commands/updates.rs). The app has no updater plugin, the
      // bundler reads this key to check TAURI_SIGNING_PRIVATE_KEY matches it
      // the release's own icon: the base config has the dev one
      conf.bundle = {
        createUpdaterArtifacts: true,
        icon: ['32x32.png', '128x128.png', '128x128@2x.png', 'icon.icns', 'icon.ico'].map(
          (file) => `icons/${file}`,
        ),
      }
      conf.plugins = { updater: { pubkey: brand.appUpdatesKey } }
      return toJson(conf)
    },
  ],
  [
    // portable overlay, merged after the release one: the code makes the
    // main window, with the webview's folder next to the exe
    // (src-tauri/src/portable.rs)
    'apps/client/src-tauri/tauri.portable.conf.json',
    () => {
      const base = JSON.parse(readFileSync(join(root, tauriConf), 'utf8'))
      const { windows } = tauriIdentity({ app: { windows: base.app.windows } }, brand).app
      return toJson({
        app: { windows: windows.map((/** @type {object} */ w) => ({ ...w, create: false })) },
      })
    },
  ],
  [
    'apps/server/Cargo.toml',
    (current) => current.replace(/(\[\[bin\]\]\nname = )"[^"]*"/, `$1"${brand.serverBinary}"`),
  ],
  [
    'deploy/Dockerfile',
    (current) =>
      current
        .replace(/^ARG BINARY=.*$/m, `ARG BINARY=${brand.serverBinary}`)
        .replace(/^ARG ENV_PREFIX=.*$/m, `ARG ENV_PREFIX=${brand.envPrefix}`)
        .replace(/^ARG SLUG=.*$/m, `ARG SLUG=${brand.slug}`)
        // exec form takes no variables
        .replace(/"\/usr\/local\/bin\/[^"]+"/g, `"/usr/local/bin/${brand.serverBinary}"`)
        .replace(/^(#\s+docker build -f deploy\/Dockerfile -t )\S+/m, `$1${brand.serverBinary}`),
  ],
  [
    'deploy/compose.yml',
    () => compose({ name: brand.slug, tag: followedTag(), port: 8080, subnet: 80 }),
  ],
  ['deploy/traefik/compose.yml', traefik],
  [`deploy/systemd/${brand.serverBinary}.service`, systemd],
]

let drift = false
for (const [path, render] of targets) {
  const file = join(root, path)
  const current = existsSync(file) ? readFileSync(file, 'utf8') : ''
  const next = render(current)
  if (next === current) continue
  if (check) {
    console.error(`out of date: ${path}`)
    drift = true
  } else {
    mkdirSync(dirname(file), { recursive: true })
    writeFileSync(file, next)
    console.log(`updated ${path}`)
  }
}

if (drift) {
  console.error('run pnpm brand to update')
  process.exit(1)
}
