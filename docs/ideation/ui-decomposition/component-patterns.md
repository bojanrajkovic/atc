# Component Decomposition Patterns for ATC Dashboard

Research into UI component patterns for real-time CI/CD dashboards, synthesized from design systems (PatternFly, Carbon), industry articles (Smashing Magazine), kanban library architectures (react-trello, react-kanban, SVAR Svelte Kanban), existing GitHub Actions dashboards, and the ATC playground prototype.

---

## Dashboard Layout Patterns

### Chrome / Shell Pattern

The outermost layer of a dashboard is the **application shell** — the persistent frame that wraps all content areas. For ATC, the prototype establishes this as:

- **TopBar** (or RunnerBar in ATC) — a persistent horizontal bar showing system-level status (runner pool utilization). This is the "dashbar" pattern from PatternFly: a condensed summary appearing at page-top showing critical system state within the upper half-screen area.
- **Content Area** — the main kanban board fills the remaining viewport.
- **No sidebar in production** — the prototype's left controls panel is for design exploration only. The production app should use a minimal top bar for theme/mode switching, not a sidebar.

**Component boundaries:**
```
AppShell
  TopBar (runner status, theme controls, connection indicator)
  MainContent
    KanbanBoard (the primary content)
  [Optional] DetailPanel (slide-over for expanded run detail)
```

**Key insight from Smashing Magazine:** Arrange elements by decision importance, not data volume. Primary KPIs (runner saturation, failure count) get center/top placement with bold typography. Supporting metrics get secondary areas. Controls and navigation go to edges with minimal visual weight.

### Responsive Strategy

PatternFly recommends a column-based grid that collapses progressively:
- **Desktop (>1200px):** 3-column kanban with full runner bar
- **Tablet (768-1200px):** 3-column kanban with condensed runner bar (icons + counts, no labels)
- **Mobile (<768px):** Stacked single-column with tab navigation between Queued/Running/Completed
- **Wall display:** Same as desktop but with larger type scale and compact card mode for density

For ATC, the primary use case is desktop monitoring, with wall display as secondary. Mobile is deprioritized but should degrade gracefully.

---

## Kanban / Board Patterns

### Column-Based Board Architecture

Every kanban implementation (Trello, Linear, GitHub Projects, react-trello, SVAR Svelte Kanban) decomposes the board identically:

```
Board
  Column (one per status: Queued, In Progress, Completed)
    ColumnHeader (status label + count badge)
    CardList (scrollable container)
      Card (one per workflow run)
```

**Critical distinction for ATC:** Unlike Trello or Linear where users drag cards between columns, ATC is a **read-only monitoring board** where cards move between columns based on server-side state changes. This fundamentally changes the component design:

- No drag-and-drop handlers needed
- Cards move between columns via **animated list transitions** triggered by data changes, not user gestures
- The "movement" is a visual representation of a WebSocket event, not a user action

### Column Design

From the ATC prototype, each column has:
- **ColumnHeader:** Uppercase label + count badge in a pill (`<span class="count">`)
- **Status accent:** A 2px bottom border on the header, colored by status
- **Scrollable card list:** `overflow-y: auto` with flex column layout and consistent gap

**Data flow:** The Board component receives all workflow runs from a store. It filters runs by status and passes them to the appropriate Column. Each Column receives only its filtered array.

```
Store (all runs) --> Board --> filter by status --> Column[status] --> Card[]
```

### Linear's Approach

