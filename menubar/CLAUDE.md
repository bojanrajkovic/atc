# CLAUDE.md — menubar

Last verified: 2026-11-13

> Canonical documentation lives in `docs/architecture/menubar-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

macOS menu bar app that lists the latest jobs from an ATC server. A Swift package built with `swift build` and `build.sh`; no Xcode project.

## Commands

```bash
make build    # Debug build into build/ATCMenubar.app
make run      # Build and launch
make test     # ATCKit tests
make dist TEAM_ID=XXXXXXXXXX   # Signed, notarized zip in dist/
```
