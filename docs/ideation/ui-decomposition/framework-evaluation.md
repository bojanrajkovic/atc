# Svelte UI Framework Evaluation for ATC Dashboard

## Framework Comparison Matrix

| Criteria | shadcn-svelte | Skeleton (v3/v4) | Melt UI | Bits UI |
|---|---|---|---|---|
| **Svelte 5 runes** | Full support (v5-native) | Full support (v4 adopts runes/snippets) | Partial (Svelte 5 compat, internals still store-based) | Full support (rewritten from scratch for Svelte 5) |
| **Tailwind v4** | Full support (CLI generates TW4 `@theme` code, `data-slot` attrs) | Supported (v3+ requires TW4 minimum) | N/A (headless, no styling layer) | N/A (headless, no styling layer) |
| **OKLCH theming** | Excellent — uses CSS custom properties with OKLCH by default; `@theme inline` integration documented | Risky — has its own OKLCH-based theme system with named CSS properties that would **conflict** with ATC's existing `--color-*` tokens | No conflict — headless, no opinion on colors | No conflict — headless, exposes `class`/`style` props only |
| **Headless vs styled** | Styled (copies component source into your project; you own and edit it) | Styled (opinionated design system with its own token namespace) | Headless (builder API, zero styles) | Headless (component API, zero styles) |
| **Dashboard components** | Table, Data Table, Tabs, Badge, Tooltip, Dialog, Card, Progress, Collapsible, Accordion, Command palette, Dropdown, Sheet, Scroll Area | Accordion, Avatar, Navigation (Rail/Bar), Progress Bar/Ring, Slider, Segment, Dialog, Tooltip, Combobox, Listbox, Calendar, Toggle, Pagination | Accordion, Dialog, Tabs, Tooltip, Popover, Select, Collapsible, Checkbox, Toggle, Slider | Accordion, Dialog, Tabs, Tooltip, Popover, Select, Scroll Area, Collapsible, Checkbox, Toggle Group, Navigation Menu, Progress, Slider, Context Menu |
| **Bundle size** | ~0 runtime — source is copied into your project; you tree-shake what you import. Underlying Bits UI primitives are ~15-25 KB gzipped total | Larger — full design system CSS + runtime; expect 30-50+ KB for the theme layer alone | Lightweight ~10-15 KB gzipped for used builders | Lightweight ~15-20 KB gzipped for used components |
| **Accessibility** | Excellent — inherits Bits UI ARIA/keyboard/focus management; WAI-ARIA compliant | Good — built-in a11y but less granular than Bits UI primitives | Excellent — WAI-ARIA compliant, keyboard nav, focus trapping | Excellent — WAI-ARIA compliant, keyboard nav, focus management, screen reader support |
| **Community/maintenance** | ~8.6k GitHub stars; ~7.7k weekly npm downloads; actively maintained by huntabyte + community; regular releases | ~5.9k GitHub stars; ~29k weekly npm downloads; active team (Skeleton Labs); shipped v3 and v4 in 2025-2026 | ~3k GitHub stars; lower activity; Svelte 5 migration incomplete internally | ~3k GitHub stars; actively maintained by huntabyte; Svelte 5 rewrite is ground-up |
| **Custom CSS animations** | Excellent — you own the source, so `scaleX()` progress bars, pulsating halos, and `animate:flip` list transitions work without fighting the framework | Risky — opinionated component styles may conflict with custom `@keyframes` and `transform` animations | Excellent — headless, no style interference | Excellent — headless, exposes `class`/`style`, no animation interference |
| **Per-column store isolation** | Compatible — components accept props, no global state opinion; works naturally with Svelte 5 runes class-based stores | Compatible but theme system adds global CSS overhead | Compatible — builder pattern is stateless | Compatible — components are stateless, accept reactive props |
| **Breaking changes risk** | Low — you own the source; upstream changes are opt-in | Medium — major version churn (v2→v3→v4 in ~18 months); theme system changed significantly each time | Medium — internal Svelte 5 migration still in progress | Low — 1.0 rewrite for Svelte 5 is stable; same maintainer as shadcn-svelte |

