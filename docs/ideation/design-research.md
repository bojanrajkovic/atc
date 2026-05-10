# ATC — Actions Traffic Control: Design

## Prior Art (March 2026)

Detailed research in `research/` directory.

### Existing Tools — None Do What ATC Would Do

| Tool | Real-time | Steps | Runners/Queue | Status |
|------|-----------|-------|---------------|--------|
| GitactionBoard (otto-de) | No (65s polling) | No | No | Active |
| github-action-dashboard | Partial (broken WS) | No | No | Abandoned |
| Meercode | No (polling) | No | No | Stalling |
| Foresight (Thundra) | Yes | Yes | No | Dead (acquired) |
| CDviz | Yes (push) | Yes | No | Active (different focus) |

Every existing tool is either polling-based (slow, rate-limited), dead,
or doesn't show runner/queue info. ATC's combination of real-time webhooks
+ per-step progress + runner/queue visibility is unserved.

## UI Design Principles

### From Concourse CI
- Pulsating halos for running state (visible across room)
- "Information ruthlessness" — strip to what helps triage
- One-click drill-down from overview to failure

### From ATC Simulators (70+ years of human factors research)
- Data blocks: identity → status → progress (max 5 lines)
- Color + symbol duality (never color alone — accessibility)
- Progressive disclosure: icon-only → labels → full detail
- Runner availability as runway capacity (green/yellow/red)
- Smooth state transitions (300-400ms animations)
- Both "planner view" (zoomed out) and "tactical view" (zoomed in)

### From Meercode
- Shareable public dashboard links (no auth for stakeholders)

### From github-action-dashboard (lessons learned)
- GitHub App auth pattern works well
- Webhook + polling hybrid for reliability
- Fix: proper multi-client websocket broadcast
- Fix: need persistence layer (in-memory = restart loses everything)

## Visual Language

### Status States — Color + Symbol Duality

| Status | Color | Symbol | Animation |
|--------|-------|--------|-----------|
| Queued | Blue `#4a9eff` | ◐ | None |
| Running | Amber `#e8b820` | ▶ | Pulsating halo |
| Success | Green `#2ecc71` | ✓ | None |
| Failed | Red `#e74c3c` | ✗ | None |
| Cancelled | Gray `#6a7588` | ⊘ | None |

### Job Data Block

```
┌─────────────────────────────────────────┐
│ JOB-NAME                    ▶ 03:45     │  ← identity + status
│ repo/workflow • branch                  │  ← context
│ Step 5/12 ████████░░░░  runner-name     │  ← progress + runner
└─────────────────────────────────────────┘
```

### Runner Status ("Runway Status")

Shows queue depth per label set, not capacity — most users run dynamic
runners (ARC, sand) where capacity is elastic. What matters is how many
jobs are running vs. waiting.

```
self-hosted/linux    4 running, 0 queued
self-hosted/macos    1 running, 2 queued ⚠
github-hosted        2 running
```

Warning indicator on non-zero queue depth. Runner names still shown on
individual job cards for debugging ("why is this slow?").

### Layout

Single layout: **Kanban / Radar** — columns for Queued | Running | Completed.
Maps directly to the workflow lifecycle and active monitoring/triage workflow.
Preferred layout from playground evaluation.

### Design Decisions (from critique review)

Resolved before implementation:

1. **Failures sort to top of Completed** — failed cards appear above successes
   and cancelled jobs. Stronger visual treatment: wider left accent bar or
   subtle red-tinted background. Browser notifications already handle alerting;
   the dashboard just needs failures to be easy to spot, not alarming.

2. **Completed column has a TTL / limit** — completed jobs age out of the
   backend's in-flight state. The UI renders what the backend sends. Decide
   retention policy during backend implementation (e.g., 30 min TTL, or max 20
   completed per repo). This is an architecture + UI concern together.

3. **Cards link to GitHub Actions run** — each card is a clickable link to the
   GitHub Actions run URL (included in the webhook payload). "See a problem →
   investigate" should be one click. Matches the Concourse "one-click
   drill-down" principle.

4. **Runner pool labels on queued cards** — each queued card shows a small chip
   (e.g., `linux`, `macos/arm64`) indicating which runner pool it's waiting
   for. Always visible, no interaction needed.

5. **Runner bar click-to-filter** — clicking a runner pool in the status bar
   filters/highlights the kanban to show only jobs targeting that pool. For
   investigating "why is my macOS job stuck?" without visual correlation.

6. **Replace `⊞` runner icon** — the current icon has no semantic association.
   Use something more meaningful or drop the icon entirely (the monospace runner
   name is already visually distinct).

7. **Test `⊘` vs `◐` symbols at small sizes** — cancelled and queued
   symbols may be confused at small sizes. Consider more distinct glyphs if
   testing confirms this.

8. **Dim completed successes** — successes in the Completed column are
   de-emphasized: reduced opacity, or auto-collapsed to compact mode. Failures
   stay expanded and prominent. Successes are confirmation, not information.

