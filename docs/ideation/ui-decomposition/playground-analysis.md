# Playground Prototype UI Analysis

## Component Inventory

| Name | Type | Data Model Fields | Interactions | Visual Notes |
|------|------|-------------------|-------------|-------------|
| **ControlsPanel** | Sidebar/Aside | state: theme, compact, dark, halo, showRunners, radius, accentBar, linuxRunners, macosRunners | Contains all control groups below | 280px fixed width, `--surface` background, right border, scrollable overflow-y, 16px padding |
| **Logo** | Header | Brand name "ATC" | None | `--mono` font, 1rem/700, `--accent` color, `--tracking-wide`, diamond symbol prefix |
| **PresetButtons** | Button group | Active preset name | Click to apply preset (Radar or Minimal) | Flex row, 6px gap, `.preset-btn` with `--text-xs`, active state uses `--accent` bg with white text |
| **ThemeSelector** | Select dropdown | theme (warm/radar/violet/pink) | onChange sets `data-theme` on `<html>` | Standard select with `--surface-raised` bg, 4px border-radius |
| **DensitySelector** | Select dropdown | compact (boolean) | onChange toggles compact class on cards | Two options: Expanded (default), Compact |
| **ToggleSwitch** | Button (role=switch) | Boolean state | Click toggles on/off | 36x20px, `--surface-raised` bg when off, `--accent` bg when on, 14px white circular knob, 16px translateX animation |
| **RangeSlider** | Range input | Numeric value (radius: 0-16, runners: 1-10/1-5) | oninput updates state and re-renders | 100px width, `--surface-raised` bg, `--border` border |
| **SimulationButtons** | Button group | None (triggers mutations) | Click: +Queued, +Running, +Failed, Advance all, Reset | Two `.btn-row` flex containers, 6px gap, `--text-xs` size |
| **RunnerBar** | Horizontal bar | Runner pools: label, used, total, queued; derived from job states | Toggle visibility via "Show runners" switch | `--surface` bg, bottom border, flex row with 16px gap, wrapping |
| **RunwayIndicator** | Inline group | label (pool name), used/total counts, queue depth | None (display only) | 8px colored dot + monospace label (120px min-width) + 80x8px progress bar + count text. Color: green (<70%), amber (70-99%), red (100%), gray (elastic/github-hosted) |
| **KanbanBoard** | Flex container | Jobs grouped by status | None (layout only) | 3-column flex, 12px gap, full height |
| **KanbanColumn** | Vertical column | Column title + item count + array of jobs | None (layout only) | flex:1, flex-direction:column. Header: uppercase `--text-xs`, `--text-dim`, 2px bottom border, count badge in `--surface-raised` pill |
| **JobCard (Expanded)** | Article | workflow, repo, branch, status, duration, currentStep, totalSteps, stepName, runner, labels | None in prototype (no click/hover handlers) | `--surface` bg, `--border` 1px border, 6px radius (configurable), 10px 12px padding. 3px left accent bar colored by status. Running cards have pulsating halo animation |
| **JobCard (Compact)** | Article | workflow, status, duration | None | Same as expanded but: 6px 10px padding, hides meta/progress/runner rows, `--text-sm` job name |
| **StatusIcon** | Span | status enum | None | `--text-base` size, 1.25em width, centered. Unique symbol per status with matching color |
| **ProgressBar** | Div group | currentStep, totalSteps, stepName | None | 4px height bar, `--surface-raised` track, status-colored fill, scaleX transform for percentage |
| **RunnerLabel** | Div | runner hostname | None | `--text-xs`, `--mono`, `--text-dim`, truncated with ellipsis, grid icon prefix |
| **DesignPrompt** | Footer region | Generated text from all current state settings | Copy button | `--surface` bg, top border, 120px max-height, scrollable |
| **CopyButton** | Button | None | Click copies prompt text to clipboard, shows "Copied!" for 1.5s | `--accent` bg, white text, `--text-xs`, 4px radius. Copied state: `--success` bg |

## Layout Map

