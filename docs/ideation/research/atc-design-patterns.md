# ATC UI Design Patterns - Concrete Implementation Guide

## Color Palette & State Mapping

### Primary State Colors
```
Running (In-flight):      #FFD700 or #FFC107 (Amber/Yellow)
Queued (Taxiing):         #87CEEB or #4A90E2 (Sky Blue)
Success (Landed):         #4CAF50 or #26A65B (Green)
Failed (Crash):           #F44336 or #E74C3C (Red)
Cancelled (Diverted):     #9E9E9E or #7F8C8D (Gray)
Skipped:                  #BDBDBD (Light Gray, desaturated)
```

### Secondary Indicators
```
Warning (approaching limits):     #FF9800 (Orange)
Critical (requires action):       #E91E63 (Magenta/Deep Pink)
Holding Pattern (stuck):          #FFEB3B (Bright Yellow)
Blocked (dependency):             #9C27B0 (Purple)
```

---

## Symbol & Icon Design

### Core Job States

**Queued/Taxiing:**
```
◐ + arrow ← (waiting to enter runway)
or
⊲⊲⊲ (triple chevron showing queue position)
or
✛ (plus/ready sign)
```

**Running/Airborne:**
```
▶ (play symbol, showing action)
or
✈ (airplane icon with trail)
or
● with orbital motion (circle with animation)
```

**Completed/Landed:**
```
✓ (checkmark)
or
✔ (solid checkmark)
or
⏹ (square/landing pad)
```

**Failed/Crashed:**
```
✗ (X symbol)
or
⊗ (circle with X)
or
⚠ (warning triangle)
```

**Blocked/Dependency:**
```
◆ (diamond, blocking shape)
or
⟡ (chain link symbol)
or
🔗 (explicit link)
```

### Job Type Differentiation

Use shape + color combination:
```
Test job:      ● (circle)
Build job:     ■ (square)
Deploy job:    ▲ (triangle)
Package job:   ◈ (diamond)
Integration:   ◎ (circle with dot)
```

**Example compound symbol:**
```
Running test:     ▶● (play + circle)
Failed build:     ✗■ (X + square, both red)
Queued deploy:    ◐▲ (queue indicator + triangle)
```

---

## Data Block Layouts

### Compact Label (At-a-glance)
```
┌────────────────┐
│ TEST-SUITE    │  ← Job name (max 12 chars)
│ ▶ 03:45      │  ← Status icon + elapsed time
└────────────────┘
```

### Standard Label (Radar strip style)
```
┌──────────────────┐
│ DEPLOY-STAGING   │  Field A: Job ID/name
│ ▶ Running        │  Field B: Status + verb
│ Step 8/12 █████▌ │  Field C: Progress
│ Est: 02:15       │  Field D: Metrics
└──────────────────┘
```

### Extended Label (Full detail)
```
┌──────────────────────────┐
│ TEST-INTEGRATION         │
│ ▶ Running since 03:45    │
│ Step 8/12 ████████▌      │
│ Prev: build-linux        │  (Previous job)
│ Next: coverage-report    │  (Next job)
│ Runner: macos-large (#4) │  (Resource allocation)
│ Est completion: 02:15    │  (Projected time)
└──────────────────────────┘
```

### Minimal Label (Zoomed out, many jobs)
```
[DEPLOY▶] 03:45  or just  [▶] 03:45
```

---

## Progress Representation

### Linear Progress Bar
```
Running (6 of 12 steps):
Step 6/12 ████████░░░░ 50% ETA: 02:34

Alternative filled representations:
████░░░░░░░░  (discrete blocks)
████████░░░░  (fluid gradient)
████████░░░░  (with checkerboard pattern)
```

### Circular Progress
```
For compact display:
    ▓▓▓▓
  ▓▓▓▓▓▓▓▓
 ▓▓▓▓▓▓▓▓▓▓▓
▓▓▓▓▓▓▓▓▓▓▓▓▓
 ▓▓▓▓▓▓▓▓▓▓▓
  ▓▓▓▓▓▓▓▓
    ▓▓▓▓
    (6/12)
```

### Breakdown View
```
Job execution phases:
Setup ✓ │ Build ▶ │ Test ░░ │ Deploy ░░░░
```

---

## Radar-Style Layout