## OKLCH CSS Custom Property Compatibility Analysis

The ATC prototype uses a distinctive theming architecture that creates a hard constraint on framework selection:

- **Single-hue theming**: All neutral surfaces, text, and borders derive from a single `--hue` CSS custom property via `oklch()` functions. Switching themes changes one value (`[data-theme="radar"] { --hue: 155; }`); everything recomputes via CSS.
- **Dark/light mode**: Independent of theme hue, toggled via a class (`.light` on `<body>` in the prototype, `[data-mode="dark"]` in the Svelte app) that overrides surface/text lightness values.
- **Status colors**: Fixed hues (success=145, warning=80, error=25, info=240), independent of theme.
- **Motion tokens**: CSS custom properties (`--ease-out-expo`, `--duration-fast/normal/slow`) for animation consistency.
- **Typography**: Minimal (3 sizes: 10px/12px/15px), system font + monospace stacks, `tabular-nums` on all numeric values.

**This architecture is fundamentally incompatible with any framework that has its own theme system.** The framework must either (a) be headless with zero opinion on colors, or (b) use CSS custom properties in a way that can be fully remapped to ATC's token namespace.

| Framework | OKLCH Compatibility | Risk |
|---|---|---|
| **shadcn-svelte** | **Compatible** — uses CSS custom properties (`--primary`, `--background`, etc.) with OKLCH values. These are defined in the copied component source, which you own. One-time find-and-replace to remap to ATC's `--color-*` tokens. Does not inject global CSS that would interfere with `--hue`-derived theming. | Low — the remapping is mechanical |
| **Skeleton** | **Incompatible** — defines its own OKLCH color ramp, its own `--skeleton-*` CSS custom properties, and its own `light-dark()` mode switching. These would collide with ATC's `--hue`-based derivation system. Skeleton's theme layer is not opt-in; it is foundational to all components. | High — architectural conflict |
| **Melt UI** | **Fully compatible** — headless, emits zero CSS. Your OKLCH tokens pass through untouched. | None |
| **Bits UI** | **Fully compatible** — headless, exposes `class` and `style` props. No CSS output to conflict with OKLCH. | None |

### Motion Token Compatibility

The prototype defines motion timing via CSS custom properties. None of the four frameworks ship their own motion/easing tokens. shadcn-svelte's copied components use Tailwind's `transition-*` utilities, which are easily overridden with ATC's motion tokens in the component source. Svelte's built-in `transition:` and `animate:` directives accept custom easing functions directly — they don't conflict with CSS-level motion tokens.

### Accessibility Pattern Compatibility

The prototype uses `<button role="switch" aria-checked>` for toggles and `.sr-only` utilities for screen reader text. Both shadcn-svelte and Bits UI implement these same WAI-ARIA patterns natively — shadcn-svelte's Switch component renders `role="switch"` with `aria-checked`, and its focus indicators can be restyled to use ATC's `2px solid var(--accent)` pattern.

## Detailed Evaluations

### shadcn-svelte

shadcn-svelte is the Svelte port of the wildly popular shadcn/ui pattern: a CLI copies component source code into your project, giving you full ownership. Under the hood, it uses **Bits UI** for accessible headless primitives and **Tailwind CSS** for styling. The latest version fully supports Svelte 5 runes and Tailwind v4, with components using `data-slot` attributes and the `@theme inline` directive.