```
+--body (flex, height:100vh, overflow:hidden)-----------------------------+
|                                                                          |
|  +--#controls (aside, 280px fixed)--+  +--#main (flex:1, column)-------+|
|  |                                  |  |                                ||
|  |  Logo: "ATC"                     |  |  +--#runner-bar (flex, wrap)--+||
|  |  Subtitle                        |  |  | RunwayIndicator (linux)    |||
|  |                                  |  |  | RunwayIndicator (macos)    |||
|  |  [Presets] Radar | Minimal       |  |  | RunwayIndicator (github)   |||
|  |  ---                             |  |  +---------------------------+||
|  |  [Theme]                         |  |                                ||
|  |    Color:   [select v]           |  |  +--#preview (flex:1, scroll)-+||
|  |    Density: [select v]           |  |  |                            |||
|  |  ---                             |  |  |  +---.kanban (flex, 3col)--+||
|  |  [Appearance]                    |  |  |  |                         |||
|  |    Dark mode     [===]           |  |  |  | Queued  | Running | Done|||
|  |    Halo animation[===]           |  |  |  | col     | col     | col |||
|  |    Show runners  [===]           |  |  |  |         |         |     |||
|  |  ---                             |  |  |  | [card]  | [card]  |[card]||
|  |  [Card Style]                    |  |  |  | [card]  | [card]  |[card]||
|  |    Border radius [---o---]       |  |  |  |         | [card]  |[card]||
|  |    Left accent   [===]           |  |  |  |         | [card]  |[card]||
|  |  ---                             |  |  |  |         |         |[card]||
|  |  [Runners]                       |  |  |  +------------------------+|||
|  |    Linux runners [---o---]       |  |  |                            |||
|  |    macOS runners [---o---]       |  |  +----------------------------+||
|  |  ---                             |  |                                ||
|  |  [Simulation]                    |  |  +--#prompt-area (footer)-----+||
|  |    [+Queued][+Running][+Failed]  |  |  | DESIGN PROMPT        [Copy]|||
|  |    [Advance all] [Reset]         |  |  | Generated prompt text...   |||
|  |                                  |  |  +---------------------------+||
|  +----------------------------------+  +-------------------------------+|
+-------------------------------------------------------------------------+
```

### Nesting Hierarchy

1. `<body>` (flex row)
   1. `<aside#controls>` (fixed-width sidebar)
      - Logo + subtitle
      - 6 `<div.control-group>` sections, each with `<h2>` header and controls
   2. `<main#main>` (flex column, fills remaining width)
      1. `<div#runner-bar>` (conditional, flex row)
         - 3 `<div.runway>` indicator groups
      2. `<div#preview>` (flex:1, scrollable)
         - `<div.kanban>` (flex row)
           - 3 `<section.kanban-col>` (Queued, Running, Completed)
             - `<h2.kanban-header>` with count badge
             - `<div.kanban-items>` (flex column, scrollable, 6px gap)
               - N `<article.job-card>` elements
      3. `<div#prompt-area>` (fixed footer)
         - Header row with label + copy button
         - Prompt text div

## State/Interaction Map

### State Variables

| Variable | Type | Default | Controls |
|----------|------|---------|----------|
| `theme` | enum (warm/radar/violet/pink) | radar | `data-theme` attribute on `<html>`, changes `--hue` CSS variable |
| `compact` | boolean | false | Adds `.compact` class to job cards, hiding meta/progress/runner |
| `dark` | boolean | true | Toggles `.light` class on `<body>`, flips all surface/text tokens |
| `halo` | boolean | true | Enables/disables `pulse-border` animation on running cards |
| `showRunners` | boolean | true | Shows/hides `#runner-bar` entirely via `display:none` |
| `radius` | number (0-16) | 6 | Sets `border-radius` inline style on each card |
| `accentBar` | boolean | true | Toggles 3px left status-colored border on cards |
| `linuxRunners` | number (1-10) | 5 | Total Linux runner capacity for runner bar calculations |
| `macosRunners` | number (1-5) | 2 | Total macOS runner capacity for runner bar calculations |
| `jobs` | array | Generated (2 queued, 4 running, 3 success, 1 failed, 1 cancelled) | Job data rendered into kanban columns |

### User Actions and Effects

