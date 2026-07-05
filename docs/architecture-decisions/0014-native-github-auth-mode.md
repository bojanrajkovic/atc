# 0014 — Native GitHub auth mode (`auth.github`)

Date: 2026-07-03
Status: Accepted

## Context

ATC has two existing authentication postures: no auth (default) and
delegation to a reverse proxy that injects an identity header. Neither gives
ATC per-repository authorization data — proxy-header identity says *who* the
user is, not *which repositories* they may see. A design review on issue
[#234](https://github.com/bojanrajkovic/atc/issues/234) worked through the
session, credential, and identity-key tradeoffs for a first-party mode that
can filter dashboard visibility by the user's actual GitHub repository
access. This ADR records the five decisions from that review.

## Decision

### 1. First-party `auth.github` mode, alongside — not replacing — no-auth and reverse-proxy modes

ATC gains a third, opt-in authentication mode that owns its own login flow,
session storage, and per-repository filtering. The existing no-auth default
and reverse-proxy delegation are unaffected.

**Rejected: composing on top of the existing reverse-proxy mode.**
Proxy-injected identity headers carry an authenticated *identity*, not an
*authorization set* — the proxy has no notion of which GitHub repositories
that identity may see. Per-repo filtering needs data only GitHub can supply,
so it cannot be built as a thin layer over proxy auth.

### 2. Two GitHub surfaces: unchanged webhook ingestion, plus a separate metadata-only GitHub App for login

Webhook ingestion — org- or repo-configured webhooks delivering
`workflow_run`/`workflow_job` events over HMAC-verified requests — is
untouched by this initiative; it is today's existing ingestion path. A
second, independent GitHub App is introduced solely for user login and
authorization-set derivation, scoped to the minimum permission GitHub allows
for that purpose.

**Rejected: a single GitHub App serving both webhooks and login.** Webhook
subscription for `workflow_run`/`workflow_job` requires an `Actions: read`
permission grant; folding login into the same app would raise the user
access token's permission ceiling to match, well beyond what authorization
derivation needs. Per-deployment GitHub Apps to avoid that (one app per ATC
install, each with its own webhook registration) were also rejected: a
GitHub App has exactly one webhook URL fixed at registration time, so
tying webhook delivery to app identity causes app sprawl across
self-hosted deployments. Splitting the two concerns avoids both problems:
webhook delivery keeps its existing manual-configuration model, and the
login app's permission ceiling stays minimal regardless of how many
repositories send webhooks.

The practical consequence of the split is a two-surface coverage rule:
a repository is visible to a given user only if it (a) sends ATC webhooks,
(b) has the login app installed, and (c) is accessible to that user on
GitHub. All three conditions must hold — **except for a publicly-visible
repository**, which is visible to every session once (a) and (b) hold,
regardless of (c). A public repository is readable by anyone on GitHub
regardless of collaborator access, so gating it on the logging-in user's own
access was a missed requirement, not a deliberate restriction: `GET
/repositories/{id}` (GitHub's `visibility` field, not `private` — see below)
answers "is this repo public" without needing a user token at all. ATC
checks this directly for every repo it already has run data for (condition
(a) already bounds that set — a repo with no webhook has no run data
regardless of visibility) and unions the public subset into every session's
`repo_ids` at callback, using Basic auth with the login app's own
`client_id`/`client_secret` (already held for the OAuth flow) for the
higher, 5,000/hr OAuth-app rate ceiling rather than the 60/hr unauthenticated
one. The result is cached app-wide with the same `repo_auth_ttl` staleness
window already governing per-user authorization, so a repository that flips
public/private stays visible/hidden for at most one `repo_auth_ttl` window
after the flip — an in-process, per-replica cache, not a shared one:
replicas can disagree on the public set for up to that window, the same
accepted-staleness posture already covering per-user authorization.

`visibility`, not `private`, is the field this check reads: GitHub
Enterprise "internal" repositories (visible to all org members, not the
public internet) report `private: true`, so `private == false` would
incorrectly include them.

### 3. Authorization keyed by immutable `repository.id`, never `org/repo` strings

The authorization set derived at login, and the identity carried on ingested
events, is GitHub's immutable numeric repository ID — not the `org/repo`
display string.

**Rejected: string-keyed authorization (`org/repo`).** Repository renames,
transfers between owners, and name reuse after deletion all change the
string while the underlying repository is unchanged (or, in the reuse case,
change the underlying repository while the string is unchanged) — either
direction breaks an authorization decision keyed on the string. The
immutable ID has neither failure mode. `org/repo` remains in use as display
metadata; it is simply not the authorization key.

### 4. No GitHub tokens stored at rest; staleness resolved by silent re-authentication

At OAuth callback, ATC exchanges the authorization code for a token, uses it
once to derive the authorized repository-ID set, and discards both the
access token and any refresh token — neither is persisted. The stored
session carries only the derived repository-ID set and a timestamp. That
set is treated as valid for a bounded window (`repo_auth_ttl`); once stale,
ATC re-derives it via a silent OAuth round-trip rather than a stored
refresh token. Re-authentication is popup-first (`window.open` plus a
same-origin `BroadcastChannel` to signal completion back to the open
dashboard, since navigating through GitHub can sever the window-opener
relationship), falling back automatically to a full-page redirect when the
browser denies the popup for lack of user activation — the case an
unattended dashboard hits on an unattended reconnect. The redirect fallback
is what lets such dashboards self-heal without a human present.

**Rejected: encrypted refresh-token custody.** Storing an encrypted GitHub
refresh token (as sketched in the initial review) keeps sessions long-lived
without depending on the user's live GitHub browser session, but it commits
ATC to at-rest encryption, key rotation, per-session refresh serialization
against concurrent renewal races, and handling for refresh failure and
revocation — machinery this design has no other use for. The no-token
design is a strict runtime subset of the refresh-token design: ATC never
calls GitHub on the user's behalf after callback, because run/job data
already arrives via webhooks independent of any user token. The
refresh-token approach is preserved as an additive upgrade path, not
discarded outright, and can be layered on later without revisiting this
decision.

### 5. Opaque `__Host-` cookie sessions, hashed at rest in Postgres; `auth.github` requires Postgres

The browser credential is an opaque, random session identifier delivered as
a `__Host-`-prefixed cookie (`Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`,
no `Domain` attribute). Only the SHA-256 hash of that identifier is stored;
the raw value exists solely in the cookie. Sessions live in Postgres, and
`auth.mode = "github"` fails boot validation if configured against the
in-memory store.

**Rejected: JWTs or other frontend-managed tokens.** The browser's
WebSocket API cannot set arbitrary request headers, so a bearer token would
have to travel as a query parameter (which leaks into access logs, proxy
logs, and browser history) or be held in `localStorage`/`sessionStorage`
(readable by any successful XSS). A `HttpOnly` cookie is invisible to
page JavaScript and is attached automatically by the browser to both the
REST snapshot request and the WebSocket upgrade request, giving both read
rails the same credential transport for free. Requiring Postgres for
`auth.github` avoids building and maintaining two session-storage
implementations for a mode that, by decision 4, has no long-lived secret
material to protect beyond the session mapping itself.

### Accepted: the global-`seq` inference side channel

Filtered WebSocket clients still observe ATC's single, global `seq`
sequence; a client authorized for only some repositories can infer that
*something* happened on a repository outside its authorization set when it
sees a gap in `seq`, even though the event content itself is filtered out.
This is accepted, not mitigated. [ADR
0003](0003-state-cursor-contract-and-operator-policy.md) already places
`seq` contiguity outside the client-facing contract, and per-session
sequence remapping to close the side channel would break the
reconnect-to-any-replica cursor design that decision depends on.

## Consequences

### Positive

- Per-repository dashboard filtering becomes possible without any stored
  GitHub credential, eliminating an entire class of at-rest-secret risk
  (encryption keys, rotation, leak blast radius) that the initial review's
  refresh-token sketch would have introduced.
- Webhook ingestion — the highest-blast-radius surface for a design
  mistake, since it already handles unauthenticated public traffic — is
  provably unchanged by this initiative.
- Authorization keyed by immutable ID is stable across the repository
  lifecycle events (rename, transfer) that would otherwise silently break a
  string-keyed authorization set.

### Negative

- **Two-surface coverage is an operator-legible but real gap for
  non-public repositories.** A repository visible via webhooks but missing
  the login app installation (or vice versa) produces a confusing
  partial-visibility state that operator documentation must explain
  clearly. Public repositories are exempt from this gap (see decision 2's
  public-repo exception): (b) and (c) are independently verifiable via
  GitHub's own visibility model, without needing the requesting user to be
  a collaborator.
- **Public-repo visibility widens the blast radius of decision (c)
  deliberately.** Any authenticated ATC session can now see run/job data
  for any repository ATC ingests webhooks from and GitHub reports as
  public, regardless of whether that session's user has any GitHub access
  to it at all. This is the intended behavior — public means public — but
  is a real widening from "collaborator access implies visibility" to
  "GitHub-public implies visibility to every ATC user."
- **No stored refresh token means authorization can only stay fresh as long
  as the user's own GitHub browser session is alive.** A session whose
  `repo_auth_ttl` has lapsed while the user's GitHub session has also
  expired forces an interactive login, unlike a refresh-token design that
  could silently renew for the token's full lifetime. This tradeoff is
  judged acceptable because it affects a narrow scenario (both clocks
  lapsed simultaneously), and unattended-dashboard use is better served by
  no-auth mode behind a network boundary than by this mode's silent-hop
  ceiling.
- **The `seq` inference side channel is a known, accepted information
  leak** for filtered clients — bounded to "something changed," never
  content.

### Out of scope

- Storing GitHub refresh or access tokens at any point — see the deferred
  upgrade path linked in decision 4.
- Hiding the global-`seq` side channel.
- In-memory session storage for `auth.github` mode.
- General OIDC/SAML SSO, which stays proxy-delegated.
- Per-repository filtering for reverse-proxy auth mode.
- The annotation sweep of existing documentation describing "no built-in
  authentication" — that sweep is owned by the ticket that ships the
  behavior described, not by this ADR.

## Amendment (2026-07-05) — public-repo check drops Basic auth entirely

Decision 2 described checking `GET /repositories/{id}` using Basic auth with
the login app's own `client_id`/`client_secret`, for the higher 5,000/hr
OAuth-app rate ceiling instead of the 60/hr unauthenticated one. In
production this failed with a 401 on every single repo checked: that
Basic-auth rate-limit mechanism is documented as exclusive to classic OAuth
Apps, and the login app here is a GitHub App (the two-surface split in
decision 2 requires a scoped, installable app; its `client_id` uses the
GitHub-App-specific ID format). GitHub has no equivalent Basic-auth
mechanism for GitHub Apps — a syntactically valid `client_id`/`client_secret`
pair is simply not a credential GitHub recognizes on this call. The OAuth
code-exchange call (`POST /login/oauth/access_token`) still succeeds with
the same two values because it is a different, shared flow (the standard
user-to-server token exchange), unrelated to the Basic-auth rate-limit
boost.

Minting a JWT with the app's private key to obtain a per-installation access
token was considered and rejected: it authenticates correctly, but only for
repos the login app is installed on — which defeats the purpose of this
specific check, since decision 2 exists precisely to catch public repos the
app is *not* installed on. `GitHubClient::is_repo_public` now makes a fully
unauthenticated call, accepting the 60/hr-per-source-IP ceiling — acceptable
at the realistic scale of "repos ATC has run data for" (dozens, refreshed
once per `repo_auth_ttl`, not per request).

## Related

- Issue: [#234 — Native GitHub auth](https://github.com/bojanrajkovic/atc/issues/234)
  (the two issue comments this ADR synthesizes)
- [ADR 0003](0003-state-cursor-contract-and-operator-policy.md) — `seq`
  contiguity already out of contract, referenced by the accepted side
  channel above
