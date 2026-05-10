# ATC — Actions Traffic Control: Architecture

## API Surface

### What Works
- **REST API** has everything needed:
  - `GET /repos/{owner}/{repo}/actions/runs?status=in_progress` — running/queued workflows
  - `GET /repos/{owner}/{repo}/actions/runs/{run_id}/jobs` — jobs with runner
    name, runner ID, labels, and per-step status with completion state
  - `GET /orgs/{org}/actions/runners` — self-hosted runner status
- **GraphQL API** is useful for listing orgs → repos (one query), but has NO
  workflow run support. `CheckRun`/`CheckSuite` types exist but don't include
  runner info or map cleanly to Actions workflows.

### What Doesn't Work
- No org-wide `/orgs/{org}/actions/runs` endpoint — must query per-repo
- GraphQL can't replace REST for Actions data
- Pure polling from the SPA would burn through rate limits fast (~50 repos at
  10s intervals = 5000 requests in ~17 minutes)

## Webhook Backend + SPA

Pure SPA polling is impractical due to rate limits.

### Deployment

1. **Register a GitHub OAuth App** — minimal scopes (`read:org`, optionally
   `repo` for private repo visibility). 2 minutes.
2. **Deploy the binary** — a single Rust binary that serves the SPA static
   files, receives webhooks, handles OAuth, and pushes state over
   websocket/SSE. Configure with OAuth App client ID/secret + webhook
   shared secret.
3. **Add an org webhook** — GitHub org Settings → Webhooks, point at the
   binary's URL, set a shared secret, subscribe to `workflow_run` +
   `workflow_job` events. 5 minute setup per org.
4. **Users sign in with GitHub** — device flow, backend verifies access,
   each user sees only their orgs/repos.

For local dev, `--no-auth` flag skips OAuth and shows all events.

### Event Flow

- `workflow_run` webhook fires on: `requested`, `in_progress`, `completed`
- `workflow_job` webhook fires on: `queued`, `in_progress`, `completed`
  (includes runner name, runner ID, labels, step data)
- Backend maintains current in-flight state in memory
- SPA connects via websocket/SSE, gets real-time updates pushed
- State builds up from webhook events as they arrive; on backend restart,
  state refills naturally as new events come in (in-flight runs at restart
  time are missed but catch up within minutes)

### SPA Frontend

- Connects to backend websocket, renders what it receives
- Handles GitHub OAuth device flow for sign-in, sends token to backend
  for verification
- Dashboard with queued/running/completed views, runner assignments,
  per-step progress

## Auth

Per-user access control with protection against enumeration:

1. Register a GitHub OAuth App with minimal scopes:
   - `read:org` — verify org membership
   - `repo` — see private repos (optional, skip for public-only)
   - No code access, no push, no settings — read-only identity
2. SPA does the OAuth device flow, gets a user token in the browser
3. SPA sends the token to the backend **once**
4. Backend calls `GET /user/orgs` + `GET /user/repos` to verify
   what the user can actually see
5. Backend stores the verified access list, **immediately discards
   the token** — never persisted, in memory for milliseconds
6. Backend tags the websocket session with the verified access list
   and filters events accordingly
7. Each user only sees orgs/repos they have verified access to

**Why the backend verifies:** Prevents enumeration. If the SPA just
sent a list of org/repo names it *claims* to have access to, a
malicious user could subscribe to any org. Backend verification with
a real GitHub token closes that hole.

**Trust model:** Same as any "Sign in with GitHub" integration. The
token touches backend memory for one API call and is dropped. The
OAuth App's scopes are read-only identity — it can never read code
or modify anything.

This makes it hostable as a service — one instance, many orgs, each
user sees only their stuff. Works for:
- **Self-hosted**: team deploys one instance, multiple orgs add webhooks
- **SaaS**: hosted instance, orgs onboard by adding a webhook + users
  sign in with GitHub

For local dev, `--no-auth` flag skips OAuth and shows all events.

## Storage

Backend should use a pluggable database interface. SQLite for local/small
deployments, Postgres for scale. State is mostly ephemeral (in-flight runs)
but persistence helps with restart resilience and historical views.

## Webhook Gaps

If the backend is down when webhooks fire, state will have gaps. Not worth
trying to backfill via the API — the syncing complexity outweighs the
benefit. State catches up naturally as new events arrive.

## Session Lifecycle

When a user's SSE/websocket connection drops, the backend forgets about
them entirely — no session persistence. On reconnect, the SPA re-does the
OAuth device flow and the backend re-verifies access. Simple, stateless.