| Action | Trigger | State Change | Visual Effect |
|--------|---------|-------------|---------------|
| Select theme | `<select>` change | `state.theme` + `data-theme` attr | All neutral surfaces/text shift hue; status colors unchanged |
| Toggle dark mode | Switch click | `state.dark` + body class | Surface lightness inverts, text inverts, status colors shift to darker variants |
| Toggle halo | Switch click | `state.halo` | Running cards gain/lose pulsating glow animation |
| Toggle runners | Switch click | `state.showRunners` | Runner bar appears/disappears, kanban area expands |
| Toggle accent bar | Switch click | `state.accentBar` | 3px left status bar appears/disappears on all cards |
| Change density | `<select>` change | `state.compact` | Cards shrink to single-line (icon + name + duration) or expand to show all data |
| Change border radius | Slider input | `state.radius` | Card corner rounding changes (0px square to 16px pill-like) |
| Change Linux runners | Slider input | `state.linuxRunners` | Runner bar capacity denominator changes, utilization % recalculates |
| Change macOS runners | Slider input | `state.macosRunners` | Same as above for macOS pool |
| Apply preset | Button click | Multiple state vars | Radar: dark+expanded+halo+runners+accent+6px. Minimal: warm+light+compact+no halo+no runners+no accent+4px |
| Add Queued/Running/Failed | Button click | Prepends new job to `jobs` array | New card appears at top of respective column |
| Advance all | Button click | Queued→Running, Running→Running(+step)/Success/Failed | Cards move between columns, progress bars advance, some complete |
| Reset | Button click | Regenerates default job set | All columns reset to initial distribution |
| Copy prompt | Button click | None (clipboard) | Button text changes to "Copied!" with green bg for 1.5s |
| 1-second timer | setInterval | Running job durations increment | Duration counters tick up, entire UI re-renders |

### Presets

| Preset | Theme | Dark | Compact | Halo | Runners | Radius | Accent |
|--------|-------|------|---------|------|---------|--------|--------|
| Radar | radar (155) | yes | no | yes | yes | 6px | yes |
| Minimal | warm (70) | no | yes | no | no | 4px | no |

## Design Tokens Observed

### OKLCH Color System

**Theming mechanism**: Single `--hue` CSS custom property on `:root` via `data-theme` attribute. All neutral colors derive from this hue. Theme switching is a single variable change.

#### Neutral Surface Ramp (Dark Mode)

| Token | OKLCH Value | Purpose |
|-------|-------------|---------|
| `--bg` | `oklch(12% 0.063 var(--hue))` | Deepest background (body) |
| `--surface` | `oklch(16% 0.063 var(--hue))` | Cards, panels, sidebar |
| `--surface-raised` | `oklch(20% 0.060 var(--hue))` | Inputs, recessed elements, badges |
| `--border` | `oklch(25% 0.055 var(--hue))` | Subtle borders, dividers |
| `--text` | `oklch(85% 0.028 var(--hue))` | Primary text |
| `--text-dim` | `oklch(72% 0.030 var(--hue))` | Secondary text, labels |

#### Status Colors (Fixed Hues, Theme-Independent)

| Token | Dark OKLCH | Light OKLCH | Hue | Meaning |
|-------|-----------|-------------|-----|---------|
| `--queued` | `oklch(72% 0.15 250)` | `oklch(45% 0.18 250)` | Blue | Waiting to run |
| `--running` | `oklch(78% 0.16 80)` | `oklch(45% 0.15 80)` | Amber | Currently executing |
| `--success` | `oklch(72% 0.16 155)` | `oklch(42% 0.15 155)` | Green | Completed successfully |
| `--failed` | `oklch(72% 0.17 25)` | `oklch(48% 0.18 25)` | Red | Failed |
| `--cancelled` | `var(--text-dim)` | `var(--text-dim)` | Theme | Cancelled (neutral) |
| `--accent` | `oklch(45% 0.20 250)` | Same | Blue | Buttons, active presets, focus rings |

### Typography

