# Implementation Plan Guidance

Tool-specific instructions and actions for implementation plan task writers in the ATC project.

## Rules

1. **Never pin library versions** — always use `pnpm add <pkg>` or `cargo add <crate>` to pull latest stable at execution time. Do not hardcode versions in task descriptions.

2. **Update doc-mapping.sh when adding architecture docs** — every new architecture doc needs a corresponding entry in `scripts/doc-mapping.sh` mapping source paths to the doc. The doc enforcement chain depends on this.

3. **GitHub Actions use SHA-pinned references** — all `uses:` references pin to full commit SHAs. Renovate's `helpers:pinGitHubActionDigests` preset keeps them current automatically.

4. **ADR implementation includes annotation sweep** — when implementing an ADR, sweep all existing documents and tests for superseded behavior. Annotate docs with `> **Revised by ADR-NNN:** ...` and retire or revise affected tests.
