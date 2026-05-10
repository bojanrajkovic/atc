# GitHub Action Dashboard - Prior Art Analysis

**Repository**: https://github.com/chriskinsman/github-action-dashboard @ 1531352
**Status**: No longer maintained (last commit: 2023-09-14)
**Version**: 1.6.0

## Executive Summary

github-action-dashboard is a self-hosted web dashboard for monitoring GitHub Actions across an organization or user account. It provides a single pane of glass showing workflow statuses, with real-time updates via webhooks and websockets. The project is well-structured but deliberately simple—focusing on the 80/20 rule for basic action monitoring.

---

## 1. Architecture Overview

### Data Flow

```
GitHub API (poll every 15 min)
    ↓
┌─────────────────────────────────────┐
│  Backend (Node.js Express)          │
│  - actions.js (orchestration)       │
│  - github.js (API client)           │
│  - routes.js (REST endpoints)       │
│  - runstatus.js (websocket manager) │
│  - webhooks.js (webhook handler)    │
└─────────────────────────────────────┘
    ↓
┌─────────────────────────────────────┐
│  Storage: In-memory (this._runs)    │
│  No persistence layer               │
└─────────────────────────────────────┘
    ↓ (Socket.IO)
┌─────────────────────────────────────┐
│  Frontend (Vue 2 + Vuetify)         │
│  - actiondashboard.vue              │
│  - Socket.IO client listener        │
└─────────────────────────────────────┘
```

### Polling Strategy (actions.js:16-24)

- **Initial load**: Runs on server startup
- **Scheduled refresh**: Every 15 minutes (900,000 ms)
- **Rate limit consideration**: Chosen to avoid GitHub API quota exhaustion
- **Re-entrancy protection**: Guards against overlapping refresh calls

```javascript
start() {
  this.refreshRuns();
  setInterval(this.refreshRuns, 1000 * 60 * 15);
}
```

### Webhook Integration (webhooks.js)

**Trigger**: `workflow_run` event
**Purpose**: Real-time updates without polling delay
**Architecture**:
- Optional (can run without webhook secret)
- Can run on same or different port than main app
- Uses `@octokit/webhooks` for signature verification
- Calls `mergeRuns()` immediately on event

**Event Handler** (webhooks.js:96-141):
- Receives webhook payload with workflow run data
- Fetches usage metrics (duration) via `getUsage()` API call
- Merges into in-memory run cache
- Triggers websocket broadcast to all clients

### WebSocket Real-Time Updates (runstatus.js)

**Technology**: Socket.IO 4.5.1
**Pattern**: Single client/broadcast model (problematic for scale)
**Flow**:
1. Server receives webhook or manual refresh
2. `updatedRun(run)` emits to connected socket
3. Client listener `updatedRun(run)` updates Vue data reactively

**Code** (runstatus.js:13-18):
```javascript
updatedRun(run) {
  if (this._client) {
    this._client.emit("updatedRun", run);
  }
}
```

**Limitation**: Only stores `this._client` (singular), meaning broadcast doesn't work properly with multiple clients.

---

## 2. UI/UX Analysis

### Layout (actiondashboard.vue)

**Component**: Single component dashboard
**Table Structure**: Vuetify v-data-table with 10 columns

**Columns**:
1. **Repository** - Repo name (sortable)
2. **Workflow** - Workflow name (linked to GitHub Actions page)
3. **Branch** - Branch name
4. **Status** - Color-coded chip (success=green, failure=red, in_progress/queued=yellow)
5. **Commit** - First 8 chars of SHA (linked to commit)
6. **Message** - Commit message (linked to run)
7. **Committer** - Commit author name
8. **Started** - Created timestamp (date + time, 12-hour format)
9. **Duration** - Run duration with smart formatting (hours/minutes/seconds)
10. **Actions** - Refresh button (single icon)

### Visual Design

- **Framework**: Vuetify 2.6.6 (Material Design)
- **Styling**: Blue header bar with white data table
- **Loading state**: Generic "Loading runs..." text
- **Sorting**: Default sorted by createdAt descending (most recent first)
- **Search**: Case-sensitive text search across fields
- **Pagination**: Disabled (shows all in single view)

### Information Density

- Horizontal scrolling required for full table (no responsive design visible)
- All runs from all repositories in single unsorted-by-repo list
- No grouping by repo, workflow, or branch
- No filtering beyond text search
- No drill-down details

