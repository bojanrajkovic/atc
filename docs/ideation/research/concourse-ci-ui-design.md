# Concourse CI Web UI/UX Deep Dive

## 1. Pipeline Visualization Layout

### Core Design: Job-Centric Model

Concourse's pipeline visualization is fundamentally **job-centric**, not stage-centric. The primary function is to show the flow of versioned resources through connected jobs that make up a pipeline.

**Visual Structure:**
- Jobs appear as **boxes/nodes** in a graph layout
- Resources (versioned artifacts) connect jobs with **lines**
- The graph is interactive: you can **zoom, pan, and fit-to-view**
- Large pipelines are organized with **job groups** for easier comprehension
- A list of jobs appears in a group; neighbors in the current group also appear on the same page for context

### Resource Flow Representation

This is Concourse's most distinctive visualization approach:

- **Solid lines** between jobs = `trigger: true` (automatically triggered when upstream resource changes)
- **Dotted lines** between jobs = `trigger: false` (dependency exists but not automatic)
- Users can **hover over connecting lines** to trace a resource's full path across the entire pipeline
- Resources are represented with **icons** to help identify different types at a glance (git, S3, Docker registry, etc.)

### Information Density

The design philosophy deliberately **strips away unnecessary information**. Instead of showing every detail:
- Removes resources from wall-display views to reduce clutter
- Shows only enough data for "at a glance" triaging
- Uses visual bandwidth efficiently for large pipelines

---

## 2. Job Status Color Coding & Visual Feedback

### Halo Indicators (Pulsating Rings)

This is Concourse's signature real-time status indicator:

- **Yellow halo** = Job currently running (pulsates outward)
- **Red halo** = Job failed
- **Grey** = Job pending/waiting
- **Blue** = Job or resource is paused
- **Orange** = Internal error (different from red to distinguish misconfiguration failures from infrastructure problems)
- **Brown banner** = Build aborted

### Accessibility Enhancements

To support color blindness, Concourse adds **icons and symbols**:
- Red failures include a **warning triangle** at the top of the job column
- Icons help distinguish job states beyond color alone

### Status Banners

- A **colored banner** behind the job name and build number gives "at a glance" understanding of build status/stage
- This serves as a visual confirmation: "if it looks wrong, it probably is wrong"

---

## 3. Dashboard & Multi-Pipeline Monitoring

### Evolution of Dashboard View

User request drove innovation: teams wanted to **observe multiple pipelines simultaneously** rather than opening multiple browser windows.

### Dashboard Features

**Standard Dashboard Routes:**
- `/` = Default dashboard with all visible pipelines across all teams
- `/hd` = High-density (HD) view for wall displays
- `/dashboard/` (v3.5.0+) and `/beta/dashboard/` (v3.6.0+)

**Treemap Visualization:**
- Uses a **treemap chart** layout showing job status density
- Resources removed from view to reduce visual complexity
- "Stripped down thumbnail to just jobs"
- Provides just enough information for triage: **focused on failed jobs and failure duration**

### Design Rationale for Monitoring

The dashboard was designed around what teams actually care about:
- **Which jobs failed?**
- **How long have they been failing?**
- This information is critical for engineering teams to triage errors and prioritize work

---

## 4. Wall Display / Big Screen Optimization

### Designed for TV/Wall Monitors

Concourse explicitly addresses wall-display use cases (CI radiator/monitor pattern):

- High-density (`/hd`) view removes unnecessary detail for glance-able status
- Treemap layout scales well on large displays
- Color and halo indicators visible from distance
- No tiny text or dense information that requires reading

### "Radiator" Pattern

Teams use Concourse on dedicated wall displays (TVs, dashboards) to:
- Keep team visibility on pipeline health
- Quick notification of failures
- At-a-glance team awareness

---

## 5. Information Design: Showing Dependencies & Parallel Execution

### Job Dependencies

**"Passed" Constraint Model:**
- Jobs can depend on other jobs via `passed` constraints
- The resulting network is a **dependency graph that continuously advances**
- The visualization shows this as lines connecting jobs
- Users can see the entire path of a resource or build constraint

### Parallel Execution

Concourse treats parallel execution as a **core architectural feature**, not an afterthought:

- Multiple jobs that trigger on the same resource change appear **side-by-side**
- The grid layout clearly shows which jobs run in parallel
- Jobs are divided into "a grid representing the visual matrix of space permutations"
- The visualization makes parallel relationships obvious

### Interactive Drill-Down

- Click on jobs to view details
- Click on resources to see version history
- Single click moves from failed job to understanding root cause
- The visualization acts as a "gut check" feedback mechanism

---

## 6. What Makes Concourse UI Distinctive

### Reasons for Praise

1. **Modern Built-in Visualization**
   - Clean, intuitive interface (no plugins needed, unlike Jenkins)
   - Consistent UI/UX across platforms
   - Designed to make complex pipelines easy to understand

