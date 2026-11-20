// Runs only as DeepGeminiProxy's private child process. No standalone admin UI is exposed.
import crypto from 'node:crypto';
import { createInterface } from 'node:readline';
import { createGateway } from './upstream/src/gateway/app.mjs';
import { StateStore } from './upstream/src/gateway/state.mjs';

const input = createInterface({ input: process.stdin });
let closed = false;
let gateway;
let store;
let shutdown;
function stop() {
  closed = true;
  return shutdown ??= (async () => {
    try {
      if (gateway?.server.listening) await gateway.close();
      else await gateway?.accounts.close();
    } finally {
      store?.unlock();
      input.close();
    }
  })();
}
input.once('close', () => { void stop(); });
for (const signal of ['SIGTERM', 'SIGINT']) {
  process.once(signal, () => { void stop(); });
}

function json(res, status, body) {
  res.writeHead(status, { 'content-type': 'application/json', 'cache-control': 'no-store' });
  res.end(JSON.stringify(body));
}

const safeErrors = Object.freeze({
  ACCOUNT_LIMIT: 'account_limit',
  ACCOUNT_NOT_FOUND: 'account_not_found',
  LOGIN_ALREADY_PENDING: 'oauth_login_pending',
  LOGIN_NOT_FOUND: 'oauth_flow_not_found',
  OAUTH_PROFILE_REQUIRED: 'oauth_profile_required',
  OAUTH_LISTENER_UNAVAILABLE: 'oauth_listener_unavailable',
  REFRESH_TOKEN_REQUIRED: 'oauth_refresh_token_missing',
  STATE_SAVE_FAILED: 'account_store_unavailable',
  CONFIRMATION_REQUIRED: 'gemini_app_terms_confirmation_required',
  ACCOUNT_LOGIN_REQUIRED: 'gemini_app_terms_login_required',
  TOS_TIMEOUT: 'gemini_app_terms_timeout',
  TOS_VERSION_MISSING: 'gemini_app_terms_version_missing',
});

function diagnostics(evidence) {
  if (!evidence) return null;
  // Accounts.list() already applies App2API's upstream message redaction.
  return {
    upstream_http_status: evidence.upstreamHttpStatus ?? null,
    grpc_status: evidence.grpcStatus ?? null,
    message: evidence.upstreamMessage ?? null,
  };
}

function accountView(account) {
  const terms = account.terms;
  const failure = account.lastError;
  return {
    // Keep the Rust/Web account DTO stable while App2API owns its IDs.
    id: account.id.replace(/^acc_([0-9a-f]{4})([0-9a-f]{4})([0-9a-f]{12})$/, '00000000-0000-$1-$2-$3'),
    email: account.label,
    needs_login: !account.enabled || account.authenticationStatus !== 'signed_in',
    terms: {
      status: terms.status,
      version: terms.version,
      accepted_at: terms.acceptedAt,
      operation: terms.evidence?.operation ?? null,
      diagnostic: diagnostics(terms.evidence),
    },
    last_error: failure ? {
      code: failure.code,
      model: failure.model ?? null,
      at: failure.at,
      diagnostic: diagnostics(failure.nativeEvidence),
    } : null,
  };
}

async function readConfirmation(req) {
  const invalid = () => Object.assign(new Error('Explicit confirmation required'), { code: 'CONFIRMATION_REQUIRED', status: 400 });
  const chunks = [];
  let bytes = 0;
  for await (const chunk of req.iterator({ destroyOnReturn: false })) {
    bytes += chunk.length;
    if (bytes > 1024) throw invalid();
    chunks.push(chunk);
  }
  try {
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    if (body?.confirmed === true && Object.keys(body).length === 1) return;
  } catch { /* Missing or malformed JSON is never consent. */ }
  throw invalid();
}