Linear's board layout shows issues as cards in status columns. Key patterns:
- Status columns map 1:1 to workflow states (exactly like ATC's Queued/Running/Completed)
- Column headers show count badges
- Cards are compact by default with progressive disclosure on click
- Board and list are interchangeable views of the same data — the view mode is a UI concern, not a data concern

---

## Card Component Patterns

### Anatomy of a Status Card

From the ATC prototype and PatternFly's card patterns, a workflow run card decomposes as:

```
Card (article element, semantic)
  StatusAccentBar (3px left border, colored by status)
  CardHeader
    StatusIcon (color + symbol: queued=blue circle, running=amber play, etc.)
    RunName (primary text, truncated with ellipsis)
    Duration (mono font, tabular-nums, right-aligned)
  CardMeta (repo + branch, secondary text, truncated)
  CardProgress
    StepLabel ("Step 3 of 8")
    ProgressBar (scaleX transform animation)
    StepName (current step name)
  CardRunner (runner hostname, mono font)
```

### Compact vs. Expanded (Progressive Disclosure)

The ATC prototype implements two density modes as a global toggle:

**Compact mode** hides: CardMeta, CardProgress, CardRunner. Shows only: StatusIcon + RunName + Duration. This is the "scanning" mode — maximum information density for quick triage.

**Expanded mode** shows everything. This is the "debugging" mode — when you need to know which step is failing, what runner it's on, what branch triggered it.

**Smashing Magazine recommendation:** Use progressive disclosure — show the minimum by default, reveal detail on demand. For ATC, this means:
- Default to compact in columns with many items
- Auto-expand the focused/selected card
- Allow global density toggle (as prototype does)
- Consider per-card expansion (click to expand one card without affecting others)

### Overlay vs. Inline Expansion

Two expansion patterns exist:
1. **Inline expansion** — card grows in place, pushes siblings down. Better for responsive design and maintaining spatial context.
2. **Overlay expansion** — card reveals detail in a panel/drawer without changing board layout. Prevents layout shift in dense grids.

**Recommendation for ATC:** Use inline expansion for compact-to-expanded toggle (global density). Use a **slide-over detail panel** (overlay) for deep-dive into a specific run (clicking a card opens full job/step details in a side panel, like Linear's issue detail view).

### Status Indicators

Carbon Design System and the ATC design system both mandate:
- **Never color alone** — every status has a unique color AND a unique symbol
- ATC's symbol set: Queued (blue, half-circle), Running (amber, play triangle), Success (green, checkmark), Failed (red, X), Cancelled (gray, circle-slash)
- Icons are 1.25em, flex-shrink: 0, center-aligned in a fixed-width container for column alignment
- Running status gets a **pulsating halo animation** (Concourse-inspired) — a 2s ease-in-out infinite box-shadow pulse

**PatternFly's aggregate status card pattern** is relevant for the column headers: show total count with breakdown by sub-status (e.g., Completed column: 5 success, 2 failed, 1 cancelled).

---

## Real-Time Update Patterns

### WebSocket-to-Store-to-UI Pipeline

Best practice architecture from multiple sources (Smashing Magazine, Inngest, svelte-websocket-store):

```
WebSocket Connection
  --> Message Parser (deserialize JSON, validate schema)
    --> Event Router (route by event type: run_created, run_updated, job_completed)
      --> Store Mutation (update specific run/job in state store)
        --> Reactive UI (Svelte auto-rerenders affected components)
```

### Per-Concern Store Isolation

From the real-time dashboard performance article: **isolate state updates through separate stores per concern** rather than one large state object. When WebSocket messages arrive with new stats, only the relevant components re-render.

For ATC, this suggests:
- **RunStore** — all workflow runs, keyed by RunId. Source of truth.
- **RunnerStore** — runner pool state (utilization counts, pool sizes). Updated independently.
- **ConnectionStore** — WebSocket connection status (connected, reconnecting, disconnected). Controls the data freshness indicator.
- **UIStore** — theme, density mode, selected card. Pure client-side state.

Svelte 5's runes (`$state`, `$derived`) make this natural:
```
// Derived stores compute column data reactively
const queuedRuns = $derived(runs.filter(r => r.status === 'queued'))
const runningRuns = $derived(runs.filter(r => r.status === 'running'))
const completedRuns = $derived(runs.filter(r => r.status === 'completed'))
```

### Animation on Data Change

Smashing Magazine's recommendations for real-time dashboard animations:

| Update Type | Animation Duration | Effect |
|---|---|---|
| Value changes (duration tick) | 200-400ms | Fade-in or count-up transitions |
| Card status change (column move) | <300ms | Smooth slide to maintain spatial memory |
| New card arrival | Variable | Slide-in from edge |
| Card removal | <200ms | Fade-out |

**Key principles:**
- Animation should clarify updates, not distract
- Keep animations under 400ms for real-time updates
- Use `cubic-bezier(0.16, 1, 0.3, 1)` (ease-out-expo) for natural motion — the ATC prototype already uses this
- Respect `prefers-reduced-motion` — degrade to instant state changes
- Running cards' pulsating halo is functional (visible across room on wall display), not decorative

### FLIP Animation for Column Transitions

When a run changes status (e.g., queued -> running), the card needs to:
1. Disappear from the Queued column
2. Appear at the top of the Running column
3. Animate smoothly to suggest movement

Svelte's built-in `animate:flip` and `transition:` directives handle this natively:
- `animate:flip` handles reordering within a list
- `transition:slide` or `transition:fly` handle enter/exit
- `crossfade` can create the illusion of a card moving from one column to another

### Data Freshness Indicator

A compact widget showing:
- **Sync status:** Live (green dot), Stale (amber), Disconnected (red)
- **Last updated timestamp:** "Data as of 10:42 AM"
- **Manual refresh button** as fallback
- Auto-retry with exponential backoff on connection failure
- Display cached data with clear labeling during disconnection

### Handling Connection Failures

- Auto-retry with exponential backoff before notifying user
- Transparent status banners: "Offline... Reconnecting..." 
- Maintain dashboard state during brief connectivity issues (show cached data)
- Use ARIA live regions to announce connection state changes for screen readers

---

## Utilization / Resource Patterns

### Runner Pool Bars

The ATC prototype implements runner utilization as horizontal capacity bars:

```
RunnerBar (horizontal strip at top of dashboard)
  RunnerPool (one per pool: self-hosted/linux, self-hosted/macos, github-hosted)
    StatusDot (8px circle, color-coded by utilization level)
    PoolLabel (mono font, e.g. "self-hosted/linux")
    CapacityBar
      BarBackground (80px wide, surface-raised color)
      BarFill (scaleX transform, color varies by utilization)
    CountLabel (mono font, "3/5", with queued annotation)
```

**Color thresholds for utilization:**
- Green (< 70%): healthy capacity
- Amber (70-99%): approaching saturation
- Red (100%): saturated, jobs queuing

**PatternFly's utilization card pattern** confirms this approach: bar charts for proportional values (percentage of whole). Use determinate progress bars with `transform: scaleX()` for GPU-accelerated animation.

### Elastic Pool Indicator

The prototype includes a "github-hosted" pool with `total: infinity`. This requires special handling:
- Show a neutral/gray indicator (no utilization bar meaningful for infinite pools)
- Display count only ("3 active")
- Consider omitting the capacity bar entirely for elastic pools

### Aggregate Counts as KPIs

From Smashing Magazine's hierarchy strategy, the runner bar effectively serves as the dashboard's **primary KPI strip**:
- Total runners busy / total runners available
- Number of queued jobs waiting for a runner
- Pool-level breakdown

This answers the user's first question upon opening the dashboard: "Are my runners saturated? Is anything stuck waiting?"

---

## Recommended Component Hierarchy for ATC

Based on all research, here is the recommended Svelte component tree:

```
App.svelte
  |
  +-- ConnectionManager.svelte (invisible, manages WebSocket lifecycle)
  |     Writes to: RunStore, RunnerStore, ConnectionStore
  |
  +-- AppShell.svelte (layout container: flex column, full viewport)
        |
        +-- TopBar.svelte (persistent horizontal strip)
        |     +-- Logo.svelte ("ATC" branding)
        |     +-- RunnerBar.svelte (runner pool utilization)
        |     |     +-- RunnerPool.svelte (one per pool)
        |     |           +-- StatusDot.svelte (reusable)
        |     |           +-- CapacityBar.svelte (reusable progress bar)
        |     +-- ConnectionIndicator.svelte (live/stale/disconnected)
        |     +-- ThemeControls.svelte (theme picker, dark/light toggle)
        |
        +-- KanbanBoard.svelte (main content area, flex row)
        |     +-- KanbanColumn.svelte (x3: Queued, Running, Completed)
        |     |     +-- ColumnHeader.svelte (label + count badge)
        |     |     +-- CardList.svelte (scrollable, animated)
        |     |           +-- RunCard.svelte (one per workflow run)
        |     |                 +-- StatusIcon.svelte (color + symbol)
        |     |                 +-- StepProgress.svelte (step label + bar)
        |     |                 +-- RunnerLabel.svelte (hostname display)
        |     |
        |     +-- EmptyState.svelte (shown when no runs at all)
        |
        +-- [Optional] DetailPanel.svelte (slide-over for deep-dive)
              +-- RunDetail.svelte (full run info)
              +-- JobList.svelte (all jobs in the run)
                    +-- JobDetail.svelte
                          +-- StepList.svelte
                                +-- StepRow.svelte (log output, timing)
```

### Store Architecture

```
stores/
  runs.svelte.ts        -- RunStore: Map<RunId, WorkflowRun>
                           Derived: queuedRuns, runningRuns, completedRuns
  runners.svelte.ts     -- RunnerStore: pool capacities and current utilization
  connection.svelte.ts  -- ConnectionStore: ws status, last update timestamp
  ui.svelte.ts          -- UIStore: theme, density, selected run, panel open
```

### Reusable Primitives

These components appear in multiple places and should be extracted:

| Component | Used In | Purpose |
|---|---|---|
| `StatusDot` | RunnerPool, ConnectionIndicator | 8px colored circle |
| `StatusIcon` | RunCard, ColumnHeader | Status symbol with color |
| `CapacityBar` | RunnerPool, StepProgress | Horizontal progress bar with scaleX fill |
| `MonoText` | Duration, step counts, runner names | Tabular-nums mono-spaced text |
| `Badge` | ColumnHeader count, status pills | Pill-shaped count/label |
| `Toggle` | ThemeControls | Accessible switch component |

### Data Flow Summary

```
Server (WebSocket)
  |
  v
ConnectionManager (parses messages, routes events)
  |
  +---> RunStore (run_created, run_updated, run_completed events)
  |       |
  |       +---> KanbanBoard reads $derived filtered arrays
  |               |
  |               +---> KanbanColumn receives filtered run array as prop
  |                       |
  |                       +---> RunCard receives single run as prop
  |
  +---> RunnerStore (runner_status events)
  |       |
  |       +---> RunnerBar reads pool states
  |
  +---> ConnectionStore (connection lifecycle)
          |
          +---> ConnectionIndicator reads status
          +---> TopBar reads for staleness warning

UIStore (client-only, no server interaction)
  |
  +---> ThemeControls reads/writes theme + mode
  +---> RunCard reads density setting
  +---> DetailPanel reads selected run ID
```

### Key Design Decisions

1. **Props down, events up** — RunCard receives a run object as a prop, emits `select` event upward. Board manages which card is selected.

2. **Stores for cross-cutting concerns** — Theme, connection status, and run data are in stores because multiple unrelated components need them. Don't prop-drill these through 4+ levels.

3. **Derived state, not duplicate state** — Column contents are `$derived` from the run store, not maintained as separate arrays. Single source of truth prevents sync bugs.

4. **Animation at the list level** — Card enter/exit/reorder animations are managed by `CardList.svelte` using Svelte's `animate:flip` + `transition:` directives. Individual cards don't manage their own list position animations.

5. **Compact/expanded is a CSS concern** ��� The density toggle adds/removes a CSS class. CardMeta, StepProgress, and RunnerLabel hide via `display: none` in compact mode (exactly as the prototype does). No conditional rendering needed.

6. **ConnectionManager is headless** — It renders nothing. It's a component only for lifecycle management (onMount to connect, onDestroy to disconnect). All state goes into stores.

7. **No drag-and-drop** — This is a monitoring dashboard. Cards move between columns based on server state. The only user interaction with cards is selection (click to view detail).

---

## Svelte Design System / Component Library Analysis

### Carbon Design System (carbon-components-svelte)

IBM's Carbon Design System has a first-class Svelte implementation with **169 exported components** across 70+ component families. It is the most comprehensive Svelte design system available.

**Full component inventory (70+ families):**
Accordion, AspectRatio, Breadcrumb, Breakpoint, Button, Checkbox, CodeSnippet, ComboBox, ComposedModal, ContainedList, ContentSwitcher, ContextMenu, CopyButton, DataTable, DatePicker, Dropdown, FileUploader, FluidForm, Form, FormGroup, FormItem, FormLabel, Grid, Heading, ImageLoader, InlineLoading, Link, ListBox, ListItem, Loading, LocalStorage, Modal, MultiSelect, Notification, NumberInput, OrderedList, OverflowMenu, Pagination, PaginationNav, Popover, Portal, **ProgressBar**, **ProgressIndicator**, RadioButton, RadioButtonGroup, RecursiveList, Search, Select, SessionStorage, SkeletonIcon, SkeletonPlaceholder, SkeletonText, Slider, Stack, StructuredList, **Tabs**, **Tag**, TextArea, TextInput, **Theme**, **Tile**, TimePicker, **Toggle**, Tooltip, TooltipDefinition, TooltipIcon, TreeView, Truncate, **UIShell**, UnorderedList, plus icons and utilities.

**Components directly relevant to ATC:**

| ATC Need | Carbon Component | Fit | Notes |
|---|---|---|---|
| Runner utilization bars | **ProgressBar** | Good | `scaleX()` animation, determinate/indeterminate, status prop (active/finished/error), sm/md sizes |
| Step progress in cards | **ProgressBar** (inline kind) | Good | `kind="inline"` for compact progress in cards |
| Multi-step workflow indicator | **ProgressIndicator** | Partial | Designed for wizard-style step sequences, not real-time step progress |
| Loading/processing states | **InlineLoading** | Good | Animated spinner with status text, useful for "connecting..." states |
| Status tags on cards | **Tag** | Good | Color variants, filterable, closeable. But ATC needs symbol+color duality which Tag doesn't natively support |
| Card containers | **Tile** | Partial | Clickable/expandable/selectable variants exist, but ATC cards are more specialized |
| Expandable card detail | **DataTable** expandable rows | Partial | Expandable rows work for list view, not kanban cards |
| App shell / top bar | **UIShell** | Good | Header, SideNav, Content layout. Matches TopBar pattern |
| Theme switching | **Theme** | Good | 5 themes (2 light, 3 dark), CSS custom property overrides via tokens prop |
| Dark/light toggle | **Toggle** | Good | Accessible switch component |
| Skeleton loading | **SkeletonText/SkeletonPlaceholder** | Good | Loading placeholders while data arrives |
| Connection status | **Notification** (inline) | Partial | Toast/inline notifications for connection state changes |
| Tabs (mobile column switch) | **Tabs** | Good | Tab navigation for responsive single-column mode |

**Theming compatibility with ATC's OKLCH system:**

Carbon uses its own token system (`--cds-{token}`) with 5 predefined themes. Custom theming is possible via the `tokens` prop:
```svelte
<Theme theme="g90" tokens={{
  "interactive-01": "#d02670",
  "hover-primary": "#ee5396",
}} />
```

**Critical limitation:** Carbon's color system is built on hex/RGB, not OKLCH. ATC's entire design system is built on OKLCH with a single `--hue` variable that derives all neutral surfaces. Integrating Carbon would mean either:
1. **Override Carbon tokens** with OKLCH-computed values — feasible but requires mapping ATC's semantic tokens to Carbon's ~60 color tokens for each theme
2. **Run dual color systems** — Carbon tokens for Carbon components, OKLCH tokens for custom components. Risk of visual inconsistency.
3. **Abandon OKLCH for Carbon's system** — loses the elegant single-hue theming

**Svelte 5 compatibility:** v0.106.0 released 2026-04-09, under very active development (~weekly releases). However, **components still use Svelte 3/4 syntax** (`export let` props, `$:` reactive statements, `$$restProps`, `createEventDispatcher`) -- NOT Svelte 5 runes. DataTable expanded rows now use Svelte 5 snippets and generic type parameters were added, but the core component authoring model hasn't migrated to runes. This means Carbon components work in Svelte 5 via backwards compatibility, but mixing Carbon (legacy API) with custom runes-based components creates two coding styles in one project.

**Carbon ecosystem:** Beyond the 169 UI components, the Carbon Svelte portfolio includes:
- **carbon-icons-svelte**: 2,600+ icons as Svelte components
- **carbon-pictograms-svelte**: 1,500+ pictograms
- **carbon-charts-svelte**: 25+ chart types powered by d3
- **carbon-preprocess-svelte**: `optimizeImports` preprocessor rewrites barrel imports to direct paths (significant build speed improvement)

**Theme component deep dive (from source):** The `Theme.svelte` component:
- Sets `--cds-{token}` CSS variables on `document.documentElement` via `style.setProperty()`
- Sets `theme` attribute on `<html>` element and `color-scheme` property (light/dark)
- Accepts `tokens` prop for custom overrides: `tokens={{ "interactive-01": "#d02670" }}`
- Built-in toggle/select/dropdown renderers for theme switching
- LocalStorage persistence support
- The token override mechanism is the integration point for OKLCH — you'd compute hex values from OKLCH and pass them as token overrides

**ProgressBar deep dive (from source):**
- Uses `style:transform={status === "active" && \`scaleX(\${capped / max})\`}` — same `scaleX()` pattern as ATC prototype
- Status prop: `"active"` | `"finished"` | `"error"` with corresponding icons (CheckmarkFilled, ErrorFilled)
- Indeterminate mode when `value === undefined && status === "active"` — useful for "connecting..." states
- Proper ARIA: `role="progressbar"`, `aria-busy`, `aria-valuenow`, `aria-valuemin`, `aria-valuemax`
- Two sizes: `"sm"` and `"md"`, three kinds: `"default"` | `"inline"` | `"indented"`

**Verdict for ATC:** Carbon provides excellent building blocks for structural components (UIShell, Toggle, Theme, InlineLoading, SkeletonText, Notification, Tabs) but its opinionated visual style and hex-based token system conflict with ATC's OKLCH design language. The legacy Svelte 3/4 authoring style is an additional friction point. **Best used selectively** — adopt Carbon for structural/behavioral components while building custom card and status components that honor the OKLCH system. Alternatively, study Carbon's component APIs (ProgressBar's status/size/kind props, Theme's token override pattern) as reference for custom implementations.

### shadcn-svelte

A Svelte port of shadcn/ui built on top of **Bits UI** (headless primitives). 60+ components, Svelte 5 + Tailwind v4 native.

**Component inventory (relevant to ATC):**
Accordion, Alert, Badge, Button, Card, Chart, Data Table, Dialog, Drawer, Dropdown Menu, Kbd, Pagination, Progress, Scroll Area, Sheet, Skeleton, Slider, Sonner (toast), Switch, Table, Tabs, Toggle, Tooltip

**Key strengths:**
- **Copy-paste model** — components are copied into your project, not imported from node_modules. Full customization freedom. No fighting with library CSS.
- **Built on Bits UI** — headless, accessible primitives. Style however you want.
- **Tailwind v4 native** — ATC already uses Tailwind v4. Direct compatibility.
- **Card, Badge, Progress, Sheet** — directly map to ATC's RunCard, StatusBadge, CapacityBar, DetailPanel
- **Svelte 5 runes** — uses `$state`, `$props`, `$derived` natively

**Theming compatibility with OKLCH:**
shadcn-svelte uses CSS custom properties for theming, defined in your own CSS file. You have **complete control** over the color system. Adopting OKLCH would be straightforward — just define your CSS variables using OKLCH values.

**Verdict for ATC:** Excellent fit. The copy-paste model means ATC owns all component code and can adapt it freely to the OKLCH system. Bits UI headless primitives provide accessibility foundations without visual opinions. The Card, Progress, Badge, Sheet, Toggle, and Tabs components directly map to ATC needs.

### Skeleton UI (v3)

An adaptive design system powered by Tailwind CSS with Svelte and React support. Integrates with Melt UI for headless accessibility.

**Dashboard-relevant components:**
App Shell, App Bar, Cards, Progress (circular + linear), Tabs, Badges/Chips, Tables, Pagination, Navigation

**Key strengths:**
- **23 built-in themes** with a theme generator tool
- **CSS custom property theming** — themes use CSS variables, switchable at runtime
- **App Shell component** — matches ATC's layout pattern exactly
- **Progress bars** — both circular and linear variants

**Theming compatibility with OKLCH:**
Skeleton's theming is CSS-variable-based and designed for customization. Creating an ATC theme that uses OKLCH values is feasible, though Skeleton's token system may not perfectly align with ATC's single-hue derivation approach.

**Verdict for ATC:** Good structural fit (App Shell, App Bar), but Skeleton adds a significant abstraction layer between Tailwind and components. Since ATC already has a strong custom design system in `.impeccable.md`, Skeleton's opinionated design layer may cause more friction than value.

### Bits UI + Melt UI (Headless Primitives)

**Bits UI** provides unstyled, accessible component primitives for Svelte 5. **Melt UI** is the underlying builder library. These are not design systems — they're accessibility and behavior foundations.

**Available primitives:**
Accordion, Alert Dialog, Avatar, Calendar, Checkbox, Collapsible, Combobox, Context Menu, Date Picker, Dialog, Dropdown Menu, Label, Link Preview, Menubar, Pagination, Pin Input, Popover, Progress, Radio Group, Range Calendar, Scroll Area, Select, Separator, Slider, Switch, Tabs, Toggle, Toggle Group, Toolbar, Tooltip

**Key strengths:**
- **Zero styling opinions** — you bring all the CSS. Perfect for ATC's custom OKLCH system.
- **Full WAI-ARIA compliance** — keyboard navigation, screen reader support, focus management
- **Svelte 5 native** — runes, snippets, modern API

**Verdict for ATC:** The ideal accessibility foundation. Use Bits UI primitives for interactive behaviors (Toggle, Tabs, Dialog/Sheet for detail panel, Progress) while styling with ATC's OKLCH system. This is what shadcn-svelte is built on — using shadcn-svelte is effectively using Bits UI with a Tailwind styling layer.

### Recommendation Matrix

| Criterion | Carbon | shadcn-svelte | Skeleton | Bits UI |
|---|---|---|---|---|
| OKLCH compatibility | Poor (hex tokens) | Excellent (own CSS) | Good (CSS vars) | Excellent (unstyled) |
| Svelte 5 support | Active migration | Native | v3 native | Native |
| Dashboard components | Excellent breadth | Good breadth | Good breadth | Primitives only |
| Visual customization | Limited (token overrides) | Full (copy-paste) | Moderate (themes) | Full (unstyled) |
| Accessibility | Excellent | Excellent (via Bits) | Good (via Melt) | Excellent |
| Bundle size impact | Heavy (~169 components) | Minimal (copy what you use) | Moderate | Minimal |
| Learning curve | High (Carbon conventions) | Low (Tailwind + Svelte) | Moderate | Low |
| ATC prototype alignment | Low (IBM aesthetic) | High (Tailwind native) | Moderate | High |

### Recommended Approach for ATC

**Primary: shadcn-svelte (selective adoption)**
- Copy Card, Badge, Progress, Sheet, Toggle, Tabs, Scroll Area, Skeleton components
- Restyle with ATC's OKLCH tokens — shadcn-svelte's CSS variable approach makes this straightforward
- Use the built-in accessibility from Bits UI primitives underneath

**Secondary: Bits UI directly for custom components**
- Build RunCard, StatusIcon, CapacityBar, ColumnHeader as custom Svelte components
- Use Bits UI Progress primitive for runner bars and step progress
- Use Bits UI Toggle for dark mode / theme switches
- Use Bits UI Tabs for responsive column navigation on mobile

**Selective Carbon adoption (optional):**
- Consider Carbon's **InlineLoading** for connection status animation
- Consider Carbon's **SkeletonText** patterns for loading states
- Consider Carbon's **UIShell** pattern (not the component) as architectural reference

The key principle: **ATC's custom OKLCH design system is the primary visual language**. Any component library must serve it, not replace it. Headless primitives (Bits UI) plus a copy-paste styling layer (shadcn-svelte) give maximum control while providing accessibility foundations.
