// SPDX-License-Identifier: AGPL-3.0-or-later
// Receives the apps' feedback in place of a Ficelle server, on the same
// three api calls, and keeps a private inbox at `/inbox`. Reports live in
// D1, their images in R2. A daily run removes what has had its time.

import type { AccessSecrets } from './access'
import { answers, challenge, send } from './api'
import { apiError, checkProtocol, noContent } from './http'
import { inbox } from './inbox'
import { cleanup } from './store'

const FEEDBACK_PATH = '/api/v1/feedback'

type Bindings = Env & AccessSecrets

/**
 * Any origin's web app may call the feedback routes, without credentials
 * (none are read there). The preflight is answered before the protocol
 * check, browsers send it without custom headers.
 */
async function feedback(request: Request, env: Bindings, path: string): Promise<Response> {
  let response: Response
  if (request.method === 'OPTIONS') {
    response = noContent({
      'access-control-allow-methods': 'GET, POST',
      'access-control-allow-headers': 'content-type, x-protocol-version',
      'access-control-max-age': '86400',
    })
  } else {
    response = checkProtocol(request) ?? (await route(request, env, path))
  }
  response.headers.set('access-control-allow-origin', '*')
  response.headers.set('access-control-expose-headers', 'retry-after')
  // read from other origins, the default same-origin would block it
  response.headers.set('cross-origin-resource-policy', 'cross-origin')
  response.headers.set('cache-control', 'no-store')
  response.headers.set('x-content-type-options', 'nosniff')
  return response
}

function route(request: Request, env: Bindings, path: string): Promise<Response> | Response {
  if (request.method === 'POST') {
    if (path === `${FEEDBACK_PATH}/challenge`) return challenge(request, env)
    if (path === FEEDBACK_PATH) return send(request, env)
    if (path === `${FEEDBACK_PATH}/answers`) return answers(request, env)
  }
  return apiError(404, 'notFound', 'no such endpoint')
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url)
    const path = url.pathname
    if (path === FEEDBACK_PATH || path.startsWith(`${FEEDBACK_PATH}/`)) {
      return feedback(request, env, path)
    }
    if (path === '/inbox' || path.startsWith('/inbox/')) return inbox(request, env, url)
    return apiError(404, 'notFound', 'no such endpoint')
  },
  async scheduled(_controller, env) {
    const removed = await cleanup(env.DB, env.IMAGES, Date.now())
    console.log(`feedback cleanup removed ${removed} rows`)
  },
} satisfies ExportedHandler<Bindings>