| Token | Value | Usage |
|-------|-------|-------|
| `--font` | `-apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif` | All UI text |
| `--mono` | `'SF Mono', 'Fira Code', 'Cascadia Code', monospace` | Durations, counts, runner names, logo, labels |
| `--text-xs` | `0.625rem` (10px) | Uppercase labels, meta, runner info, buttons |
| `--text-sm` | `0.75rem` (12px) | Card data, controls, duration, prompt text |
| `--text-base` | `0.9375rem` (15px) | Job names, status icons |
| `--leading-tight` | `1.2` | Headings, labels |
| `--leading-normal` | `1.5` | Body text |
| `--leading-relaxed` | `1.6` | Prompt text |
| `--tracking-tight` | `-0.01em` | Job names |
| `--tracking-wide` | `0.1em` | Uppercase labels |

**Tabular nums**: All monospace data values use `font-variant-numeric: tabular-nums` to prevent layout jitter.

### Spacing

| Context | Value |
|---------|-------|
| Controls panel width | 280px |
| Controls padding | 16px |
| Control group margin/padding bottom | 16px |
| Control row margin bottom | 8px |
| Kanban column gap | 12px |
| Kanban items gap | 6px |
| Card padding (expanded) | 10px 12px |
| Card padding (compact) | 6px 10px |
| Runner bar padding | 10px 16px |
| Runner bar item gap | 16px |
| Preview padding | 16px |
| Prompt area padding | 12px 16px |

### Motion

| Token | Value | Usage |
|-------|-------|-------|
| `--ease-out-expo` | `cubic-bezier(0.16, 1, 0.3, 1)` | Theme/mode transitions, progress bars, toggle knobs |
| `--ease-out-quart` | `cubic-bezier(0.25, 1, 0.5, 1)` | Card hover shadows, button hover |
| `--duration-fast` | `150ms` | Button hover/active, copy button |
| `--duration-normal` | `250ms` | Toggle animation, progress bar fill, card border |
| `--duration-slow` | `400ms` | Theme/dark mode background transitions |
| `pulse-border` | `2s ease-in-out infinite` | Running card halo glow |

**Reduced motion**: `@media (prefers-reduced-motion: reduce)` sets all animation/transition durations to 0.01ms.

### Borders and Shadows

| Element | Border | Shadow |
|---------|--------|--------|
| Cards | `1px solid var(--border)`, configurable radius (default 6px) | None at rest; running cards: `0 0 8px 2px oklch(78% 0.16 80 / 0.25)` pulsating |
| Controls panel | Right: `1px solid var(--border)` | None |
| Runner bar | Bottom: `1px solid var(--border)` | None |
| Kanban header | Bottom: `2px solid var(--border)` | None |
| Prompt area | Top: `1px solid var(--border)` | None |
| Buttons | `1px solid var(--border)` | None |
| Left accent bar | `3px solid status-color` (pseudo-element) | None |

### Scrollbar Styling

WebKit scrollbars: 6px width, transparent track, `var(--border)` thumb with 3px radius.

### Accessibility

- Focus visible: `2px solid var(--accent)`, `2px offset`
- Screen reader text: `.sr-only` utility class
- ARIA roles: `switch` on toggles, `progressbar` on step bars, `region` on major areas, `group` on kanban
- `aria-label` on all regions, articles, and progress bars
- `aria-checked` on toggle switches
- `aria-hidden="true"` on decorative status icons
- `prefers-reduced-motion` media query respected

## Recommended Svelte Component Tree