**Pros:** The copy-paste model is ideal for ATC because we already have a bespoke OKLCH design system. We can take the structural/behavioral code from shadcn-svelte components and restyle them entirely with our existing `--color-*` tokens. The component catalog is the most comprehensive of all four options, covering everything a dashboard needs: Data Tables with sorting/filtering, Command palette (keyboard-first search), Tabs, Tooltips, Badges, Progress bars, Dialogs, Sheets (slide-over panels), Collapsibles, and Cards. Accessibility is inherited from Bits UI — full WAI-ARIA compliance, keyboard navigation, and focus management. Because you own the source, there's zero risk of upstream breaking changes forcing migration. Critically for ATC's real-time dashboard patterns: owning the component source means custom CSS animations (pulsating halos on running cards, `scaleX()` progress bars, smooth height transitions for expandable cards) can be applied directly without fighting framework abstractions. The Collapsible component is particularly well-suited for ATC's progressive disclosure pattern (compact cards that expand inline to show job/step details, pushing siblings down). The Badge component supports fully custom content (icon + text), making it compatible with the color + symbol duality requirement for accessible status indicators.

**Cons:** You take on maintenance of the copied component source. The default styling uses shadcn's own CSS variable convention (`--primary`, `--background`, `--foreground`) which would need to be remapped to ATC's `--color-surface-*`, `--color-text-*`, `--color-status-*` tokens. This is a one-time effort at adoption. The CLI currently assumes SvelteKit for scaffolding — a vanilla Svelte 5 + Vite setup requires manual component copying or a workaround script.

### Skeleton (v3/v4)

Skeleton is a full design system toolkit built on Tailwind CSS. It provides styled components with a comprehensive theming system. As of v3/v4, it uses OKLCH colors natively and CSS custom properties for its theme layer.

**Pros:** Skeleton is the most "batteries included" option — theme generator, component variants, presets, dark/light mode via `light-dark()`, and framework-level utilities. It has the highest npm download count, suggesting broad adoption. The v4 component set includes some dashboard-relevant items like Accordion, Navigation (Rail/Bar), Progress Bar/Ring, Dialog, Tooltip, Combobox, and Pagination.

**Cons:** Skeleton is a **design system that would conflict with ATC's existing design system**. It defines its own CSS custom property namespace (e.g., `--skeleton-bg`, `--skeleton-color-*`), its own OKLCH color ramp, its own typography scale, and its own spacing tokens. This is fundamentally incompatible with ATC's single-`--hue` theming architecture, where all neutral surfaces derive from one CSS variable via `oklch()` functions. Skeleton's theme layer would inject a parallel set of OKLCH-derived surface/text/border colors that collide with ATC's `--color-surface-*`, `--color-text-*` tokens. Skeleton also brings its own dark/light mode mechanism via `light-dark()`, which would conflict with ATC's `[data-mode="dark"]` class-based approach. Adopting Skeleton would mean either (a) abandoning ATC's carefully designed `.impeccable.md` OKLCH token system and adopting Skeleton's, or (b) fighting a constant battle to override Skeleton's theme with ATC's tokens. Neither is acceptable. Additionally, the major version churn (v2→v3→v4 in ~18 months) indicates API instability. The component catalog is smaller than shadcn-svelte, notably lacking a Data Table component. Skeleton historically required SvelteKit; vanilla Svelte 5 + Vite support exists but is secondary.

### Melt UI

Melt UI provides low-level headless "builders" — factory functions that return props, event handlers, and ARIA attributes you spread onto your own HTML elements. It is the most flexible and lowest-level option.

**Pros:** Complete headless approach means zero style conflicts with ATC's design system. The builder pattern gives maximum control over markup structure. WAI-ARIA compliance is thorough. Bundle size is minimal since you only import the builders you use.

**Cons:** Melt UI's Svelte 5 migration is **incomplete internally** — it works in Svelte 5 compatibility mode but still uses Svelte 4 store patterns under the hood. This creates a technical debt risk: code you write today with Melt UI may need rewriting when/if Melt UI fully migrates to runes. The builder API has a steeper learning curve than component-based alternatives. The component catalog is the smallest of the four options. Community momentum has shifted toward Bits UI (which was originally built on Melt UI but has since been rewritten independently for Svelte 5). Lower GitHub activity suggests reduced maintenance velocity.