### Screenshot Analysis

The UI shows:
- Simple search bar
- Two-row table with success statuses
- Clickable workflow and commit links
- Date/time formatting: "2021-10-08 9:26 AM"
- Duration display: "8m 50s", "11m 10s"
- Refresh icon visible on right

---

## 3. Feature Set

### Implemented Features

**Data Collection**:
- Fetch all repos in org/user (github.js:64-72)
- Fetch all workflows per repo (github.js:74-85)
- Fetch latest runs per workflow branch (actions.js:26-107)
- Fetch run duration/usage metrics (github.js:87-100)

**Display**:
- Table view of all runs
- Color-coded status chips
- Commit/workflow/run links to GitHub
- Search/filter capability
- Duration formatting (smart units)

**Interaction**:
- Manual refresh per workflow (actiondashboard.vue:146-153)
- Real-time updates via webhooks

**Configuration**:
- GitHub App authentication (with private key)
- Single org OR username support
- Webhook secret + port configuration
- Lookback days for historical data (default 7)

### NOT Implemented

**Notable Gaps**:
- Per-step progress tracking
- Runner information (node, OS, compute)
- Queue depth analysis
- Cross-organization support
- Persistence (restarts lose state)
- Historical trends/analytics
- Filtering by status, repo, or branch
- Multiple view layouts
- Alerts/notifications
- User authentication
- Multi-client websocket broadcast
- Pull request/branch filtering
- Retry information
- Logs aggregation

---

## 4. Technology Stack

### Backend

| Component | Technology | Version |
|-----------|-----------|---------|
| Runtime | Node.js | (via npm scripts) |
| Server | Express | 4.18.1 |
| GitHub API | @octokit/rest | 18.12.0 |
| GitHub Auth | @octokit/auth-app | 3.6.1 |
| Webhooks | @octokit/webhooks | 9.24.0 |
| Real-time | Socket.IO | 4.5.1 |
| Rate limiting | @octokit/plugin-throttling | 3.6.2 |
| Retry logic | @octokit/plugin-retry | 3.0.9 |
| Concurrency | p-limit | 3.1.0 |
| Date handling | dayjs | 1.11.2 |
| Utilities | lodash | 4.17.21 |
| Testing | Jest | 28.1.0 |

**Notes**:
- Uses GitHub App auth (JWT) instead of PAT
- Throttling + retry plugins for robust API handling
- p-limit (concurrency limiter) for controlling parallel API calls

### Frontend

| Component | Technology | Version |
|-----------|-----------|---------|
| Framework | Vue | 2.6.14 |
| UI Library | Vuetify | 2.6.6 |
| Socket.IO Client | socket.io-client | 4.5.1 |
| Socket.IO Vue | vue-socket.io-extended | 4.2.0 |
| HTTP Client | axios | 0.27.2 |
| Date handling | dayjs | 1.11.2 |
| Utilities | lodash-es | 4.17.21 |
| Build Tool | @vue/cli | 4.5.15 |

**Notes**:
- Uses Vue 2 (legacy, EOL in April 2024)
- Vuetify 2 (still maintained but major version behind)
- Direct socket emission listener pattern (not store-based)

### Deployment

- **Docker support**: Dockerfile included
- **Environment variables**: Extensive configuration via env vars
- **Port flexibility**: Main app + webhook can use separate ports

---

## 5. Key Implementation Details

### GitHub API Usage Patterns (github.js)

**Authentication**:
```javascript
const MyOctoKit = Octokit.plugin(throttling).plugin(retry);
this._octokit = new MyOctoKit({
  auth: { appId, privateKey, clientId, clientSecret, installationId },
  authStrategy: createAppAuth,
  throttle: { onRateLimit, onAbuseLimit }
});
```

**Rate Limit Handling** (github.js:35-51):
- Retries once on rate limit
- Logs abuse detection
- No exponential backoff, just single retry

**Data Fetching Pattern**:
- Uses `octokit.paginate()` for all list operations
- Concurrency limiting with p-limit (max 10 concurrent usage fetches)

### Run Status Merge Logic (actions.js:109-126)

```javascript
mergeRuns(runs) {
  runs.forEach((run) => {
    const index = _.findIndex(this._runs, {
      workflowId: run.workflowId,
      branch: run.branch,
    });
    if (index >= 0) {
      this._runs[index] = run;  // Replace
    } else {
      this._runs.push(run);     // Append
    }
    this._runStatus.updatedRun(run);  // Broadcast
  });
}
```

