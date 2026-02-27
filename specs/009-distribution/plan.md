# Implementation Plan: Distribution & Release

**Branch**: `009-distribution` | **Date**: 2026-02-23 | **Spec**: [spec.md](spec.md) | **Depends On**: 008-semantic-hybrid
**Input**: Feature specification from `/specs/009-distribution/spec.md`

## Summary

Set up the complete distribution pipeline for Cruxe: cross-platform binary builds
via cargo-dist/cross, GitHub Actions release workflow with automated changelog, Homebrew
tap with auto-update, MCP configuration templates for major AI coding agents, integration
guides, and auto-indexing reference templates. The goal is to make installation and
configuration effortless for the target audience: developers using AI coding agents.

## Technical Context

**Language/Version**: Rust (latest stable, 2024 edition)
**Build Tools**: cargo-dist, cross (cross-compilation), git-cliff (changelog generation)
**CI/CD**: GitHub Actions
**Distribution Channels**: GitHub Releases, Homebrew tap
**Target Platforms**: macOS arm64, macOS x86_64, Linux x86_64, Linux aarch64, Windows x86_64
**Documentation Format**: Markdown guides, JSON schemas, TOML/JSON config templates

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Code Navigation First | N/A | Distribution spec, not navigation feature |
| II. Single Binary Distribution | PASS | This spec directly implements single binary distribution |
| III. Branch/Worktree Correctness | N/A | Distribution spec |
| IV. Incremental by Design | N/A | Distribution spec |
| V. Agent-Aware Response Design | PASS | MCP config templates + integration guides serve agent awareness |
| VI. Fail-Soft Operation | N/A | Distribution spec |
| VII. Explainable Ranking | N/A | Distribution spec |

## Project Structure

### Documentation (this feature)

```text
specs/009-distribution/
├── plan.md              # This file
├── spec.md              # Feature specification
├── tasks.md             # Actionable task list
└── contracts/
    └── mcp-distribution.md # MCP template and schema distribution contract
```

### Deliverables

```text
# Build & Release
.github/
├── workflows/
│   ├── release.yml              # NEW: Release workflow (tag-triggered)
│   ├── ci.yml                   # UPDATE: Add cross-platform build matrix
│   └── homebrew-update.yml      # NEW: Homebrew formula auto-update

dist.toml                        # NEW: cargo-dist configuration
Cross.toml                       # NEW: cross-compilation configuration (if needed)

# Homebrew Tap (separate repo: signalridge/homebrew-tap)
Formula/
└── cruxe.rb               # NEW: Homebrew formula

# MCP Configuration Templates
configs/
└── mcp/
    ├── claude-code.json         # NEW: Claude Code mcp_servers config
    ├── cursor.json              # NEW: Cursor MCP config
    ├── codex.json               # NEW: Codex MCP config
    ├── generic.json             # NEW: Generic MCP server config
    └── tool-schemas.json        # NEW: JSON schema for all MCP tool definitions

# Integration Guides
docs/
├── guides/
│   ├── claude-code.md           # NEW: Claude Code integration guide
│   ├── cursor.md                # NEW: Cursor integration guide
│   ├── copilot.md               # NEW: Copilot integration guide (placeholder)
│   ├── codex.md                 # NEW: Codex integration guide
│   └── auto-indexing.md         # NEW: Auto-indexing setup guide
└── reference/
    └── mcp-tools-schema.md      # NEW: Human-readable MCP tool reference

# Auto-Indexing Templates
configs/
└── templates/
    ├── rust/
    │   ├── .cruxeignore   # NEW: Rust-specific ignore patterns
    │   └── hooks/
    │       ├── post-commit      # NEW: Git post-commit hook for sync
    │       └── pre-push         # NEW: Git pre-push hook for doctor
    ├── typescript/
    │   ├── .cruxeignore   # NEW: TypeScript-specific ignore patterns
    │   └── hooks/
    │       ├── post-commit
    │       └── pre-push
    ├── python/
    │   ├── .cruxeignore   # NEW: Python-specific ignore patterns
    │   └── hooks/
    │       ├── post-commit
    │       └── pre-push
    ├── go/
    │   ├── .cruxeignore   # NEW: Go-specific ignore patterns
    │   └── hooks/
    │       ├── post-commit
    │       └── pre-push
    └── monorepo/
        ├── .cruxeignore   # NEW: Multi-language monorepo ignore patterns
        └── hooks/
            ├── post-commit
            └── pre-push
```

**Structure Decision**: Distribution artifacts live outside the `crates/` source tree.
MCP config templates and auto-indexing templates are in `configs/`. Integration guides
are in `docs/guides/`. The Homebrew tap is a separate GitHub repository
(`signalridge/homebrew-tap`) with automated formula updates via GitHub Actions.

## Complexity Tracking

No constitution violations. Distribution is a well-defined operational concern with
no architectural complexity.
