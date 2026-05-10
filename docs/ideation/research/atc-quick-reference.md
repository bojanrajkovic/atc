# ATC UI Design - Quick Reference for GitHub Actions

## The Three Systems Researched

| System | Type | Key Insight for Your Tool |
|--------|------|--------------------------|
| **Real ATC Displays** | Operational, FAA-regulated | Proven human factors patterns; avoid clutter; use color + shape duality |
| **openScope** | Browser simulator | Command-driven interface; flight state pipeline (taxi→fly→land); multi-player considerations |
| **BlueSky** | Research simulator (Python) | Top-down spatial view; synthetic symbols (clean, not raw); multiple rendering backends |

---

## One-Page Design System

### States → Colors
```
Queued (taxiing)     → Blue     (#2196F3)
Running (airborne)   → Yellow   (#FFC107)
Success (landed)     → Green    (#4CAF50)
Failed (crashed)     → Red      (#F44336)
Blocked (held)       → Purple   (#9C27B0)
Cancelled (diverted) → Gray     (#9E9E9E)
```

### States → Symbols
```
Queued    ◐  or  ⊲  or  ⬜
Running   ▶  or  ⚙  or  ●
Success   ✓  or  ✔  or  ⬛
Failed    ✗  or  ⚠  or  ◆
Blocked   🔗 or  ⧈  or  ◇
```

### Job States Pipeline
```
┌─────────┐   ┌─────────┐   ┌──────────┐   ┌───────────┐
│ Queued  │──▶│ Running │──▶│ Complete │──▶│ History   │
│   ◐    │   │   ▶    │   │ ✓ or ✗  │   │ (faded)   │
└─────────┘   └─────────┘   └──────────┘   └───────────┘
    ▲                            │
    └────────────────────────────┘
           (retry path)
```

---

## Data Block Template (steal directly from ATC)

```
Standard 3-line label:

┌──────────────────┐
│ TEST-SUITE       │  Line 1: Job name (most important)
│ ▶ Running 03:45  │  Line 2: Status icon + elapsed time
│ Step 5/12 ░░░░░░ │  Line 3: Progress bar or metrics
└──────────────────┘
```

**Ordering principle**: Identity → Status → Details

---

## Information Density: Progressive Disclosure

```
Context 1 - Many jobs visible (zoomed out):
  ▶ ✓ ◐ ✓ ▶ ✗ ✓ ◐ ▶ ✓ ✗ ◐  (icon only)

Context 2 - Standard view:
  ▶ TEST-SUITE 03:45
  ✓ LINT
  ◐ DEPLOY

Context 3 - Detail view (single job):
  ┌──────────────────────────┐
  │ DEPLOY-STAGING           │
  │ ▶ Running since 03:45    │
  │ Step 8/12 ████████▌      │
  │ Previous: build-linux    │
  │ Runner: macos-large (#4) │
  └──────────────────────────┘
```

**Key rule**: Show only info needed for current context. Hide metadata until requested.

---

## Real ATC Principles Applied

### 1. Vector Arrows for Direction
**In ATC**: Triangle points in direction of flight
**For you**: Arrows show workflow direction
- ← : waiting to queue
- ▶ : moving toward completion
- ↗ : escalating (priority increase)
- ↘ : step backward (retry)

### 2. Color is Secondary to Symbol
Controllers reject displays where color is the ONLY indicator.
**Always pair color with shape/symbol** for colorblind users.

### 3. Minimize Cognitive Load
- No more than 3-4 colors on screen
- No more than 5-6 simultaneous metadata fields
- Expand details on interaction, don't show all at once
- Use familiar symbols (✓ = success, ✗ = failure)

### 4. Velocity Vector = Progress
In ATC: Line shows 1-minute projected position
For you: Progress bar shows step-by-step or time-to-completion

### 5. Runway Capacity ≈ Runner Availability
ATC displays runway utilization as a critical resource
**Show runner availability clearly**:
```
🟢 ubuntu-latest    8 /12
🟡 macos-latest     4 /12
🔴 windows-latest   0 /12
```

### 6. The Flight Progress Strip
ATC's proven solution for showing multiple pieces of info cleanly:
- Vertical, rectangular card
- 5 lines max, ordered by importance
- Leader line connects symbol to label
- Only critical info shown; details on demand

---

## Visual Density Strategies (ranked by effectiveness)

1. **Grouping** - Collapse/expand workflow groups
2. **Filtering** - Show only: running, only failed, only next-to-run, etc.
3. **Timeline** - Horizontal Gantt-style view instead of list
4. **Layers** - Multiple toggleable information layers
5. **Zoom** - Scale detail level based on viewport size
6. **Pagination** - Separate into pages (avoid infinite scroll)

