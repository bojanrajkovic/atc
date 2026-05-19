# Authentication

Last verified: 2026-05-18

> Operator runbook. Architectural rationale lives in [`docs/architecture/deployment.md`](../architecture/deployment.md#authentication). This document holds the per-proxy recipes.

## What ATC ships

**No built-in authentication.** The SPA, `GET /v1/state`, and `GET /v1/ws` are open to anyone who can reach the HTTP port. The webhook endpoint at `POST /v1/webhooks/github` validates HMAC-SHA256 signatures when `ATC_GITHUB__WEBHOOK_SECRET` is configured; nothing else is gated.

This is a deliberate scope decision. ATC accepts the surrounding deployment's identity model rather than ship its own OIDC / SAML / session-store subsystem.

## Supported patterns

- **Private network.** Deploy into a VPC, a homelab subnet, a Tailscale tailnet — any network where the access-control answer is "you have to already be inside." Pair with the chart's NetworkPolicy (hardened `from` list scoped to ingress controllers + VPN endpoints) when CNI enforcement is available.
- **Authenticating reverse proxy.** Front the Service with a proxy that runs an OIDC / OAuth2 flow against an upstream IdP and forwards the authenticated session to ATC. Recipes for the common proxies are below.
- **Ingress annotations.** The chart's Ingress (`templates/ingress.yaml`) passes `ingress.annotations` through, so operators can wire any ingress-class-specific auth filter (nginx `auth_request`, Traefik middleware chains, etc.) without modifying the chart.
- **Gateway API attachment.** The chart's `HTTPRoute` (`templates/httproute.yaml`) does not currently expose `annotations` through chart values. Gateway API auth attaches via the API's native mechanisms instead — Envoy Gateway's `SecurityPolicy` resource keyed by `targetRef` on the HTTPRoute, an HTTPRoute `filters` entry with `type: ExtensionRef`, or whatever the operator's Gateway implementation supports. Operators who want chart-managed `HTTPRoute` annotations should open an issue — it's a small, additive chart change.

## Recipes

Every recipe below splits the route surface into two policies: **webhook bypass** for `POST /v1/webhooks/github` (HMAC does the gating) and **authenticated** for everything else (SPA + `GET /v1/state` + `GET /v1/ws`). The WebSocket upgrade always inherits the SPA's session cookie because they're same-origin; the only thing the proxy must do is forward the `Upgrade` and `Connection` headers and not impose a short idle timeout on the upgraded connection.

### Pomerium (recommended)

Pomerium is the most mature fit. Per-route policy is first-class; `allow_public_unauthenticated_access: true` covers the webhook bypass and `allow_websockets: true` covers the WS upgrade. JWT claims are forwarded to the upstream via `X-Pomerium-Jwt`, so a future ATC version could pick them up for audit logging without changing proxy config.

```yaml
routes:
  # Webhook: public, no policy, no WS
  - from: https://atc.example.com
    to: http://atc.svc.cluster.local:8080
    path: /v1/webhooks/github         # exact match — prefix would also expose /v1/webhooks/githubfoo
    allow_public_unauthenticated_access: true

  # SPA + REST + WS: authenticated, WS enabled, explicit idle timeout
  - from: https://atc.example.com
    to: http://atc.svc.cluster.local:8080
    allow_websockets: true
    idle_timeout: 1h            # ATC's WS is long-lived; default policy timeouts will drop it
    policy:
      - allow:
          and:
            - domain: { is: example.com }
```

**Why `idle_timeout` explicitly:** Pomerium's global timeout policies do not apply to upgraded WebSocket connections; the route-level `idle_timeout` is the only knob that keeps them open. Without it, the WS gets dropped by the proxy's default request budget. Setting `0s` means unlimited (use with caution — slowloris surface).

Source: [Pomerium Public Access](https://www.pomerium.com/docs/reference/routes/public-access), [WebSocket support](https://www.pomerium.com/docs/capabilities/routing).

### oauth2-proxy

The current canonical bypass flag is `--skip-auth-route`, with syntax `method=path_regex`. The older `--skip-auth-regex` is deprecated. WebSocket proxying is on by default (`--proxy-websockets`).

```bash
# K8s/ingress shape — see security advisory below before enabling --reverse-proxy
oauth2-proxy \
  --provider=oidc \
  --oidc-issuer-url=https://idp.example.com/ \
  --client-id=atc \
  --client-secret=$OAUTH_CLIENT_SECRET \
  --cookie-secret=$OAUTH_COOKIE_SECRET \
  --email-domain=example.com \
  --upstream=http://atc.svc.cluster.local:8080 \
  --skip-auth-route='POST=^/v1/webhooks/github$' \
  --proxy-websockets=true \
  --upstream-timeout=1h \
  --reverse-proxy=true \
  --trusted-proxy-ip=10.0.0.0/8       # MUST narrow to ingress CIDR; default 0.0.0.0/0 is unsafe
```

**Method scoping matters:** `POST=^/v1/webhooks/github$` only bypasses POST. A GET to the same path would still hit the auth flow (and 404 from ATC), which is the right shape.

**Security advisory — `--reverse-proxy` + `--skip-auth-route` is a footgun on oauth2-proxy < 7.15.2.** [GHSA-7x63-xv5r-3p2x](https://github.com/oauth2-proxy/oauth2-proxy/security/advisories/GHSA-7x63-xv5r-3p2x) — affected versions 7.5.0 through 7.15.1, fixed in 7.15.2 — let a client-supplied `X-Forwarded-Uri` header rewrite the path oauth2-proxy evaluated against `--skip-auth-route`, so an attacker could make a request to `/v1/ws` look like `/v1/webhooks/github` and bypass auth entirely. Three layered mitigations are required for any K8s ingress shape:

1. **Pin oauth2-proxy to v7.15.2 or later.** Upgrade alone is necessary but not sufficient.
2. **Set `--trusted-proxy-ip` explicitly** to your ingress controller's pod / Service CIDR. The default is `0.0.0.0/0` for backward compatibility — leaving it unset trusts every source IP, including any attacker, which preserves the bug. Narrow it.
3. **Strip or overwrite `X-Forwarded-Uri` at the ingress layer.** nginx-ingress: add `more_clear_input_headers "X-Forwarded-Uri";` (requires the headers-more module); Envoy / Traefik: equivalent header-removal filter on the route. This is defense-in-depth even with the trusted-proxy-ip mitigation in place.

If you don't run oauth2-proxy behind a reverse proxy (single-node bare-metal, direct exposure), `--reverse-proxy=false` (the default) means `X-Forwarded-*` headers are ignored entirely and the advisory doesn't apply.

**Open issue worth knowing about:** [`oauth2-proxy#2996`](https://github.com/oauth2-proxy/oauth2-proxy/issues/2996) — `Origin` is not validated on WS upgrades. See § Cross-cutting gotchas.

Source: [oauth2-proxy Configuration Overview](https://oauth2-proxy.github.io/oauth2-proxy/configuration/overview), [GHSA-7x63-xv5r-3p2x](https://github.com/oauth2-proxy/oauth2-proxy/security/advisories/GHSA-7x63-xv5r-3p2x).

### Authelia + nginx

Authelia provides the auth decision; nginx routes traffic, runs the `auth_request` subrequest for gated paths, and skips it for the webhook. Two snippets are required: the standard `authelia-authrequest.conf` and an explicit `websocket.conf` block that forwards `Upgrade` / `Connection` headers.

```yaml
# authelia configuration.yml
access_control:
  default_policy: 'deny'
  rules:
    - domain: 'atc.example.com'
      resources: ['^/v1/webhooks/github([/?].*)?$']
      methods: ['POST']
      policy: 'bypass'
    - domain: 'atc.example.com'
      policy: 'one_factor'         # or two_factor for the SPA + /v1/ws
```

```nginx
# nginx server block
server {
  listen 443 ssl http2;
  server_name atc.example.com;

  # Webhook: no auth subrequest; HMAC gates
  location = /v1/webhooks/github {
    proxy_pass http://atc.svc.cluster.local:8080;
  }

  # SPA + REST + WS: gated via auth_request
  location / {
    include /config/nginx/snippets/authelia-authrequest.conf;
    proxy_pass http://atc.svc.cluster.local:8080;

    # WS upgrade headers — Upgrade and Connection are hop-by-hop, must be forwarded explicitly.
    # proxy_http_version 1.1 is required: nginx's default upstream protocol is HTTP/1.0,
    # which strips the Upgrade headers and never reaches the 101 Switching Protocols response.
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";

    # Long-lived WS; default proxy_read_timeout is 60s
    proxy_read_timeout 3600s;
  }
}
```

**Rule ordering matters:** Authelia evaluates rules sequentially. The `bypass` rule must appear above the catch-all rule, or the catch-all wins.

**Header gotcha:** [`authelia#5350`](https://github.com/authelia/authelia/discussions/5350) — the `Upgrade` and `Connection` headers are hop-by-hop per RFC 7230 and nginx does not forward them by default. They must be set explicitly on the WS location.

Sources: [Authelia Access Control](https://www.authelia.com/configuration/security/access-control/), [Authelia nginx Integration](https://www.authelia.com/integration/proxies/nginx/), [nginx WebSocket Proxying](https://nginx.org/en/docs/http/websocket.html).

### Authelia + Caddy

Caddy's `forward_auth` is the equivalent of `auth_request`. The Caddyfile keeps webhook traffic on a route that doesn't invoke `forward_auth`. The WS upgrade is handled automatically by `reverse_proxy`.

```caddyfile
atc.example.com {
  # Webhook: no auth, HMAC gates
  handle /v1/webhooks/github {
    reverse_proxy atc.svc.cluster.local:8080
  }

  # SPA + REST + WS: gated via forward_auth
  handle {
    forward_auth authelia.svc.cluster.local:9091 {
      uri /api/verify?rd=https://auth.example.com/
      copy_headers Remote-User Remote-Groups Remote-Email Remote-Name
    }
    reverse_proxy atc.svc.cluster.local:8080
  }
}
```

Caddy's `reverse_proxy` passes WebSocket upgrades through transparently and applies no per-stream timeouts by default, so no override is needed for long-lived ATC WS. Avoid adding `transport http { read_timeout / write_timeout }` here: those are per-read/per-write deadlines that will silently disconnect quiet dashboards because ATC sends no application-level ping. If you want a hard maximum lifetime (to force reconnects), use `stream_timeout` instead — but for ATC the safest config is no idle cutoff, letting OS-level TCP keepalive detect dead connections.

Sources: [Caddy `forward_auth`](https://caddyserver.com/docs/caddyfile/directives/forward_auth), [Caddy `reverse_proxy`](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy).

### Cloudflare Access

A single Cloudflare Access Application fronting `atc.example.com/*` works for the full SPA + REST + WS surface in the standard browser flow. The Access cookie is set when the user authenticates against the SPA load; subsequent requests — including the WebSocket upgrade — carry the cookie on the same origin, and Access validates the upgrade as the HTTP request the WS connection counts as. No split-proxy required.

For the webhook bypass, add a second Access Application keyed on a more specific path with action `Bypass`. Cloudflare matches the most-specific path first.

| Application | Path | Action |
|-------------|------|--------|
| `atc-webhook` | `atc.example.com/v1/webhooks/github*` | Bypass (Everyone) — HMAC gates inside ATC |
| `atc-app` | `atc.example.com/*` | Allow with IdP rules — gates SPA + REST + WS |

**Do NOT add a `Bypass` policy for `/v1/ws`.** Cloudflare documents `Bypass` as disabling Access enforcement entirely with no identity checks. ATC has no server-side session validation to backstop, so a Bypass on `/v1/ws` would make the live event stream — which carries the same workflow / job / runner-pool data the rest of the surface is gated on — fully public. The WS route belongs under the authenticated `atc-app` Application, not under a Bypass.

**Known limitations.**

- **Non-browser WebSocket clients won't authenticate.** Access cannot serve its login challenge on a WS upgrade (a browser cannot follow a redirect on a `Connection: Upgrade` request). This only affects clients that hit `/v1/ws` directly without first loading the SPA through Access — a CLI scraper, a synthetic test, etc. ATC has no first-party non-browser WS consumers today, so this is academic; if you build one, plan to fetch a Service Token and attach it as `CF-Access-Client-Id` / `CF-Access-Client-Secret` headers on the upgrade request.
- **Cookie expiry mid-session.** The Access session cookie has a finite lifetime (default 24 h). An already-established WebSocket keeps running until the TCP connection drops — Access doesn't tear it down mid-stream — but a reconnect attempt after the cookie expires will fail the upgrade. The user reloads the page to re-authenticate; the SPA's reconnect loop will keep retrying in the meantime, so the failure mode is "stuck reconnecting" rather than data loss.

Source: [Cloudflare Access Policies](https://developers.cloudflare.com/cloudflare-one/policies/access/).

### Traefik / Envoy Gateway / Istio (brief)

The pattern generalizes — every per-route proxy supports the same shape:

- **Traefik.** Define one router for the webhook path with no `middlewares:` attached; another for the rest with a `forwardAuth` or OIDC middleware in the chain. Per-router scoping is native.
- **Envoy Gateway.** `SecurityPolicy` resources target a specific `HTTPRoute` via `targetRef`. Define two `HTTPRoute`s (one for the webhook path, one for the catch-all), attach the policy to only the catch-all route.
- **Istio.** `AuthorizationPolicy` with path-based matchers; one policy with `action: ALLOW` for the webhook path, another with `action: CUSTOM` (or DENY by default) for the rest. Use `RequestAuthentication` for the JWT validation.

Forward `Upgrade` / `Connection` in all three (Traefik does this automatically; Envoy needs `upgradeConfigs` on the route; Istio inherits whatever the ingress gateway does).

## Cross-cutting gotchas

### `Origin` is not validated by ATC

None of the proxies above validate the `Origin` header on the WS upgrade either, which leaves a CSRF surface: a malicious page loaded under the same authenticated session could open `/v1/ws` and read the event stream. For deployments where this matters:

- **nginx:** `if ($http_origin !~ "^https://atc\.example\.com$") { return 403; }` inside the WS location.
- **Caddy:** `@badorigin not header_regexp Origin "^https://atc\.example\.com$"` matcher with `respond @badorigin 403`.
- **Pomerium:** add a PPL rule on the WS route asserting `request.headers.Origin == "https://atc.example.com"`.
- **Envoy:** `header_match` filter on `Origin`.

Adding native `Origin` validation in `atc-server` is on the table — file an issue if you need it.

### Cookie `SameSite`

`SameSite=Lax` is the safe default for the auth proxy's session cookie. `SameSite=Strict` can drop the cookie on cross-scheme transitions even when the WS is same-origin; if your proxy issues `Strict` cookies, the WS upgrade may silently fail with a `1008 Unauthorized` (or whatever code your proxy emits when the session cookie is missing).

### Idle-timeout starvation

ATC's WS is event-driven — there is no application-protocol ping inside the connection. Where the proxy imposes a default idle timeout, quiet periods will drop the connection. The client reconnects and re-fetches `/v1/state` so it's recoverable, but noisy. Per-proxy guidance:

- **Pomerium:** set `idle_timeout: 1h` on the WS route — Pomerium's default request budget would otherwise drop the upgrade.
- **oauth2-proxy:** set `--upstream-timeout=1h`.
- **nginx:** set `proxy_read_timeout 3600s` — nginx's default is 60 s.
- **Caddy:** no override needed. `reverse_proxy` passes WS upgrades through with no per-stream timeout by default; `transport http { read_timeout / write_timeout }` is per-read/per-write on the backend connection and would actively add an idle cutoff that isn't there. If you want a hard maximum lifetime (to force reconnects), use `stream_timeout`.

### Sticky sessions are not required

ATC's recovery model is reconnect-to-any-healthy-replica via `/v1/state` + `lastSeq`; pinning the session to one replica masks gap-healing regressions during development. If your proxy's load-balancing model is round-robin, leave it alone.

### `X-Forwarded-For` and audit logs

ATC does not log frontend reads today. If you want IP audit trail on the reverse-proxy side, configure your proxy to log requests (most do this by default). Don't rely on `X-Forwarded-For` reaching ATC unless you've set `axum`'s trust configuration appropriately — out of scope for this document.

## Webhook endpoint — why the split

`POST /v1/webhooks/github` should NOT sit behind the same auth flow as the SPA. GitHub does not authenticate to OIDC providers, and forcing it to would either drop every delivery or require GitHub to acquire a token per-call (it cannot). The endpoint is gated independently by HMAC-SHA256 verification of the `X-Hub-Signature-256` header against the configured `ATC_GITHUB__WEBHOOK_SECRET`.

Two layouts work:

1. **Same proxy, path-bypass.** What the recipes above do — declare `/v1/webhooks/github` as a public route on the same proxy that gates the rest. Simplest operationally; one Ingress, one cert, one DNS record.
2. **Separate Ingress / HTTPRoute.** If your auth proxy can't be configured for per-path policy (rare), expose the webhook endpoint on a sibling Ingress with no auth attached, pointed at the same Service. Both Ingresses can share a hostname (`atc.example.com`); routing precedence handles which Ingress wins per-path.

**Always configure `ATC_GITHUB__WEBHOOK_SECRET`** before exposing the webhook endpoint publicly. Without it, HMAC verification is skipped (`webhook_secret: None`) and anyone who knows the URL can forge events into the state machine.

## Not supported (today)

- **First-class OIDC inside `atc-server`.** No per-request token validation, no session store.
- **Per-repository or per-org access control.** The webhook firehose is per-deployment; partition by running separate deployments per access boundary.
- **Audit logging of frontend reads.** Reverse-proxy access logs are the workaround.
- **Native `Origin` allowlist on the WS endpoint.** See § Cross-cutting gotchas for the proxy-side mitigations.

If any of these matter for your deployment, open a GitHub issue describing the operator surface you'd want.