### Bits UI

Bits UI is a headless component library that provides unstyled, accessible Svelte components. Originally built on top of Melt UI, it has been **rewritten from scratch for Svelte 5** with native runes support.

**Pros:** Fully headless — zero style conflicts with ATC's design system. Native Svelte 5 runes throughout (no legacy store patterns). Same maintainer as shadcn-svelte (huntabyte), ensuring alignment between the two projects. Each component exposes `class` and `style` props, making Tailwind integration trivial. WAI-ARIA compliance, keyboard navigation, and focus management are built-in. The component catalog covers most dashboard needs: Accordion, Dialog, Tabs, Tooltip, Popover, Select, Scroll Area, Collapsible, Toggle Group, Navigation Menu, Progress, Context Menu.

**Cons:** Being headless means you write all the styling yourself — there's no starting point for visual design. The component catalog is slightly smaller than shadcn-svelte (no Data Table, no Command palette, no Badge, no Card — though these are mostly styling concerns rather than behavioral ones). If you want a styled starting point, you'd essentially be recreating what shadcn-svelte already provides.

## Svelte 5 State Patterns for Real-Time Data

### Core Pattern: Reactive WebSocket Store with Runes

```typescript
// lib/stores/websocket.svelte.ts

class WorkflowStore {
  runs = $state<Map<string, WorkflowRun>>(new Map());
  connected = $state(false);
  
  // Derived computations — only recalculate when dependencies change
  runsByStatus = $derived.by(() => {
    const grouped = { queued: [], running: [], success: [], failed: [], cancelled: [] };
    for (const run of this.runs.values()) {
      grouped[run.status]?.push(run);
    }
    return grouped;
  });
  
  activeCount = $derived(
    [...this.runs.values()].filter(r => r.status === 'running').length
  );
  
  connect(url: string) {
    const ws = new WebSocket(url);
    ws.onopen = () => { this.connected = true; };
    ws.onclose = () => { this.connected = false; };
    ws.onmessage = (event) => {
      const update = JSON.parse(event.data);
      // Direct mutation — Svelte 5 proxies the Map and updates only affected DOM nodes
      this.runs.set(update.id, { ...this.runs.get(update.id), ...update });
    };
  }
}

export const workflowStore = new WorkflowStore();
```

### Key Patterns

1. **Class-based reactive stores**: Use `$state` fields in a class exported as a singleton. No need for Svelte 4's `writable`/`derived` stores. The class encapsulates both state and methods.

2. **Fine-grained derived state**: `$derived` memoizes computed values and only recalculates when actual dependencies change. Chain `$derived` for multi-level computations (e.g., `runsByStatus` → `failedInLastHour`).

3. **Direct mutation**: Svelte 5 proxies objects and arrays, so `this.runs.set(key, value)` triggers surgical DOM updates without spread/copy patterns. This is critical for high-frequency WebSocket updates (60fps responsive).

4. **`$effect` for side effects**: Use `$effect` for reconnection logic, notification sounds, or syncing to localStorage. Keep effects minimal — prefer `$derived` for computed state.

5. **Per-column store isolation**: Each kanban column should derive its own filtered view from the central store. When a WebSocket update changes a single run's status, only the affected columns (source and destination) re-render — other columns are untouched because their `$derived` dependencies didn't change. This is natural with Svelte 5 runes and does NOT require the UI framework to support any particular state pattern:

```typescript
// In a KanbanColumn component
const { status } = $props<{ status: RunStatus }>();
const columnRuns = $derived(workflowStore.runsByStatus[status]);
// Only re-renders when runs matching THIS status change
```

6. **Batching**: For burst WebSocket messages, accumulate updates in a plain array and flush to `$state` on `requestAnimationFrame` to avoid excessive re-renders:

```typescript
let pending: Update[] = [];
let rafId = 0;

ws.onmessage = (event) => {
  pending.push(JSON.parse(event.data));
  if (!rafId) {
    rafId = requestAnimationFrame(() => {
      for (const update of pending) {
        this.runs.set(update.id, { ...this.runs.get(update.id), ...update });
      }
      pending = [];
      rafId = 0;
    });
  }
};
```

### Animation Primitives

Svelte 5 provides built-in animation support relevant to real-time dashboards. **No UI framework adds animation primitives beyond what Svelte ships natively** — this is a key finding that reduces the weight of "animation support" as a framework selection criterion.

**Column-to-column card transitions** (the core animation need for a kanban-style dashboard):
- **`animate:flip`** — Smooth reordering animations when list items change position. FLIP = First, Last, Invert, Play. When a workflow run changes status (e.g., queued -> running), the card exits one column and enters another. Within a column, `animate:flip` handles position changes as items reorder.
- **Cross-column movement** requires `transition:fly`/`transition:slide` for exit/enter animations (item fades out of one `{#each}` block and fades into another). Svelte's `crossfade` from `svelte/transition` can create a paired send/receive animation where the card appears to fly from one column to another. This is built into Svelte, not any UI framework.
- **Important**: ATC's dashboard is **read-only monitoring** — cards move between columns based on server-pushed state changes, NOT user drag-and-drop. No drag-and-drop library is needed.

**Progressive disclosure animations**:
- **Expandable cards**: The Collapsible component (from shadcn-svelte/Bits UI) handles expand/collapse with smooth height transitions. For ATC's compact-to-expanded card pattern, `transition:slide` provides the smooth height animation that pushes sibling cards down.

**Real-time value animations**:
- **`svelte/motion` (spring/tweened)** — Animated number transitions for live duration counters, step progress percentages, and runner utilization bars. Spring physics for the "physical, responsive" feel specified in `.impeccable.md`.
- **`transform: scaleX()` progress bars** — Pure CSS, no framework involvement. Works with any component since shadcn-svelte's Progress component source is owned and editable.

**Accessibility**:
- **`prefers-reduced-motion`** — All Svelte transitions respect this media query when properly configured. The pulsating halo on running cards and all list animations must degrade to instant state changes, per ATC's accessibility requirements.

**Pulsating halo effect** (Concourse-inspired running indicator): Pure CSS `@keyframes` with `box-shadow` animation and `prefers-reduced-motion` fallback. No library needed — this is a CSS animation applied to the card's status indicator element.

## Recommendation: shadcn-svelte

### Rationale

**shadcn-svelte** is the recommended choice for ATC's dashboard, for these reasons:

1. **Best of both worlds**: It combines Bits UI's excellent accessibility primitives with pre-built component structure. You get the headless behavioral layer (ARIA, keyboard, focus) AND a starting point for markup, which you then restyle with ATC's OKLCH tokens.

2. **Largest dashboard-relevant component catalog**: Data Table (critical for workflow run lists), Command palette (keyboard-first search — aligns with "keyboard-first, mouse-friendly" design principle), Tabs, Tooltips, Badges, Progress bars, Dialogs, Sheets, Cards, Collapsibles, Scroll Areas. No other option matches this breadth.

3. **Zero lock-in**: The copy-paste model means components live in your repo. You can modify, delete, or replace any component without upstream dependency issues. This aligns with ATC's bespoke design system — you're not fighting a framework's opinions.

4. **OKLCH compatibility**: shadcn-svelte already uses CSS custom properties with OKLCH values. Remapping its default tokens (`--primary`, `--background`) to ATC's tokens (`--color-accent`, `--color-surface-base`) is a one-time find-and-replace in the copied component source. After that, the components are native to ATC's design system.

5. **Full Svelte 5 + Tailwind v4 support**: Both are first-class supported with documented migration guides and active CLI tooling.