9. **Repo/org filtering** — a filter bar to scope the kanban by repo or org.
   Essential for the cross-org promise. Pattern matches the runner bar
   click-to-filter (#5). Could be a dropdown, tag filter, or quick-toggle
   chips.

10. **Thicker left accent bar** — increase from 3px to 4–5px so it reads as a
    deliberate color indicator rather than a border artifact. Valuable for
    at-a-glance state. Keep it.

**Explicitly not goals for v1:**
- System-wide "all green" vs "something red" color shift (Concourse-style).
  ATC shows state, it doesn't alarm. Browser notifications handle alerting.
- Click-to-expand cards with full step detail. The GitHub link is the
  drill-down path. Inline step expansion is a future feature.

### Progressive Disclosure

- **Compact**: Icon + name + duration only
- **Expanded** (default): Full data block with step progress, runner,
  repo/branch context

### Animation Notes (for real SPA)

The playground rebuilds the DOM every second (innerHTML), so card entrance/exit
animations aren't possible. When building the real SPA:

- **Use keyed lists with transition groups** (e.g., Solid's `<TransitionGroup>`,
  Preact/React's `react-transition-group`, or Vue's `<TransitionGroup>`) so
  cards animate in/out as jobs change status and move between columns.
- Cards entering a column should fade + slide in from the top (150ms ease-out-expo).
- Cards leaving should fade out faster (~100ms).
- Cards moving between columns (queued → running) should animate position via
  FLIP (First, Last, Invert, Play) for a smooth cross-column slide.
- The 1-second duration tick should only update the duration text in-place, not
  rebuild the entire DOM — use incremental/reactive updates.

### Performance Notes (for real SPA)

From the audit of the playground prototype — things to get right in the real build:

- **No `transition: all`** — always scope transitions to the specific properties
  that change (e.g., `transition: box-shadow 250ms, border-color 250ms`).
  `transition: all` forces the browser to check every animatable property on
  every frame. With 11+ cards ticking every second, this compounds.
- **Progress bars use `transform: scaleX()`**, not `width` — already done in
  the playground. `scaleX` is GPU-composited (no layout/paint), while `width`
  triggers layout recalculation on every frame.
- **Avoid full DOM rebuilds on tick** — the playground uses `innerHTML` to
  rebuild all cards every second. The real SPA must use a reactive framework's
  diffing (Solid's signals, Preact's VDOM, or vanilla keyed-DOM) so that the
  1-second duration tick only touches the text node of running jobs' duration
  counters, not the entire card tree.
- **Virtual scrolling if job count grows** — with 10-20 jobs the kanban is fine,
  but if an org has 50+ concurrent jobs, the completed column could grow large.
  Consider windowed rendering (e.g., `@tanstack/virtual`) for columns with >30
  items, or aggressively cull completed jobs after a TTL.
- **WebSocket message batching** — in the real backend, webhook events may arrive
  in bursts (e.g., a workflow with 8 jobs all starting at once). Batch incoming
  WS messages and apply them in a single RAF frame, rather than re-rendering
  per-message.

### Responsive Notes (for real SPA)

The playground has a controls panel — that's playground-only. The real SPA
is just the dashboard: a top bar (logo, theme/density toggles, runner status)
and the kanban below. No sidebar.

- **Top bar** — logo, theme picker (dropdown), dark/light toggle, compact/expanded
  toggle, runner status summary. All fits in one row. On narrow screens, runner
  status can collapse to a summary ("3 running, 1 queued") that expands on tap.
- **Kanban columns** — on mobile (< 640px), stack columns vertically or use
  horizontal swipe between Queued / Running / Completed tabs. The three-column
  layout needs ~900px minimum to be usable.
- **Job cards** — the expanded card works down to ~280px column width. Below
  that, auto-switch to compact mode.
- **Touch targets** — ensure 44x44px minimum touch targets on mobile for all
  interactive elements.
- **Text scaling** — the `rem`-based type scale already respects user font-size
  preferences, but test at 200% zoom to verify no overflow/clipping.

## Interactive Playground

`playground.html` — interactive HTML explorer for trying different
visual treatments. Also published as a gist:
https://gist.github.com/bojanrajkovic/3e9aa4de8a4051c63e5d41a4620d79d0

Includes 2 presets: Radar, Minimal.

## Browser Notifications

Trivial to implement with HTTPS — `Notification.requestPermission()` +
`new Notification()`. The websocket already has the events. Examples:
- "CI failed on loupe-app/loupe"
- "Job queued for 10m with no matching runner"

Click-to-open links to the GitHub Actions run. Works on desktop and mobile.
Service Worker push notifications (slightly more work) would work even
when the tab is closed.

## Future: Historical Analytics

With the pluggable DB storing events, historical analytics come for free:
- Average queue wait time
- Slowest workflows
- Runner utilization over time
- DORA-lite metrics as a byproduct

Not in scope for v1 but the architecture supports it.