### Plan View (Top-down)
```
┌─────────────────────────────────────┐
│        AIRSPACE (Workflow)          │
│                                     │
│    RUNWAY 1 (macos-latest)         │
│      Taxiing: [◐ TEST-UNIT]        │
│      Flying:  [▶ BUILD-APP]        │
│      Landed:  [✓ LINT]             │
│                                     │
│    RUNWAY 2 (ubuntu-latest)        │
│      Taxiing: [◐ TEST-INT]         │
│      Flying:  [▶ COVERAGE]         │
│                                     │
│    QUEUE (waiting):                 │
│      [◐◐◐] 5 jobs waiting          │
│                                     │
└─────────────────────────────────────┘
```

### Sector View (Grouped by phase)
```
QUEUE SECTOR          ACTIVE SECTOR         LANDING SECTOR
[◐ TEST-1]            [▶ BUILD-1]           [✓ LINT ✓ FORMAT]
[◐ TEST-2]            [▶ BUILD-2]           [✓ AUDIT]
[◐◐◐] +3              [▶◐] deploy pending

Delay: 2 jobs        Running: 2 jobs       Completed: 3 jobs
```

---

## Runner/Runway Status

### Runway Availability Board
```
RUNWAY UTILIZATION:

Runway 1 (macos-latest):
  │████████░░░░│ 8/12 available
  Active: BUILD-APP (started 03:45)

Runway 2 (ubuntu-latest):
  │████████░░░░│ 8/12 available
  Active: COVERAGE (started 03:52)

Runway 3 (windows-latest):
  │░░░░░░░░░░░░│ 12/12 available
  [Ready for next]

Queue depth: 5 jobs waiting
Est. drain time: ~8 minutes
```

### Simplified Runway Status
```
🟢 macos-latest     8 free  (Running: 4)
🟡 ubuntu-latest    6 free  (Running: 6)
🔴 windows-latest   0 free  (Running: 12)
```

---

## Visual Transitions & Animations

### State Change Animations
```
Queued → Running:
  [◐ TEST] → [▶ TEST]  (transform icon + highlight pulse)

Running → Complete:
  [▶ TEST] → [✓ TEST]  (smooth fade, keep on screen 3-5s)

Failed → Retry:
  [✗ TEST] → [◐ TEST]  (flash warning color, then reset)
```

### Progress Animation
```
Continuous bar advance:
████░░░░░░ → ████░░░░░░ → █████░░░░░ ...
(Update every 1-2 seconds, smooth interpolation)

Pulsing indicator for long-running:
████████░░ (pulse glow on current step)
```

### Attention-Drawing (without distraction)
```
New alert/failure:
1. Subtle highlight flash (20-50ms)
2. Icon color shift (if not already visible)
3. Position elevation (raise slightly)
4. Optional: sound cue (very soft, optional)
```

---

## Information Density Reduction

### Tiered Display Strategy

**Level 1: Icon-only (very dense)**
```
▶ ✓ ◐ ✓ ▶ ✗ ✓ ◐ ▶ ✓ ✗ ◐
```
(Just symbols, 30+ items visible)

**Level 2: Icon + Name**
```
▶ BUILD         ✓ TEST         ◐ DEPLOY
▶ COVERAGE     ✓ LINT         ✗ SECURITY
```
(Name + icon, 10-15 items visible)

**Level 3: Icon + Name + Status**
```
▶ BUILD         Running 02:34
✓ TEST          Passed in 01:12
◐ DEPLOY        Queued, 5 ahead
```
(Full standard label, 5-8 items visible)

**Level 4: Full detail**
```
(Everything from extended label, max 2-3 items visible without scrolling)
```

### Zoom/Scale Adaptation
```
Zoomed out:    Show only icons + job names
Medium zoom:   Add status + elapsed time
Zoomed in:     Full labels with all metadata
```

### Pagination/Grouping Strategy
```
By workflow:
  ▸ On: Push validation (8 jobs)
    ✓ ✓ ✓ ▶ ◐ ◐ ◐ ◐
  ▸ Deploy: Staging (3 jobs)
    ▶ ◐ ◐
  ▸ Deploy: Production (1 job)
    ◐

(Click to expand/collapse each group)
```

---

## Keyboard/Command Interface (Optional, inspired by openScope)