6. **Shared maintainer with Bits UI**: huntabyte maintains both projects, ensuring the behavioral primitives and the component layer evolve together.

7. **Right-sized for ATC's scope**: The prototype analysis shows ATC needs only ~15-20 distinct components, with the most complex being a two-density JobCard. shadcn-svelte's copy-paste model means we only pull in the components we need — no unused framework overhead. The prototype currently has zero client-side dependencies (pure HTML/CSS/JS), so adding shadcn-svelte's Bits UI runtime (~15-25 KB gzipped) is a minimal footprint increase for significant behavioral value (accessibility, keyboard nav, focus management).

### Why not the others?

- **Skeleton**: Disqualified. Its own design system would conflict with ATC's OKLCH token system. You'd be fighting two design systems instead of building one.
- **Melt UI**: Incomplete Svelte 5 migration, smallest component catalog, steepest learning curve, declining community momentum.
- **Bits UI alone**: Viable but strictly worse than shadcn-svelte for this use case. Bits UI gives you the behavioral primitives without any structural starting point. shadcn-svelte gives you Bits UI + pre-built markup you can restyle. Since ATC needs to move fast, the head start matters.

## Component Shopping List

### From shadcn-svelte (use directly, restyle with ATC tokens)

| Component | ATC Use Case |
|---|---|
| **Data Table** | Primary workflow run list — sortable, filterable columns for run name, status, duration, repo, branch |
| **Tabs** | Top-level view switching (All / Queued / Running / Failed) or repo grouping |
| **Badge** | Status indicators (Queued, Running, Success, Failed, Cancelled) with color + symbol duality (custom icon + color, not predefined options) |
| **Tooltip** | Hover details on compact cards (full run name, commit SHA, trigger info) |
| **Dialog** | Run detail view, settings panels, keyboard shortcut reference |
| **Card** | Workflow run cards in kanban columns |
| **Collapsible** | Expandable job/step details within a run card — inline expansion with smooth height transition pushes siblings down (progressive disclosure) |
| **Progress** | Step progress within a job, overall run progress |
| **Command** | Keyboard-first search/filter (Cmd+K pattern) — critical for "keyboard-first" design principle |
| **Dropdown Menu** | Repo/org filter, theme switcher, sort options |
| **Sheet** | Slide-over detail panel for run inspection without losing dashboard context |
| **Scroll Area** | Custom-styled scroll for long run lists and step logs |
| **Separator** | Visual dividers between card sections |
| **Toggle Group** | Dark/light mode, density controls, view mode switching |
| **Kbd** | Keyboard shortcut display in command palette and tooltips |

### Build custom (not available or needs heavy customization)

| Component | Reason |
|---|---|
| **Status Kanban Board** | Core layout — no framework provides a kanban component; build with CSS Grid + Svelte `crossfade` for cross-column card transitions + `animate:flip` for within-column reordering. Read-only (no drag-and-drop needed — cards move via server state changes) |
| **Pulsating Halo** | Running-state indicator — pure CSS `@keyframes` with `box-shadow` animation and `prefers-reduced-motion` fallback to static indicator |
| **Runner Utilization Bar** | `transform: scaleX()` progress bar with CSS transition — custom because it needs the specific scaleX animation pattern, not standard Progress semantics |
| **Duration Counter** | Live-updating elapsed time — `$state` + `$effect` with `setInterval`, `svelte/motion` tweened for smooth number transitions, `tabular-nums` for layout stability |
| **Connection Status Indicator** | WebSocket connection state — simple reactive component with `$derived` |
| **Theme Switcher** | Already prototyped in `App.svelte` — extend with shadcn Toggle Group for the 4-hue selector |
| **Empty State / Idle Display** | "Calm and intentional" per design system — custom illustration + messaging |
| **Log Viewer** | Step log output — virtualized scroll with monospace font; consider a virtual list library for performance |
