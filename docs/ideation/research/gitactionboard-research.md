# GitActionBoard Deep Dive

**Repository**: https://github.com/otto-de/gitactionboard @ `4d29ab6`
**Last Commit**: 2026-03-20 (actively maintained)

## Executive Summary

GitActionBoard is a production-ready GitHub Actions dashboard for monitoring workflow runs across multiple repositories. It uses **pull-based polling** (every 5 seconds) rather than webhooks. Built with Spring Boot 3 (Java 21) on backend and Vue 3 with Vuetify on frontend.

---

## 1. Architecture: Data Fetching Strategy

### Primary Approach: Polling (Pull-Based)
- **Frontend**: JavaScript polls backend every **5 seconds** via `setInterval(this.renderPage, 5000)`
  - Source: `/frontend/src/components/Dashboard.vue:157`
- **Backend**: Makes synchronous REST calls to GitHub API on-demand
  - No webhooks; relies on REST API exclusively
  - Caching layer prevents rate-limiting issues

### Data Flow
1. **Frontend** → `fetchCctrayJson()` → Backend `/v1/cctray` endpoint
2. **Backend** → `PipelineService.fetchJobs()` (cached for 60 seconds default)
3. **PipelineService**:
   - Fetches workflows for each repo (parallel)
   - For each workflow, fetches the **last 2 runs** via GitHub API
   - Fetches job details for current & previous runs
   - Compares current vs previous to show build status changes
4. **GitHub API calls**:
   - `GET /repos/{owner}/actions/workflows` - List workflows
   - `GET /repos/{owner}/actions/workflows/{workflow_id}/runs?per_page=2` - Get last 2 runs
   - `GET /repos/{owner}/actions/runs/{run_id}/jobs` - Get job details for a run

### Optional: Periodic Scanning for Security Alerts
- Cron-based scheduler: `@Scheduled(cron = "${PERIODIC_SCAN_CRON_SCHEDULE}")`
  - Source: `/backend/src/main/java/de/otto/platform/gitactionboard/adapters/service/PeriodicScanScheduler.java:36`
- Fetches secrets & code scan alerts separately if enabled
- Not real-time; runs on configured schedule

---

## 2. UI/UX: Dashboard Layout & Visual Design

### Responsive Grid Layout
- **Build Monitor View**: Compact 200px cards (configurable)
  - Source: `/frontend/src/components/Dashboard.vue:232`
- **Individual View**: Larger 300px cards
  - Toggle between views in preferences

