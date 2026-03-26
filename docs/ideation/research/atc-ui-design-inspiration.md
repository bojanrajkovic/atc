# Air Traffic Control UI Design Inspiration for ATC (Action Traffic Control)

## Executive Summary

Real ATC systems and ATC simulators have developed sophisticated visual languages for managing high-density, real-time systems with multiple states and priorities. This research synthesizes design patterns from three sources: openScope (web-based ATC simulator), BlueSky (research ATC simulator), and real FAA/operational ATC displays. These insights are directly applicable to GitHub Actions workflow monitoring, where you're metaphorically mapping queued jobs → taxiing, running jobs → in the air, and runners → runways.

---

## 1. Visual Representation Systems

### 1.1 Real ATC Radar Displays

**Symbol Design Fundamentals:**
- **Target symbols**: Modern ATC uses an acute isosceles **triangle pointing in the direction of travel** (heading indicator)
- **Historical design**: Older systems used square symbols as the "target head" (actual aircraft position) with velocity vectors (lines showing 1-minute projected position)
- **Category coding**: Aircraft size/category (Small, Large, Heavy, Super) is encoded in the symbol itself (via line placement or absence)
- **TA/RA status**: In cockpit displays, amber circles indicate Traffic Advisory, red squares indicate Resolution Advisory

**Key insight for ATC tool**: Use directional indicators (arrows/triangles) pointing toward current action state. A job "heading toward" deployment/completion gets a specific orientation.

### 1.2 openScope UI Approach

openScope is a browser-based ATC simulator focused on **command-based interaction** with a radar display. Key features:
- Uses traditional radar visualization with Mercator projection maps
- Toggleable overlays: Fixes & Runways, SID/STAR displays, Airspace, Terrain, Video Maps
- Supports multi-player simulation with keyboard-driven command interface
- Aircraft states include: taxiing (to runway), in-flight (climbing/descending/level), approach/landing
- Command reference system: `[↑/↓]` to navigate, `[tab]` to select, `[esc]` to go back

**Aircraft flight phases with state representation:**
- **Taxiing**: Aircraft moves to hold short of runway
- **Takeoff**: Clear for takeoff, climb to assigned altitude or follow SID procedure
- **In-flight**: Cruise, level-off at assigned altitude
- **Approach**: ILS approach, following glideslope, landing

**Key insight for ATC tool**: State transitions follow a clear pipeline. Use visual progression (not just color) to show workflow progression.

### 1.3 BlueSky Visualization

BlueSky (TU Delft research simulator) uses Python with two rendering backends:
- **PyGame**: Classic GUI (older systems support)
- **Qt+OpenGL**: Advanced GUI with real-time 3D visualization

**Display characteristics:**
- **Top-down Mercator projection** of airspace
- **Synthetic symbols** based on multiple surveillance data sources (PSR blips + SSR responses = cleaner target representation)
- Supports ADS-B traffic visualization
- Used extensively for research in Air Traffic Management and conflict detection/resolution

**Key insight for ATC tool**: Top-down perspective with real-time symbol updates provides clarity. Use "synthetic" clean symbols rather than raw data blips.

---

## 2. Information Density Management & Visual Hierarchy

### Challenge
Controllers work with **many simultaneous aircraft in different states**. Display design must avoid cognitive overload while maintaining necessary information.

**The Problem:**
- Large quantities of overlaid information require **salience-manipulation strategies** to avoid clutter
- Display scale interacts with information sampling density
- Too many colors or codes can obscure important information
- Contrast is critical: low-contrast changes can be missed if controller attention is directed elsewhere

### Solutions from Real ATC

**Color-based visual hierarchy:**
- Psychological and **cartographic principles** enable layering information into visual hierarchy
- Color palettes designed for **layered data** (not random color schemes)
- Different colors for different aircraft states or conformance categories
- Controllers prefer **minimal coding** on displays (research shows they reject overly cluttered symbology)

**Size and prominence techniques:**
- **Label shrinking**: Only most important information shown (expandable on demand)
- **Two-tier label system**: Simple labels (basic info) vs. extended labels (full information)
- **Scale-dependent display**: Different information shown based on zoom level