2. **Resource-Flow Model**
   - Most CI systems don't visualize resources and versioning
   - Concourse makes **the flow of artifacts through jobs** the primary model
   - This is philosophically unique in CI/CD tools

3. **User-Centered Design Evolution**
   - Design based on actual user needs and feedback
   - "Learning, framing, assessing, and iterating"
   - Responsive to team feedback (e.g., dashboard added due to user requests)

4. **At-a-Glance Monitoring**
   - Specific focus on information needed for triage
   - Visual feedback system (halos, colors, icons) works from across a room
   - Designed for distributed teams and wall displays

5. **Interactive Transparency**
   - One-click navigation from failure to root cause
   - Hover-to-trace resource paths
   - Visual representation of triggers vs. dependencies

### Common Criticisms

1. **Scalability Issues**
   - UI "breaks" or becomes unusable with **dozens of concurrent jobs**
   - Performance degrades with very large pipelines

2. **Discoverability & Learning Curve**
   - UI offers "very little in terms of discoverability"
   - Some elements are cryptic without guidance
   - Steep learning curve despite "clean" design

3. **Limited Workflow Control**
   - **No conditional jobs** (can't say "if A succeeds AND B succeeds, then C")
   - Rigid pipeline structure
   - No templates (each step must be configured manually)
   - Limited plugins

4. **Browser Navigation Issues**
   - Browser back button doesn't always work
   - Limited state preservation in certain views

5. **Information Gaps**
   - Could expose more information about past runs
   - Some views difficult to read or click on

---

## 7. Comparison to Jenkins

### Concourse Advantages

| Aspect | Concourse | Jenkins |
|--------|-----------|---------|
| **Visualization** | Built-in, native to tool | Requires plugins (Blue Ocean, etc.) |
| **Pipeline clarity** | Job and resource flow obvious | Depends on plugin quality |
| **Resource management** | First-class visualization | Not well represented |
| **UI consistency** | Unified design | Varies by plugins |
| **Plugin overhead** | Minimal; clean interface | Plugin complexity can clutter UI |

### Why This Matters for ATC

Concourse demonstrates that **purpose-built visualization can make complex processes clear**. Unlike Jenkins' plugin-heavy approach, Concourse's focused design philosophy (resource versioning + job flow + at-a-glance status) creates a tool that teams trust visually.

---

## Key Takeaways for "Action Traffic Control" (ATC)

### Visual Principles Worth Stealing

1. **Color + Symbol Redundancy**: Don't rely on color alone; add icons/symbols for accessibility
2. **Halo Animation for State**: Real-time pulsating feedback (especially for running state) is effective
3. **Information Density Discipline**: Strip ruthlessly; show only what users need to triage
4. **Interactive Drill-Down**: Single-click navigation from overview to details
5. **Hover Tracing**: Allow users to trace paths/dependencies via hover
6. **Wall Display Optimization**: Ensure design works on large screens viewed from distance
7. **Trigger vs. Dependency Distinction**: Visual differentiation (solid/dotted lines) for automation vs. dependency
8. **Resource/Version Visualization**: Show artifact flow, not just job success/failure

### Challenges to Avoid

1. Don't let parallel execution visualization become confusing with too many simultaneous indicators
2. Scalability matters—design for pagination/virtualization if showing many pipelines
3. Keep discoverability high; don't assume users understand the visualization model immediately
4. Preserve browser navigation state (back button must work)
5. Design for both detailed single-pipeline view AND multi-pipeline overview

---

## Sources

- [Concourse CI Official Site](https://concourse-ci.org/)
- [Concourse Pipeline UI Explained - Medium](https://medium.com/concourse-ci/concourse-pipeline-ui-explained-87dfeea83553)
- [Designing a Dashboard for Concourse - Medium](https://medium.com/concourse-ci/designing-a-dashboard-for-concourse-fe2e03248751)
- [Designing for Space in Concourse - Medium](https://medium.com/concourse-ci/designing-for-space-in-concourse-3037344644c6)
- [Concourse Build Page Explained - Medium](https://medium.com/concourse-ci/concourse-build-page-explained-4f92824c98f1)
- [The Making of a Cloud-Native CI/CD Tool - Tanzu Blog](https://blogs.vmware.com/tanzu/the-making-of-a-cloud-native-ci-cd-tool-the-concourse-journey)
- [Concourse vs Jenkins Comparison - eficode](https://www.eficode.com/blog/jenkins-concourse)
- [Concourse CI Modern Tool Overview - Coding and Beyond](https://www.codingandbeyond.com/2025/02/04/concourse-a-modern-ci-cd-tool/)
- [GitHub Issues - Web UI/UX Accessibility](https://github.com/concourse/concourse/issues/5964)
- [GitHub Issues - Dashboard Feedback](https://github.com/concourse/concourse/issues/1829)
