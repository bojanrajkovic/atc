# CDviz Deep Dive - Prior Art Research for ATC

## Overview

CDviz is an **open-source SDLC observability platform** built on the CDEvents standard. Version 1.1.0 (as of March 2026). It collects, stores, and visualizes software delivery events without polling. Unlike Apache DevLake (polling-based), CDviz uses a **push/event-driven model** where the same event stream powers both dashboards AND downstream automation via NATS/Kafka/HTTP.

**Repo**: https://github.com/cdviz-dev/cdviz (Apache 2.0 licensed)
**Website**: https://cdviz.dev/
**Commit**: 36134ede317327f9e6a677f328bf4052583afa59 (shallow clone depth 1)
**Status**: Actively maintained (0.32.0 dependency updates in latest commit, professional AGENTS.md documentation, CI/CD pipelines)

---

## 1. ARCHITECTURE

### Event Flow (Core Pattern)

```
Sources → cdviz-collector → Database (PostgreSQL/TimescaleDB) → Grafana Dashboards
```

**Key Design Decision**: Direct database access from Grafana (no API abstraction). Dashboards query PostgreSQL directly using full SQL power and TimescaleDB optimizations.

### Three Core Components

#### A. **cdviz-collector** (Separate Repository)
- **Purpose**: Flexible data pipeline that acquires, transforms, and forwards events
- **Pattern**: Push/event-driven (NOT polling)
- **Integrations**: GitHub, GitLab, Kubernetes, ArgoCD, custom webhooks
- **Output**: Routes events to PostgreSQL, ClickHouse, NATS, Kafka, HTTP
- **Standard**: CDEvents specification compliant
- **Location**: https://github.com/cdviz-dev/cdviz-collector (separate repo)

#### B. **cdviz-db** (PostgreSQL + TimescaleDB)
- **Primary Table**: `cdviz.cdevents_lake` (TimescaleDB hypertable)
- **Storage**: JSONB payload + extracted metadata columns
- **Partitioning**: Time-based (7-day chunks) + hash partitioning by subject
- **Retention**: Auto-delete after 13 months
- **Indexing**: Unique on context_id, GIN index on JSONB payload
- **Deduplication**: Unique constraint on context_id prevents duplicate events
- **Migration Tool**: golang-migrate (timestamp-based versioning, NOT Atlas)
- **Database Migrations**: `/cdviz-db/migrations/` (YYYYMMDDHHMM format)
  - Latest 3 migrations include: baseline, fix_testview, add_ticket_view

#### C. **cdviz-grafana** (TypeScript Dashboard Generator)
- **Framework**: Grafana Foundation SDK (type-safe dashboard generation)
- **Runtime**: Bun (NOT Node.js)
- **Language**: TypeScript source → compiled JSON dashboards
- **Custom Panels**: D3.js/Apache ECharts via volkovlabs-echarts-panel plugin
- **Browser Scripts**: Custom visualization code in `src/panels/browser_scripts/`
- **Code Generation**: Dashboard versioning auto-generated from git history or timestamps

### Supporting Components