**Key**: Uses (workflowId, branch) tuple as identifier, not runId

### Frontend Socket Listener (actiondashboard.vue:62-71)

```javascript
sockets: {
  updatedRun(run) {
    const index = findIndex(this.runs, { workflowId: run.workflowId, branch: run.branch });
    if (index >= 0) {
      this.$set(this.runs, index, run);
    } else {
      this.runs.push(run);
    }
  }
}
```

Same merge logic on client side for consistency.

### REST API Endpoints (routes.js)

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/owner` | GET | Return configured org/username |
| `/api/initialData` | GET | Return in-memory run cache |
| `/api/runs/:owner/:repo/:workflow_id` | GET | Refresh specific workflow |

Refresh endpoint does not return data—relies on subsequent socket emission.

---

## 6. Limitations & Issues

### Architectural Limitations

1. **Single-client websocket**: `runstatus.js` only stores one `_client` reference, so multiple browser tabs won't work properly for real-time updates.

2. **In-memory only**: No persistence. Restarting loses all cached data until next 15-minute refresh.

3. **Single org/user**: Hard limit to one org OR one user account. No multi-org support.

4. **15-minute polling floor**: Chosen to avoid API quotas, but creates blind spots between webhook deliveries.

5. **No data ordering**: All runs flattened in single list. N workflows * N branches per repo create a long list.

6. **Missing row-level details**: Can't see step-by-step progress, runner info, or detailed logs without leaving the dashboard.

### Operational Limitations

1. **Webhook is optional but recommended**: Without webhook secret, only gets updates every 15 minutes.

2. **GitHub App setup is manual**: No auto-registration. Requires multiple UI steps to configure.

3. **Ngrok/inlets needed for local development**: Webhook testing requires external tunnel for non-public instances.

4. **No error recovery**: If API fails, just logs and continues. No circuit breaker or exponential backoff.

5. **No alerts/notifications**: No way to be notified of failures other than checking dashboard.

### Feature Gaps (for ATC consideration)

- **No step-level granularity**: Can't see which step failed or is currently running
- **No queue analysis**: Can't see actions waiting for runners
- **No runner metadata**: Node type, labels, available runners
- **No historical data**: No trends, no SLA tracking
- **No multi-repo grouping**: No way to view all workflows for a single repo
- **No branch filtering**: Showing all branches together
- **No retry information**: Can't track manual reruns
- **No audit logging**: No record of who refreshed what

---

## 7. Activity & Maintenance

### Repository State

| Metric | Value |
|--------|-------|
| Last commit | 2023-09-14 14:49 (Sept 14, 2023) |
| Commit count (shallow clone) | 1 (no history in shallow clone) |
| Status | **No Longer Supported** (per README) |
| Open issues/PRs | Unknown (likely abandoned) |

**From README.md**:
> "My team is no longer using this for monitoring our GitHub actions. Feel free to fork and improve!"

### Why It Was Abandoned

While not explicitly stated, likely reasons:
- Outgrown by larger teams (single org limitation problematic)
- 15-minute polling insufficient for fast-moving teams
- No step-level visibility
- Better commercial solutions emerged
- Feature gaps made it incomplete

---

## 8. Comparison Matrix for ATC

| Capability | github-action-dashboard | ATC Recommendation |
|-----------|-------------------------|-------------------|
| **Real-time updates** | Webhooks only (delayed without) | Should be primary |
| **Polling fallback** | 15 minutes | Consider 1-5 minutes |
| **Step visibility** | No | Critical feature |
| **Multi-org** | No | Essential |
| **Queue analysis** | No | Key differentiator |
| **Runner info** | No | Important context |
| **Persistence** | No | Optional but useful |
| **Multi-client** | Broken | Must fix |
| **Filtering/grouping** | Minimal (search only) | Rich (by repo, status, branch) |
| **Scalability** | ~50-100 workflows max | 100s to 1000s |
| **Maintenance** | Abandoned | Active development |

---

## 9. Code Quality & Testing

### Test Coverage

**Test files**:
- `tests/unit/actions.test.js` - Actions orchestration
- `tests/unit/runstatus.test.js` - WebSocket manager
- `tests/unit/webooks.test.js` - Webhook handling
- `tests/integration/github.test.js` - GitHub API integration

**Testing approach**: Jest with mocking

**Sample test** (actions.test.js:10-23):
```javascript
test("Actions - Start", () => {
  jest.useFakeTimers();
  const refreshRuns = jest.spyOn(actions, "refreshRuns").mockImplementation(() => {});
  actions.start();
  expect(refreshRuns.mock.calls.length).toBe(1);
  jest.advanceTimersByTime(1000 * 60 * 16);
  expect(refreshRuns.mock.calls.length).toBe(2);
});
```

### Code Organization

**Clean separation**:
- `github.js` - API client (no business logic)
- `actions.js` - Orchestration (polling, caching, merging)
- `webhooks.js` - Event handling
- `routes.js` - REST API
- `runstatus.js` - WebSocket management

**No complex interdependencies** - good for understanding flow

### Linting

- ESLint configured
- Babel parser for ES2020+ syntax
- Pre-commit linting in dev mode (`npm run serve`)

---

## 10. Lessons for ATC

### What to Adopt

1. **GitHub App authentication pattern** - More secure than PATs
2. **Webhook + polling hybrid** - Real-time with fallback
3. **Concurrency limiting** - Avoid API quota exhaustion
4. **Clean separation of concerns** - Easy to test and modify
5. **Socket.IO for real-time** - Battle-tested, simple
6. **Vuetify for fast UI** - Material Design out of the box

### What to Improve

1. **Multi-client websocket** - Implement proper broadcast (Socket.IO has `io.emit()`)
2. **Persistence layer** - Database or Redis cache
3. **Multi-organization** - Design for scale from start
4. **Step-level visibility** - Fetch workflow run jobs, not just runs
5. **Queue analysis** - Track pending vs. in-progress vs. complete
6. **Filtering/grouping** - Group by repo, status, branch
7. **Scalable polling** - Use cursor-based pagination, not list-all
8. **Error handling** - Circuit breaker, exponential backoff
9. **Frontend state management** - Vuex or Pinia instead of direct sockets
10. **Persistence across restarts** - Cache to DB, restore on startup

### What to Avoid

1. **In-memory only state** - Causes loss on restart
2. **Single point of failure** - No horizontal scalability
3. **Manual webhook setup** - Automate or provide clear UI flow
4. **15-minute polling floor** - Too slow for modern teams
5. **Abandoned codebase** - Maintain actively, document decisions
6. **Vue 2 / Vuetify 2** - Plan Vue 3 migration or use modern alternative

---

## 11. File Structure for Reference

```
github-action-dashboard/
├── index.js                 # Server entry point
├── configure.js             # Dependency injection setup
├── routes.js                # REST API routes (3 endpoints)
├── actions.js               # Core orchestration (polling + merging)
├── github.js                # GitHub API client wrapper
├── webhooks.js              # Webhook handler + server
├── runstatus.js             # WebSocket manager
├── package.json             # Backend dependencies
├── Dockerfile               # Docker build
├── client/
│   ├── package.json         # Frontend dependencies
│   ├── src/
│   │   ├── main.js          # Vue entrypoint
│   │   ├── App.vue          # Root component (shows title)
│   │   ├── components/
│   │   │   └── actiondashboard.vue  # Main dashboard table
│   │   └── plugins/         # Vuetify config
│   ├── public/              # Static assets
│   └── vue.config.js        # Vue CLI config
├── tests/
│   ├── unit/                # Jest unit tests
│   └── integration/         # Integration tests
└── docs/images/             # Screenshots
```

---

## Closing Assessment

**github-action-dashboard** is a **solid proof-of-concept** that demonstrates:
- ✅ Feasibility of GitHub Actions monitoring dashboards
- ✅ Webhook + polling architecture patterns
- ✅ Real-time websocket updates for monitoring
- ✅ Clean code organization for maintainability

However, it's **incomplete for production use** due to:
- ❌ Abandoned (no support since Sept 2023)
- ❌ Single org limitation (doesn't scale)
- ❌ Missing critical features (step visibility, queue analysis)
- ❌ In-memory state loss on restart
- ❌ Broken multi-client support

**For ATC, this is excellent prior art to learn from but not to fork.** You should build from scratch with its lessons applied, focusing on:
- Step-level workflow visibility (primary differentiator)
- Queue depth analysis (key for Kubernetes/runner scaling)
- Multi-organization support from day one
- Proper persistence and state management
- Active maintenance and documentation