---

## Color Palette (WCAG AA accessible)

```
Dark mode (recommended for status dashboards):
Running:   #FFD700 (gold)
Queued:    #64B5F6 (light blue)
Success:   #66BB6A (green)
Failed:    #EF5350 (red)
Blocked:   #BA68C8 (purple)
Neutral:   #BDBDBD (gray)

Light mode:
Running:   #FFC107 (amber)
Queued:    #2196F3 (blue)
Success:   #4CAF50 (green)
Failed:    #F44336 (red)
Blocked:   #9C27B0 (purple)
Neutral:   #9E9E9E (gray)
```

Test with: https://webaim.org/resources/contrastchecker/

---

## Update Frequency & Animations

**From ATC display research:**
- Position updates: 1-2 Hz (smooth, not jumpy)
- Status changes: Event-driven (immediate)
- Alerts: Top priority (visual highlight)
- Completed items: Fade after 3-5 seconds

**Animations:**
- State transitions: 300-400ms (smooth but responsive)
- Progress bars: Update every 1-2 seconds
- Attention pulse: 200ms flash on change (subtle)

---

## Layout Styles to Consider

### Option A: Radar View (spatial)
```
┌─────────────────────────────┐
│       RUNWAY 1              │
│   ◐ ◐ ◐ (queue)            │
│     ▶ (active)             │
│   ✓ ✓ (completed)          │
│       RUNWAY 2              │
│   ◐ (queue)                │
│   ▶ (active)               │
│   ✓ (completed)            │
└─────────────────────────────┘
```

### Option B: Strip View (temporal, like paper strips)
```
TEST-UNIT     ▶ 03:45  ████░░
BUILD-APP     ▶ 02:30  ███░░░
COVERAGE      ◐ Queued ░░░░░░
LINT          ✓ 01:12  Passed
```

### Option C: Gantt/Timeline View (with duration & dependencies)
```
TEST-UNIT     ├─────▶    3:45
BUILD-APP       ├──────▶  2:30
COVERAGE              ◐   Wait
```

### Option D: Board View (Kanban-style)
```
| Queued      | Running     | Completed  |
|─────────────|─────────────|─────────────|
| ◐ TEST-1    | ▶ BUILD-1   | ✓ LINT     |
| ◐ TEST-2    | ▶ BUILD-2   | ✓ FORMAT   |
| ◐◐ +2 more  |             | ✓ AUDIT    |
```

---

## Anti-Patterns to Avoid

❌ **Display clutter** - Too many colors, too much info at once
✓ **Solution**: Progressive disclosure, folding/grouping

❌ **Color-only states** - No symbols or text labels
✓ **Solution**: Pair color with shape (e.g., 🟢✓ for success)

❌ **Inconsistent positioning** - Info appears in different locations
✓ **Solution**: Fixed layout (header at top, progress below, details last)

❌ **No visual feedback** - Static, no indication of changes
✓ **Solution**: Smooth animations, highlight changes

❌ **Ignoring runners/capacity** - Only show jobs, not resources
✓ **Solution**: Display runner availability as critical info

❌ **All states equal importance** - Running and queued get same visual weight
✓ **Solution**: Make running jobs larger/more prominent

---

## Implementation Checklist

- [ ] **Color scheme defined** - Test for WCAG AA contrast
- [ ] **Symbol set designed** - At least 5 core states (queued, running, success, failed, blocked)
- [ ] **Data block template** - 3-4 lines, identity → status → details
- [ ] **Progressive disclosure** - Icon view → standard → detailed
- [ ] **Runner display** - Show capacity and utilization clearly
- [ ] **Animation timings** - Define transition speeds (300-400ms typical)
- [ ] **Update frequency** - Real-time for status, throttled for logs
- [ ] **Accessibility** - Color + symbol, WCAG AA minimum
- [ ] **Responsive design** - Works at different zoom/viewport sizes
- [ ] **Keyboard shortcuts** (optional) - Command interface like openScope

---

## Key Takeaway

**Real ATC systems are brilliant at displaying high-density, real-time information where errors are costly and cognitive load is heavy.**

Steal their playbook:
1. Use symbols + colors (not color alone)
2. Order by importance (identity first, details last)
3. Hide metadata by default
4. Show runner availability like runway capacity
5. Smooth animations for state changes
6. Familiar symbols (checkmark for success, X for failure)
7. Progress bars for long operations
8. Groups/filters for managing many items

Your GitHub Actions dashboard can be more useful AND more beautiful by borrowing from 70+ years of ATC display design research.
