# ATC — Actions Traffic Control

Real-time GitHub Actions monitoring. One dashboard, all your orgs, zero polling.

> **Status:** Under construction. Tooling and conventions are in place; application code is landing in phases.

## The problem

Every GitHub Actions dashboard either polls (slow, rate-limited), is dead, or can't show you what actually matters: which runner is stuck, how deep the queue is, and which step just failed. You end up clicking through per-repo views in the GitHub UI, mentally assembling the picture yourself.

## What ATC does

ATC receives webhook events directly from GitHub and pushes them to your browser over WebSocket. No polling, no lag — jobs move across a kanban board as they happen.

- **Cross-org, single pane** — every org that sends webhooks shows up in one view
- **Per-step progress** — see which step is running, not just "in progress"
- **Runner pool visibility** — queue depth and assignments per runner label set
- **One-click to GitHub** — every card links directly to the Actions run
- **Single binary** — Rust backend serves the SPA, receives webhooks, handles auth. Deploy one thing.

## Quick start

```bash
# Prerequisites: mise (https://mise.jdx.dev)
just setup    # Install all tools and dependencies
just dev      # Start development servers
```

## Documentation

| What | Where |
|------|-------|
| Contributing & setup | [CONTRIBUTING.md](CONTRIBUTING.md) |
| Architecture | [docs/architecture/](docs/architecture/) |
| Design decisions | [docs/architecture-decisions/](docs/architecture-decisions/) |
| Design research & prototype | [docs/ideation/](docs/ideation/) |

## License

[Apache-2.0](LICENSE)