### Grid Cell (Job Status Card)
- **Status indicators**: Color-coded backgrounds
  - Green (#3a964a): Success
  - Red (#e23d2c): Failure
  - Gray (#6d6a6a): Unknown
  - Source: `/frontend/src/components/GridCell.vue:170-182`
- **Content density**:
  - Job name (bold, 14px, truncated)
  - Relative time chip (e.g., "2m ago")
  - Progress bar (orange striped) when in progress
  - Hover: Reveals GitHub link + visibility toggle
- **In-progress indicator**: Striped orange bar; height varies by view (15px monitor, 10px individual)

### Dashboard Features
- **Hide healthy builds**: Filter out "Success" status
- **Show/hide specific workflows**: Per-workflow visibility toggle
- **Idle timeout optimization**: Stops polling after N minutes of inactivity (default: 5 min)
  - Configurable via `maxIdleTime` preference
- **Light/Dark theme toggle** (Vuetify)
- **Relative time formatting**: "2 minutes ago", "1 hour ago", etc.

### Multi-Tab Navigation
Routes: `/workflow-jobs`, `/secrets`, `/code-standard-violations`, `/metrics`, `/preferences`
- Source: `/frontend/src/router/index.js`

---

## 3. Features: Comprehensive Monitoring Capability

### Workflow & Job Monitoring
- ✓ Per-job status (success, failure, in-progress)
- ✓ Run number tracking
- ✓ Triggered event type (push, pull_request, schedule, etc.)
- ✓ Branch name filtering
- ✓ Run attempt tracking (retry count)
- ✓ Comparison with previous run (shows if build got better/worse)
- ✓ Job execution times (started_at, completed_at)

### Security & Code Quality Monitoring
- ✓ **Secret scanning alerts**: List exposed secrets with creation date
- ✓ **Code scanning alerts**: Standard violations with severity & line numbers
- ✓ Both available via separate endpoints: `/v1/alerts/secrets`, `/v1/alerts/code-standard-violations`

### Metrics & Reporting
- ✓ **Workflow reliability metrics**: Success rates, execution times
- ✓ **Time-range filtering**: Date range picker for metrics (per repository)
- ✓ Chart.js integration for visualizations
- ✓ Endpoint: `/v1/metrics/workflow-runs/{repoName}?from=&to=`

### API Compatibility
- ✓ **CCTray format** (XML & JSON): Industry-standard build monitor format
  - `/v1/cctray.xml` - CI/CD dashboard integrations
  - `/v1/cctray` - JSON for custom integrations
  - Sample: `name="repo :: workflow :: job"`, `activity="Sleeping|Building"`, `lastBuildStatus="Success|Failure"`

### Cross-Org Support
- ✓ Configurable owner: `REPO_OWNER_NAME` env var (org or username)
- ✓ Multiple repos: `REPO_NAMES` comma-separated list
- ✓ Works with public & private repos (requires GitHub Personal Access Token)

---

## 4. Limitations: What's Missing vs Real-Time Webhook Approach

### Polling-Based Drawbacks
1. **Latency**: Up to 5 seconds + backend cache expiry (60s default) = ~65 seconds delay
   - Webhook-based would be near-instant (<1s)
2. **GitHub API rate limits**: Each poll hits GitHub API
   - Mitigated by caching but still a constraint at scale
   - 1,000 requests/hour per user for public repos
3. **No event stream**: Can't see intermediate states (e.g., job queued vs running)
   - Only sees "completed" or "in-progress"; no step-level granularity
4. **Comparison logic is naive**: Always fetches last 2 runs
   - If a run is still in-progress, shows as "unknown" until complete
5. **No retry/backoff**: Simple synchronous API calls; fails if GitHub is slow

### Missing Real-Time Capabilities
1. **Step-level progress**: Shows job status, not individual step completion
   - Source: Job model doesn't store step data
2. **Queue depth**: No information on queued/pending runs
3. **Runner info**: No tracking of which runner executed a job
4. **Live logs**: No streaming of job logs
5. **Webhook delivery**: No push notifications to external systems (except MS Teams failures)
6. **Cross-org aggregation**: Must be configured server-side; no dynamic repo discovery

### Caching Trade-offs
- Default 60-second cache means same data served to all users
- Job details cache (12 hours) holds completed runs; not invalidated on new runs
- Only re-fetches if job is incomplete (`anyJobNotCompleted()` check)
  - Source: `/backend/src/main/java/de/otto/platform/gitactionboard/adapters/service/job/GithubJobDetailsService.java:212`

---

## 5. Tech Stack

### Backend
- **Framework**: Spring Boot 3.5.5 (Java 21)
- **Build**: Gradle
- **Cache**: Caffeine (in-memory cache with TTL)
- **HTTP Client**: Apache HttpClient5
- **Auth**: Spring Security + OAuth2 + Basic Auth
- **Database**: SQLite (lightweight, file-based)
- **Notifications**: MS Teams webhooks
- **Testing**: JUnit 5, MockServer, ArchUnit, Pitest
- **Code Quality**: SpotBugs, SonarQube via Gradle

### Frontend
- **Framework**: Vue 3 (composition API)
- **Router**: Vue Router 4
- **UI Library**: Vuetify 3 (Material Design)
- **Charts**: Chart.js + vue-chartjs
- **Build**: Vite 7
- **Testing**: Vitest + @vue/test-utils
- **Linting**: ESLint + Stylelint

### Infrastructure
- **Containerization**: Docker (Alpine, multi-stage build with jlink)
- **Image size optimization**: Custom JRE via jlink (21-alpine base)
- **CI/CD**: GitHub Actions (builds, Trivy security scans, CodeQL)

---

## 6. Activity & Maintenance

- **Last commit**: 2026-03-20 (dependency updates)
- **Commit frequency**: Regular dependabot updates & feature development
- **Maintainers**: Team at Otto (otto-de org)
- **Contributors**: 15+ active, well-documented contribution process
- **OSS Lifecycle**: Active (not archived/deprecated)

---

## 7. Key Design Decisions for ATC Differentiation

### What GitActionBoard Does Well
1. **Low operational overhead**: No external dependencies, runs in one container
2. **Workflow-level granularity**: Shows job status for each workflow step
3. **Multi-repo support**: Single dashboard for org-wide visibility
4. **Standards-based output**: CCTray format allows integration with other tools

### Where ATC Could Differentiate
1. **Real-time webhooks**: Replace 5s polling with GitHub webhooks (instant updates)
2. **Step-level progress**: Show individual workflow step statuses & runtimes
3. **Queue visibility**: Track queued, pending, & running jobs separately
4. **Runner insights**: Show which runner (machine/self-hosted) executed jobs
5. **Live log streaming**: Stream job logs instead of linking to GitHub
6. **Cross-org federation**: Discover repos dynamically from multiple orgs/teams
7. **Traffic shaping**: Show job queue depth, runner capacity, estimated wait time
8. **Advanced filtering**: Branch patterns, workflow tags, failure types
9. **Alerting**: Smart notifications (e.g., "60 jobs queued, normal: 5")
10. **Historical trends**: Visualize runner utilization, queue wait times over time

---

## References

- Backend entry: `/backend/src/main/java/de/otto/platform/gitactionboard/Application.java`
- Polling interval: `/frontend/src/components/Dashboard.vue:157`
- Data fetch logic: `/backend/src/main/java/de/otto/platform/gitactionboard/domain/service/PipelineService.java`
- API endpoints: `/backend/src/main/java/de/otto/platform/gitactionboard/adapters/controller/GithubController.java`
- Job details: `/backend/src/main/java/de/otto/platform/gitactionboard/adapters/service/job/GithubJobDetailsService.java`
- UI components: `/frontend/src/components/Dashboard.vue`, `/GridCell.vue`
- Cache config: `/backend/src/main/java/de/otto/platform/gitactionboard/config/CacheConfig.java`