- **cdviz-site**: VitePress 2.0 documentation site (Vue-based)
- **charts/**: Helm charts for Kubernetes deployment (OCI registry: ghcr.io/cdviz-dev/charts)
- **demos/**: Docker Compose and Kubernetes integration examples

### Database Schema Pattern

```sql
CREATE TABLE "cdviz"."cdevents_lake" (
  "imported_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  "timestamp" TIMESTAMP WITH TIME ZONE NOT NULL,
  "payload" JSONB NOT NULL,           -- Full CDEvent stored as JSON
  "subject" VARCHAR(100) NOT NULL,    -- Extracted: context.type subject
  "predicate" VARCHAR(100) NOT NULL,  -- Extracted: context.type predicate
  "version" INTEGER[3],               -- Semantic version array
  "context_id" VARCHAR(100) NOT NULL  -- Unique event identifier (dedup key)
);
```

---

## 2. EVENT COLLECTION & STANDARDS

### CDEvents Standard Compliance

CDviz is built entirely on **CDEvents** — a CD Foundation-backed standard for cloud-native delivery events.

- **Specification**: https://cdevents.dev/
- **Conformance**: cdevents-spec/ (git submodule in repo) with conformance test suites
- **Event Types Supported**: Pipeline, artifact, test, incident, ticket, service deployment, environment, etc.
- **Format**: All events stored in JSONB with full CDEvent schema

### Event Collection Pattern

**Push-based (NOT polling)**:
- Sources push events to collector via webhooks, direct SDK, or native integrations
- Collector transforms and forwards to multiple backends simultaneously
- Enables real-time dashboards and downstream automation

**Supported Sources** (via cdviz-collector):
- GitHub (webhook integration)
- GitLab (webhook integration)
- Kubernetes (native observer)
- ArgoCD (webhook)
- Custom webhooks (transformer pattern for webhook standards)
- CI/CD platforms via webhook transformers

**Routing Options**:
- PostgreSQL (primary: TimescaleDB hypertable)
- ClickHouse (alternative time-series DB)
- NATS, Kafka (event broker)
- HTTP webhooks (downstream services)

---

## 3. UI/UX & DASHBOARDS

### Grafana-Based Visualization

CDviz uses **Grafana dashboards** (not custom UI). Dashboard system is 100% programmatic:
- **Source**: TypeScript code in `dashboards_generator/src/dashboards/`
- **Compilation**: `mise run build` generates JSON
- **Output**: `/dashboards/*.json` files imported into Grafana
- **Never edit JSON directly**: Follow edit-source-regenerate workflow

### Dashboard Library (12 Generated Dashboards)

| Dashboard | Purpose |
|-----------|---------|
| `artifact_timeline` | Version deployment timeline across stages |
| `cdevents_activity` | Event activity monitoring and volume overview |
| `demo_service_deployed` | Service deployment tracking (demo) |
| `dora_metrics` | DORA metrics (deployment frequency, lead time, MTTR, change failure rate) |
| `incident_executions` | Incident lifecycle tracking |
| `pipelinerun_executions` | CI/CD pipeline run history |
| `sdlc_stack_size` | SDLC stack composition and size |
| `service_deployments` | Service deployment tracking |
| `taskrun_executions` | Individual task run tracking |
| `testcaserun_executions` | Test case run results |
| `testsuiterun_executions` | Test suite run results |
| `ticket_executions` | Ticket/issue lifecycle tracking |

### Query Pattern

All dashboards **query PostgreSQL directly** without API abstraction:
```sql
-- Example pattern used in dashboards
SELECT
  timestamp,
  payload->>'subject' as subject,
  payload->'data' as event_data
FROM cdviz.cdevents_lake
WHERE
  timestamp >= $__timeFrom()
  AND timestamp <= $__timeTo()
  AND subject = 'pipeline'
```

### Custom Panels

- **D3.js Visualizations**: Browser scripts in `src/panels/browser_scripts/`
- **Plugin**: volkovlabs-echarts-panel (Apache ECharts)
- **Responsive Design**: Handle container resize, real-time updates, accessibility

### Demo Environment

Live read-only demo available at: https://demo.cdviz.dev/grafana (no installation required)

Local demo via Docker Compose:
```bash
cd cdviz/demos/stack-compose
docker compose up
# Grafana at http://localhost:3000/d/demo_service_deployed/service3a-demo
```

---

## 4. FEATURES & CAPABILITIES

### Core Analytics

**Deployment Tracking**:
- Current application version status across environments
- Version correlation between deployed apps and observable runtime metrics
- Deployment frequency and frequency trends

**DORA Metrics** (Four Keys):
- **Deployment Frequency**: How often code is deployed
- **Lead Time**: Time from commit to production
- **MTTR (Mean Time to Recovery)**: Incident recovery speed
- **Change Failure Rate**: Percentage of deployments causing issues

**Pipeline Observability**:
- End-to-end deployment process duration
- CI/CD pipeline performance analytics
- Pipeline run history and execution tracking
- Task run tracking (individual pipeline steps)

**Test Analytics**:
- Test case and test suite execution results
- Test coverage and failure patterns

**Incident Management**:
- Incident lifecycle tracking
- Incident correlation with deployments
- Recovery metrics

**Artifact Management**:
- Artifact timeline and version tracking
- Package URL (PURL) format support
- Cross-environment artifact status

### Cross-Organization Capability

**Multi-tenant Design** (inferred from TimescaleDB partitioning):
- Hash partitioning by subject allows isolation
- JSONB payload supports arbitrary event metadata
- Direct SQL queries enable custom tenant filtering

---

## 5. TECH STACK

### Languages & Runtimes

| Component | Language | Runtime | Build |
|-----------|----------|---------|-------|
| cdviz-db | SQL (PostgreSQL) | N/A | golang-migrate |
| cdviz-grafana | TypeScript | Bun | Bun compiler |
| cdviz-site | Markdown/Vue | Bun/Node | VitePress 2.0 |
| cdviz-collector | (separate repo) | Not examined | N/A |

### Databases

- **Primary**: PostgreSQL 16+ with TimescaleDB extension
- **Alternative**: ClickHouse support (not deeply examined)
- **Hypertables**: Time-series partitioning with compression and retention

### Tools & Frameworks

| Tool | Purpose | Version |
|------|---------|---------|
| Grafana Foundation SDK | Type-safe dashboard generation | Latest |
| D3.js | Custom visualizations | Embedded in browser scripts |
| VitePress | Documentation site generator | 2.0 |
| TailwindCSS | Styling | 4.x with custom plugins |
| Bun | JavaScript runtime | Latest (not Node.js) |
| golang-migrate | Database migrations | Latest |
| sqruff | SQL linting | Latest |
| Helm | Kubernetes deployment | Charts in /charts |
| Docker | Containerization | Multi-platform builds |
| mise | Task runner/monorepo orchestration | 2025.10.6+ |

### Package Managers

- **Bun** (primary for TS/JS components)
- **pnpm** likely (inferred from modern Node project)

### Quality & CI/CD

- **Linters**: biome (TypeScript), sqruff (SQL)
- **Testing**: Bun test runner
- **CI Pipeline**: GitHub Actions (ci.yml, release.yml, update-deps.yml)
- **Dependency Updates**: updatecli for automated dependency management
- **Code Format**: Strict formatting via biome

---

## 6. ACTIVITY & MAINTENANCE STATUS

### Actively Maintained

**Evidence of Active Development**:
1. **Latest Commit** (March 21, 2026): "fix(deps): Bump cdviz-collector to 0.32.0" — recent dependency management
2. **Version**: 1.1.0 (semantic versioning followed)
3. **Professional Documentation**: Comprehensive AGENTS.md in every component with detailed workflows
4. **Dependency Management**: Automated updatecli + release-plz workflow
5. **CI/CD Automation**: Full pipeline including multi-platform Docker builds

### Code Quality Indicators

- **DCO (Contributor License Agreement)**: Required signed commits (git commit -s)
- **Structured Workflows**: mise monorepo with experimental_monorepo_root config
- **Comprehensive Testing**: CI runs for db, grafana, charts, demos
- **Architecture Decision Records (ADRs)**: /adr/ directory for rationale

### Community & Documentation

- **Official Website**: https://cdviz.dev/ with blog posts and tutorials
- **Blog Posts** (Found in cdviz-site/src/blog/):
  - "CDEvents in Action #3: Direct CI/CD Pipeline Integration"
  - "CDEvents in Action #4: Webhook Transformers and Passive Monitoring"
- **Getting Started Guide**: Detailed tutorial with Docker Compose
- **Installation Docs**: Helm charts, Docker Compose, Kubernetes deployment
- **GitHub Organization**: https://github.com/cdviz-dev (multiple repos)

### Release Cycle

- **release-prepare.yml** + **release.yml**: Automated release workflow with git-cliff changelog
- **Semantic Versioning**: Followed strictly (1.1.0)
- **Container Registry**: ghcr.io/cdviz-dev (public images for charts)

---

## 7. PRIOR ART IMPLICATIONS FOR ATC

### What CDviz Does Well (Learn From)

1. **Event-Driven Architecture**: Push-based collection is superior to polling for real-time visibility
2. **CDEvents Standard Compliance**: Building on open standard enables ecosystem interoperability
3. **Programmatic Dashboard Generation**: TypeScript-based dashboard definitions (source of truth) avoid manual JSON editing
4. **Direct DB Access Pattern**: Grafana querying PostgreSQL directly is simpler than API abstraction, enables SQL power
5. **TimescaleDB Choice**: Excellent for time-series event data with built-in partitioning and compression
6. **Modular Components**: Monorepo design with independent components (collector, db, grafana)
7. **Professional DevOps**: Helm charts, multiple deployment options (Docker Compose, Kubernetes), automated releases

### Potential Differences/Gaps for ATC

1. **Real-Time Automation**: CDviz mentions "event-driven SDLC backbone" (NATS/Kafka routing) but unclear how actively used vs. dashboards being primary
2. **Cross-Org Capability**: No evidence of multi-tenant SaaS hosting (appears self-hosted only)
3. **Custom Visualizations**: D3.js panels require browser script development (not drag-and-drop like some tools)
4. **Alternative Storage**: ClickHouse support mentioned but PostgreSQL is primary focus
5. **Incident Correlation**: Dashboards track incidents but unclear if correlates with deployments automatically

### ATC Design Advantages to Consider

- **Event Routing Beyond Dashboards**: Make automation trigger as first-class feature (not afterthought)
- **Multi-Tenant SaaS**: Build for hosted service if solving for broader audience
- **Custom Rules Engine**: Beyond event storage, enable custom logic (deployment approval gates, auto-rollback rules, etc.)
- **Integration-First**: Go deeper on webhook transformers and source integrations
- **Real-Time Collaboration**: Live dashboards with shared annotations, team communication

---

## 8. CRITICAL FILES & LOCATIONS

**Repository Structure** (as of commit 36134ede):

```
cdviz/
├── cdviz-db/                    # PostgreSQL + TimescaleDB
│   ├── Dockerfile               # Migration container
│   ├── migrations/              # golang-migrate files (YYYYMMDDHHMM_*.sql)
│   │   ├── 202601010000_baseline.up.sql
│   │   ├── 202602130000_fix_testview.up.sql
│   │   └── 202602161352_add_ticket_view.up.sql
│   ├── AGENTS.md                # Detailed database guidance
│   └── mise.toml                # Task config
├── cdviz-grafana/               # Dashboard generation
│   ├── dashboards_generator/    # TypeScript source
│   │   ├── src/
│   │   │   ├── index.ts         # Entry point
│   │   │   ├── dashboards/      # Dashboard definitions
│   │   │   ├── panels/          # Panel definitions
│   │   │   └── tools.ts         # Utilities
│   │   ├── package.json         # Dependencies
│   │   └── tsconfig.json
│   ├── dashboards/              # Generated JSON (DO NOT EDIT)
│   │   ├── artifact_timeline.json
│   │   ├── dora_metrics.json
│   │   ├── incident_executions.json
│   │   └── [9 more dashboard JSONs]
│   ├── AGENTS.md                # Detailed dashboard guidance
│   └── mise.toml
├── cdviz-site/                  # VitePress documentation
│   ├── src/
│   │   ├── index.md             # Homepage
│   │   ├── docs/
│   │   │   ├── index.md         # Platform overview
│   │   │   ├── architecture.md  # Architecture details
│   │   │   ├── getting-started.md
│   │   │   └── cdevents.md      # CDEvents info
│   │   └── blog/                # Blog posts
│   ├── AGENTS.md
│   └── mise.toml
├── charts/                      # Helm charts (OCI: ghcr.io/cdviz-dev/charts)
├── demos/                       # Integration examples
│   ├── stack-compose/           # Docker Compose demo
│   └── stack-k8s/               # Kubernetes demo
├── cdevents-spec/               # Git submodule (CDEvents spec)
├── adr/                         # Architecture Decision Records
├── .github/workflows/           # CI/CD
│   ├── ci.yml                   # Main CI pipeline
│   ├── release.yml              # Release automation
│   └── update-deps.yml          # Dependency updates
├── AGENTS.md                    # Monorepo guidance
├── README.md                    # Project overview
├── CONTRIBUTING.md              # Contribution guide
├── mise.toml                    # Monorepo config (experimental_monorepo_root = true)
└── VERSION                      # Current version: 1.1.0
```

### Key Configuration Files

- **mise.toml** (root): Monorepo orchestration, dependency paths
- **cdviz-db/AGENTS.md**: Database schema patterns, migration workflows
- **cdviz-grafana/AGENTS.md**: Dashboard generation, TypeScript patterns
- **README.md**: Architecture overview with diagram (CdvizArchitecture.svg)
- **.github/workflows/ci.yml**: Matrix CI for all components

---

## 9. LICENSING & COMPLIANCE

- **License**: Apache License 2.0
- **CLA**: Contributor License Agreement required (cla-assistant.io)
- **DCO**: Developer Certificate of Origin sign-off on all commits (git commit -s)
- **Compliance Page**: https://cdviz.dev/compliance

---

## Research Sources

- Official Site: https://cdviz.dev/
- Main Repository: https://github.com/cdviz-dev/cdviz
- Collector Repository: https://github.com/cdviz-dev/cdviz-collector
- Organization: https://github.com/cdviz-dev
- CDEvents Standard: https://cdevents.dev/
- Live Demo: https://demo.cdviz.dev/grafana
- Blog: CDEvents in Action series (episodes 3-4 on webhook transformers and CI/CD integration)