### Command Shortcuts
```
? or h         Show help/command reference
r <job-id>    Retry job
c <job-id>    Cancel job
v <job-id>    View logs (verbose)
l <job-id>    Show labels/tags
p <job-id>    Pause job
k <job-id>    Kill job

Navigation:
↑ ↓           Navigate between jobs
→ ←           Expand/collapse details
[space]       Select current job
[enter]       Open selected job details
[esc]         Go back / dismiss modal
```

### Command reference display:
```
┌────────────────────────────┐
│ COMMAND REFERENCE          │
├────────────────────────────┤
│ ?              Show this    │
│ r <id>         Retry       │
│ c <id>         Cancel      │
│ v <id>         Verbose     │
│ ↑/↓            Navigate    │
│ [esc]          Back        │
└────────────────────────────┘
```

---

## Practical Color Examples

### Comprehensive Palette for Accessibility

**Main States (WCAG AA contrast-compliant):**
```
Status      Light Mode        Dark Mode        RGB
────────────────────────────────────────────────────
Running     #FFC107          #FFD700          (255, 199, 0)
Queued      #2196F3          #64B5F6          (33, 150, 243)
Success     #4CAF50          #66BB6A          (76, 175, 80)
Failed      #F44336          #EF5350          (244, 67, 52)
Cancelled   #9E9E9E          #BDBDBD          (158, 158, 158)
Blocked     #9C27B0          #BA68C8          (156, 39, 176)
```

**For colorblind users**, pair with:**
- Symbols (different shapes)
- Patterns (hatching, textures)
- Labels (text indicators)

Example colorblind-safe icon:
```
Success: ✓ (universal symbol)
Failed:  ✗ (universal symbol)
Running: ▶ (arrow implies motion)
Queued:  ◐ (incomplete circle)
```

---

## Sample Mock-up Wireframe

```
╔════════════════════════════════════════════════════════════════╗
║ ATC - Action Traffic Control Dashboard                         ║
╠════════════════════════════════════════════════════════════════╣
║                                                                ║
║  CURRENT ACTIVITY                      RUNNERS                ║
║  ┌──────────────────────────┐  ┌──────────────────────────┐   ║
║  │ TEST-UNIT           ▶    │  │ 🟢 macos-latest     8 /12  │   ║
║  │ ████████░░ 8/12 03:45    │  │ 🟡 ubuntu-latest   11 /12  │   ║
║  │                          │  │ 🟢 windows-latest  12 /12  │   ║
║  │ BUILD-APP           ▶    │  └──────────────────────────┘   ║
║  │ ███████░░░ 7/12 02:15    │                                ║
║  │ Next: COVERAGE           │  QUEUE DEPTH                   ║
║  │                          │  ┌──────────────────────────┐   ║
║  │ COVERAGE            ◐    │  │ 5 jobs waiting           │   ║
║  │ Queued, est 02:30        │  │ ◐ TEST-INTEGRATION       │   ║
║  └──────────────────────────┘  │ ◐ TEST-SECURITY          │   ║
║                                │ ◐ BUILD-API              │   ║
║  HISTORY                        │ ◐ BUILD-WORKER           │   ║
║  ┌──────────────────────────┐  │ ◐ COVERAGE               │   ║
║  │ ✓ LINT                   │  └──────────────────────────┘   ║
║  │ ✓ FORMAT                 │                                ║
║  │ ✓ AUDIT                  │  WARNINGS                      ║
║  │ ✗ SECURITY (retry)       │  ┌──────────────────────────┐   ║
║  └──────────────────────────┘  │ ⚠ BUILD-API running 8m   │   ║
║                                │ ⚠ ubuntu runner at 92%   │   ║
║                                └──────────────────────────┘   ║
║                                                                ║
╚════════════════════════════════════════════════════════════════╝
```

---

## Recommended Implementation Stack

Based on ATC display design:
- **Vector graphics** (SVG) for symbols (sharp, scalable)
- **Canvas** for progress bars and animations (smooth 60fps)
- **CSS animations** for state transitions (non-blocking)
- **Real-time updates** via WebSocket (like radar refresh rate)
- **Progressive enhancement** (works with JavaScript disabled, adds animations on top)

---

## References

All design patterns derived from:
- FAA Standard Terminal Automation Replacement System (STARS) documentation
- Real aviation ATC display research (see main research document)
- openScope simulator visual design
- BlueSky ATC visualization principles
- WCAG 2.1 AA accessibility standards