```
App.svelte
├── ThemeProvider.svelte          -- manages data-theme, dark/light class, CSS vars
│
├── ControlsSidebar.svelte        -- aside#controls, 280px fixed width
│   ├── Logo.svelte               -- brand mark + subtitle
│   ├── ControlGroup.svelte       -- reusable section with h2 label + border
│   │   ├── PresetBar.svelte      -- Radar/Minimal preset buttons
│   │   ├── ThemeControls.svelte  -- theme select + density select
│   │   ├── AppearanceToggles.svelte -- dark mode, halo, show runners toggles
│   │   ├── CardStyleControls.svelte -- border radius slider + accent bar toggle
│   │   └── RunnerSliders.svelte  -- Linux/macOS runner count sliders
│   └── SimulationControls.svelte -- +Queued/+Running/+Failed, Advance, Reset (dev only)
│
├── MainArea.svelte               -- main#main, flex column
│   ├── RunnerBar.svelte          -- horizontal runner capacity strip
│   │   └── RunwayIndicator.svelte -- single pool: dot + label + bar + count
│   │
│   ├── KanbanBoard.svelte        -- flex container for 3 columns
│   │   └── KanbanColumn.svelte   -- single column with header + scrollable items
│   │       ├── ColumnHeader.svelte -- uppercase title + count badge
│   │       └── JobCard.svelte     -- the main card component
│   │           ├── StatusIcon.svelte  -- symbol with status color
│   │           ├── JobHeader.svelte   -- icon + name + duration row
│   │           ├── JobMeta.svelte     -- repo + branch (hidden in compact)
│   │           ├── ProgressBar.svelte -- step N of M + bar + step name
│   │           └── RunnerLabel.svelte -- runner hostname (hidden in compact)
│   │
│   └── PromptPanel.svelte        -- design prompt footer (dev/playground only)
│       └── CopyButton.svelte     -- copy-to-clipboard with feedback state
│
└── (Shared/Primitives)
    ├── Toggle.svelte             -- reusable switch component (role=switch)
    ├── Select.svelte             -- styled select dropdown
    ├── Slider.svelte             -- range input wrapper
    ├── Button.svelte             -- base button with hover/active states
    └── Badge.svelte              -- pill-shaped count indicator
```

### Component Notes

**ThemeProvider**: Should use Svelte context + writable stores for `theme`, `dark`, and derived CSS custom properties. The `data-theme` attribute approach from the prototype maps directly to Svelte's element directives. Consider a `createThemeStore()` that manages both the theme hue and dark/light mode.

**JobCard**: The most complex component. Should accept a `job` prop and a `variant` prop (expanded/compact). The compact variant hides child components via conditional rendering (`{#if !compact}`), not CSS `display:none` as the prototype does. The left accent bar is a CSS pseudo-element — keep this approach. The pulsating halo animation should be a conditional class.

**RunnerBar**: Derives its data from the job store (count running jobs by label). In the real app, this would come from the StateStore's runner pool data, not be computed from jobs. The three RunwayIndicators should handle the "elastic" (github-hosted, infinite capacity) case as a special variant.

**KanbanBoard**: The column grouping (queued/running/completed) maps directly to the domain model's RunStatus enum. The prototype groups success+failed+cancelled into "Completed" — this is the right UX choice. Each column's item list should be a `{#each}` block keyed by job ID.

**ControlsSidebar**: This is largely a playground/dev-tool concern. In production, only Theme, Density, and Appearance toggles would likely survive as user preferences stored in localStorage or a preferences store. Runner slider and simulation controls are dev-only.

**PromptPanel**: Playground-only component, not needed in production. However, the prompt generation logic is a useful reference for documenting the design system.

### State Management Recommendation

Use Svelte 5 runes ($state, $derived, $effect) rather than stores:

- `$state` for theme settings (persisted to localStorage)
- `$state` for the job/run collection (fed from SSE/WebSocket in production)
- `$derived` for kanban column groupings, runner utilization calculations
- `$effect` for syncing theme to DOM attributes and localStorage

The prototype's `updateAll()` function that re-renders everything on every state change is a direct match for Svelte's reactive model — each component will reactively update when its dependencies change, without manual orchestration.

### CSS Architecture

The prototype uses a single global `<style>` block. For Svelte:

- **Global tokens** (`--hue`, `--bg`, `--surface`, status colors, typography, motion): Keep in a global `theme.css` or `:root` block in `app.css` / Tailwind v4 `@theme`
- **Component styles**: Use Svelte scoped styles for component-specific rules
- **Theme switching**: The `data-theme` + `--hue` pattern works perfectly with Svelte — set `data-theme` on `<html>` via `$effect`
- **Dark/light mode**: The `.light` class override pattern also works directly; consider `color-scheme: dark` / `color-scheme: light` for native form controls
- **Tailwind integration**: The OKLCH tokens can be mapped to Tailwind v4's `@theme` layer, making them available as utility classes while keeping the single-variable theme switching