**Practical implementation from research:**
From [Moving Toward an Air Traffic Control Display Standard](https://hf.tc.faa.gov/publications/2010-moving-toward-an-air-traffic-control-display-standard/full_text.pdf):
- Controllers indicated radar displays should NOT be cluttered with too much coding
- Unnecessary/difficult-to-process data distracts from primary task
- Integrating weather + traffic creates clutter problems; must use salience manipulation

### Key Insight for ATC Tool
Implement **progressive disclosure**: Show queue count at glance, expand to see details. Use color for priority/state, size for importance. Avoid simultaneous display of all metadata.

---

## 3. Color Coding Conventions

### Real ATC Color Usage
**Important caveat**: FAA has **NOT yet standardized color schemes** across different display vendors, though research is ongoing. However, general color coding conventions exist:

**Status color coding** (from industry standards):
- **Green**: Normal/healthy operation
- **Yellow**: Caution/warning/intermediate state
- **Red**: Critical/alert/immediate attention needed
- **Amber**: Traffic advisory / potential conflict

**Weather radar precedent** (though this is different from target display):
- Black: < 0.7 mm/hr rainfall
- Green: 0.7-4 mm/hr
- Yellow: 4-12 mm/hr
- Red: > 12 mm/hr

### Application to GitHub Actions
- **Green**: Jobs completed successfully
- **Yellow**: Jobs queued/pending, or running (in-progress)
- **Red**: Jobs failed/cancelled, or system issues
- **Amber/Orange**: Warnings or stuck jobs

**For the runway metaphor:**
- Green = runway clear
- Yellow = runway in use (job running)
- Red = runway closed/unavailable (runner down)

### Key Insight
Use color primarily for **state**, not just severity. Pair color with symbol shape/orientation for colorblind accessibility.

---

## 4. State Representation: Queued vs. In Progress vs. Completed

### Real ATC Model: The Flight Progress Strip

Traditional ATC uses **physical flight progress strips** (paper cards) with information organized in rows:

**Strip layout** (vertical, rectangular):
```
[CALLSIGN]        (top, most prominent)
[SSR CODE]
[ALTITUDE]
[SPEED/ROUTE]
[COMMENTS]        (bottom, variable)
```

**Digital equivalent (Data Block Format):**
- **Field A**: Aircraft ID (call sign, 2-7 characters)
- **Field B/C**: Altitude (assigned vs. actual, with up/down arrow for climb/descent)
- **Field D**: Speed (ground speed, 001-999)
- **Field E**: Computer ID and handoff/beacon data

**Key visual components:**
- **Target symbol** (position indicator)
- **Leader line** (connects symbol to data block)
- **Velocity vector** (1-minute projected position, shows direction + speed)
- **History dots** (5 previous tracked positions)

### Mapping to GitHub Actions Jobs

**Queue/Taxiing state:**
```
[JOB_NAME]
[Queued/Waiting]
[Runner: ___/12]   ← runner availability
[Duration: --:--]
```

**In Progress/Airborne state:**
```
[JOB_NAME]
[Running] ▶
[Step: 3/12]       ← progress indicator
[Duration: 02:34]
[↗ trend info]     ← direction of next step
```

**Completed state:**
```
[JOB_NAME]
[Completed] ✓
[Duration: 05:47]
[Result: PASS]
```

---

## 5. Visual Language for Workflow States

### Symbol Design Principles (from FAA research)

**Directional indicators show motion/intent:**
- Triangle pointing up = climbing
- Triangle pointing down = descending
- Triangle pointing left/right = heading in that direction
- Square = holding position / no change

**For GitHub Actions mapping:**
- ↗ arrow = queued, ready to move up (toward execution)
- ▶ arrow = running/in progress
- ✓ or → = completed
- ⊗ or ⬇ = failed/cancelled

### Category/Type Coding

Real ATC encodes aircraft category (small, large, heavy) in the symbol itself. For GitHub Actions:
- Symbol type could indicate job category (test, build, deploy, etc.)
- Color could indicate priority (critical path, blocking, optional)

### Velocity Vectors as Progression

In real ATC, velocity vectors show **projected position in 1 minute**. For jobs:
- Could show projected completion time
- Could indicate step progression with a "trend" indicator
- Visual bar filling from left to right (0% → 100%)

---

## 6. Layout & Organization Principles

### Information Ordering (from [SKYbrary - Plots, Tracks and Labels](https://skybrary.aero/articles/plots-tracks-and-labels))

**Standard ATC label structure** prioritizes information:
1. **Identity** (top, most critical) - aircraft ID/call sign
2. **Altitude** (second) - current/assigned level with direction
3. **Speed** (supporting) - ground speed
4. **Additional info** (bottom, variable) - route, handoff, warnings

### Display Sectoring

Real ATC controllers have **different views based on role**:
- **Planners**: Zoomed out, see far ahead to sequence traffic
- **Controllers**: Zoomed in, focus on current sector
- **Supervisors**: Medium zoom, monitor coordination

**For ATC GitHub tool:**
- **Queue view**: All jobs waiting to run (planner perspective)
- **Activity view**: Currently running jobs (controller perspective)
- **History view**: Recently completed jobs

### Proximity Clustering

In real ATC, related aircraft are grouped by:
- Altitude blocks (e.g., FL350-FL375)
- Approach/departure procedures
- Holding patterns

**For GitHub Actions:**
- Group by workflow (e.g., push-validation, deploy-staging, deploy-prod)
- Group by runner type/availability
- Group by failure type

---

## 7. Real-time Update Strategies

### From [Design of Information Display Systems for Air Traffic Control](https://hf.tc.faa.gov/publications/2004-design-of-information-display-systems/full_text.pdf)

**Display update rate requirements:**
- Position updates: High frequency (~1-2 Hz or faster)
- Label changes: Event-driven
- Alerts/warnings: Immediate (top priority)

**Visual change principles:**
- **Smooth transitions**: Changes should animate, not jump instantly
- **Highlight changes**: New updates get subtle attention (flash, highlight)
- **Consistency**: Same information always appears in same location
- **Non-intrusive**: Don't distract during critical operations

**For GitHub Actions:**
- Job status updates: immediate (state change is critical)
- Progress updates: animated (step count filling bar)
- Log updates: on-demand (don't auto-scroll or redirect focus)
- Completed jobs: fade into history view after N seconds

---

## 8. Summary: Design Patterns for ATC Tool

### Core Design Principles

1. **Directional Symbols**: Use arrows/triangles showing workflow direction (queued→running→done)

2. **Color + Shape Duality**:
   - Color = state (green=success, yellow=running, red=failed)
   - Shape = type (circle=test, square=build, triangle=deploy)

3. **Progressive Disclosure**:
   - At-a-glance: icon + status text + 1 key metric
   - Expanded: full data block with details
   - Detail view: logs and full trace

4. **Visual Hierarchy via Size/Prominence**:
   - Running jobs: larger, more prominent
   - Queued jobs: medium size, grouped
   - Completed/old jobs: smaller, fade to history

5. **Scalar Information**:
   - Progress bar (step 3/12, duration elapsed)
   - Runner availability (4/12 runners available)
   - Queue depth (15 jobs waiting)

6. **Velocity Vectors** (adapted):
   - Show projected completion time
   - Show next step or blocking issue
   - Use trend indicators (→, ↗, ↘)

7. **Minimize Clutter**:
   - Default to simple labels
   - Expand on hover/interaction
   - Hide metadata until requested
   - Use data aggregation (show summary, not raw logs)

8. **Radar vs. Strip Metaphor**:
   - Radar view: spatial layout showing all jobs (like real runway/airspace)
   - Strip view: list view grouped by status/workflow (like flight progress strips)
   - Timeline view: Gantt-style showing job duration and queueing patterns

---

## 9. Sources & References

### Official FAA/ATC Standards
- [Moving Toward an Air Traffic Control Display Standard](https://hf.tc.faa.gov/publications/2010-moving-toward-an-air-traffic-control-display-standard/full_text.pdf)
- [Design of Information Display Systems for Air Traffic Control](https://hf.tc.faa.gov/publications/2004-design-of-information-display-systems/full_text.pdf)
- [Color Analysis in ATC Displays, Part I](https://libraryonline.erau.edu/online-full-text/faa-aviation-medicine-reports/AM06-22.pdf)
- [Guidelines for the Use of Color in ATC Displays](https://www.tc.faa.gov/its/worldpac/techrpt/ar99-52.pdf)

### ATC Simulators & Visualization
- [openScope Air Traffic Control Simulator](https://www.openscope.co/) - Browser-based, command-driven
- [GitHub: openscope/openscope](https://github.com/openscope/openscope)
- [BlueSky ATC Simulator](https://github.com/TUDelft-CNS-ATM/bluesky) - Python-based research tool

### Reference Standards
- [SKYbrary - Plots, Tracks and Labels](https://skybrary.aero/articles/plots-tracks-and-labels)
- [SKYbrary - Situation Display](https://skybrary.aero/articles/situation-display)
- [FAA Section 3: Flight Progress Strips](https://www.faa.gov/air_traffic/publications/atpubs/atc_html/chap2_section_3.html)

### Human Factors & Design
- [Situation Awareness in Air Traffic Control: Enhanced Display Design](https://rosap.ntl.bts.gov/view/dot/16675/dot_16675_DS1.pdf)
- [Color Usability on ATC Displays](https://www.researchgate.net/publication/253111803_Color_Usability_on_Air_Traffic_Display)