async function privateRequest(accounts, req, res) {
  const { method, url = '' } = req;
  if (method === 'GET' && url === '/__dgp/accounts') {
    return accounts.list().map(accountView);
  }
  if (method === 'POST' && url === '/__dgp/login') {
    return accounts.beginLogin(undefined, { acceptTerms: false }).then(flow => ({
      flow_id: flow.flowId,
      authorization_url: flow.authorizationUrl,
    }));
  }
  const flow = /^\/__dgp\/login\/([0-9a-f-]{36})$/.exec(url);
  if (flow && method === 'GET') {
    const result = accounts.flow(flow[1]);
    return { status: result.status, error: result.errorCode ? (safeErrors[result.errorCode] ?? 'oauth_denied') : null };
  }
  if (flow && method === 'DELETE') {
    try { accounts.cancelFlow(flow[1]); }
    catch (error) { if (error?.code !== 'LOGIN_NOT_FOUND') throw error; }
    return null;
  }
  const account = /^\/__dgp\/accounts\/00000000-0000-([0-9a-f]{4})-([0-9a-f]{4})-([0-9a-f]{12})(\/terms)?$/.exec(url);
  if (account) {
    const id = `acc_${account.slice(1, 4).join('')}`;
    if (method === 'DELETE' && !account[4]) {
      accounts.remove(id);
      return null;
    }
    if (method === 'POST' && account[4]) {
      await readConfirmation(req);
      const current = accounts.list().find(entry => entry.id === id);
      if (!current) throw Object.assign(new Error('Account not found'), { code: 'ACCOUNT_NOT_FOUND', status: 404 });
      if (!current.enabled || current.authenticationStatus !== 'signed_in') {
        throw Object.assign(new Error('Account sign-in required'), { code: 'ACCOUNT_LOGIN_REQUIRED', status: 409 });
      }
      const controller = new AbortController();
      const abort = () => controller.abort();
      res.once('close', abort);
      try {
        if (res.destroyed) controller.abort();
        await accounts.acceptTerms(id, { confirmed: true, signal: controller.signal });
        return accountView(accounts.list().find(entry => entry.id === id));
      } finally {
        res.off('close', abort);
      }
    }
  }
  throw Object.assign(new Error('Not found'), { status: 404 });
}

input.once('line', async key => {
  try {
    if (closed || !/^[0-9a-f]{64}$/.test(key) || !process.argv[2]) throw new Error('Invalid bootstrap');
    const authorizer = Buffer.from(`Bearer ${key}`);
    store = new StateStore({ directory: process.argv[2], adminPassword: '' });
    if (process.platform !== 'win32') store.lock();
    store.authenticate = candidate => {
      if (typeof candidate !== 'string' || candidate.length !== key.length) return null;
      const value = Buffer.from(candidate);
      return value.length === key.length && crypto.timingSafeEqual(value, Buffer.from(key)) ? { id: 'embedded' } : null;
    };
    // Optional fixed OAuth callback for a container whose port is published on host loopback.
    const port = process.env.ABV_GEMINI_APP_DOCKER_CALLBACK_PORT;
    if (port && (!/^\d{1,5}$/.test(port) || Number(port) < 1024 || Number(port) > 65535)) {
      throw new Error('Invalid OAuth callback port');
    }
    gateway = createGateway({
      store,
      callbackPort: port ? Number(port) : 0,
      callbackHost: port ? '0.0.0.0' : '127.0.0.1',
    });
    const [serve] = gateway.server.listeners('request');
    if (typeof serve !== 'function') throw new Error('Gateway handler unavailable');
    gateway.server.removeAllListeners('request');
    gateway.server.on('request', async (req, res) => {
      const raw = typeof req.headers.authorization === 'string' ? Buffer.from(req.headers.authorization) : Buffer.alloc(0);
      if (raw.length !== authorizer.length || !crypto.timingSafeEqual(raw, authorizer)) {
        return json(res, 401, { error: 'unauthorized' });
      }
      if (req.method === 'POST' && ['/v1/chat/completions', '/v1/responses'].includes(req.url)) {
        return serve(req, res);
      }
      try {
        json(res, 200, await privateRequest(gateway.accounts, req, res));
      } catch (error) {
        const status = Number.isInteger(error?.status) && error.status >= 400 && error.status <= 599 ? error.status : 500;
        json(res, status, { error: safeErrors[error?.code] ?? 'gemini_app_embedded_request_failed' });
      }
    });
    await gateway.listen({ host: '127.0.0.1', port: 0 });
    if (closed) { await gateway.close(); return; }
    process.stdout.write(`${JSON.stringify({ port: gateway.server.address().port })}\n`);
  } catch {
    process.exitCode = 1;
    await stop();
  }
});
