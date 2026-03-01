## Why

Cruxe needs a complete distribution pipeline to make installation and configuration effortless for its target audience: developers using AI coding agents. Most users will not have a Rust toolchain and should not need one, so prebuilt binaries and Homebrew are the primary adoption paths. Beyond installation, MCP configuration templates, agent integration guides, and auto-indexing reference templates bridge the gap between "installed" and "productively using."

## What Changes

1. Provide prebuilt static binaries for 5 target platforms via cargo-dist/cross with a GitHub Actions release workflow, automated changelog, and SHA-256 checksums. [US1]
2. Provide a Homebrew tap at `signalridge/tap/cruxe` with automatic formula updates on new releases. [US2]
3. Publish MCP configuration templates for Claude Code, Cursor, Codex, and a generic MCP server, plus a JSON schema for all tool definitions. [US3]
4. Provide agent integration guides with step-by-step setup, recommended prompt rules, workflow tips, and troubleshooting for Claude Code, Cursor, Copilot, and Codex. [US4]
5. Provide reference auto-indexing templates including git hooks and project-type-specific `.cruxeignore` files for Rust, TypeScript, Python, Go, and multi-language monorepos. [US5]

## Capabilities

### New Capabilities

- **Binary distribution pipeline**: Cross-platform prebuilt static binaries (macOS arm64/x86_64, Linux x86_64/aarch64, Windows x86_64) via cargo-dist/cross, GitHub Actions release workflow with changelog generation and checksums.
  - FR-801: System MUST provide prebuilt static binaries for: macOS arm64, macOS x86_64, Linux x86_64, Linux aarch64, Windows x86_64.
  - FR-802: System MUST use cargo-dist and/or cross for cross-compilation and release artifact generation.
  - FR-803: System MUST provide a GitHub Actions release workflow that triggers on tag push, builds all target binaries, generates changelog from conventional commits, and creates a GitHub release with attached artifacts and checksums.
  - FR-811: System MUST generate changelogs organized by conventional commit type (feat, fix, refactor, docs, test, chore) with links to PRs.
  - FR-812: System MUST include SHA-256 checksums for all release artifacts.

- **Homebrew tap**: Homebrew distribution for macOS (and Linux Homebrew) users with automatic formula updates.
  - FR-804: System MUST provide a Homebrew tap at `signalridge/tap/cruxe` with automatic formula updates on new releases.

- **MCP configuration templates**: Ready-to-use config templates for AI coding agents with JSON schema for tool definitions.
  - FR-805: System MUST publish a JSON schema file for all MCP tool definitions, validated against the MCP specification.
  - FR-806: System MUST provide MCP configuration templates in `configs/mcp/` for Claude Code, Cursor, Codex, and a generic MCP server configuration.

- **Agent integration guides**: Step-by-step setup guides with prompt rules, workflow tips, and troubleshooting.
  - FR-807: System MUST provide agent integration guides with step-by-step setup, recommended prompt rules, workflow tips, and troubleshooting sections.
  - FR-808: System MUST provide integration guides for: Claude Code, Cursor, Copilot (if MCP support available), and Codex.

- **Auto-indexing templates**: Reference configurations for automatic indexing in common project types.
  - FR-809: System MUST provide reference auto-indexing templates including git hooks (`post-commit` for sync, `pre-push` for doctor) and project-type-specific `.cruxeignore` files.
  - FR-810: System MUST provide reference configurations for common project types: Rust, TypeScript, Python, Go, and multi-language monorepos.

### Modified Capabilities

- None. This spec is entirely additive distribution infrastructure.

> **Cross-reference**: spec 001 US1 also requires `cargo install cruxe` as a source-based installation path. This spec (009) complements that with prebuilt binary distribution; the `cargo install` path remains supported but is not gated by this spec's release workflow.

### Key Entities

- **ReleaseBinary**: A platform-specific compiled binary with version, target triple, checksum, and download URL.
- **HomebrewFormula**: A Ruby formula describing the package, dependencies, installation method, and platform-specific download URLs.
- **MCPConfigTemplate**: A JSON/TOML configuration file for a specific AI coding agent that enables Cruxe as an MCP server.
- **IntegrationGuide**: A markdown document with step-by-step instructions for setting up Cruxe with a specific AI coding agent.
- **AutoIndexTemplate**: A collection of configuration files (git hooks, ignore patterns, config overrides) for automatic indexing in a specific project type.

## Impact

- SC-801: A user with no Rust toolchain can install and run Cruxe on any of the 5 target platforms in under 2 minutes.
- SC-802: `brew install signalridge/tap/cruxe` succeeds and the installed binary passes `cruxe doctor` on macOS.
- SC-803: A new user following any integration guide can have Cruxe working with their AI agent within 10 minutes.
- SC-804: The Homebrew formula is automatically updated within 1 hour of a new GitHub release.
- SC-805: All MCP configuration templates pass validation against their respective agent's configuration schema.

### Edge Cases

- Wrong-platform binary download: binary fails to execute with OS-level error. README and release page clearly label each binary's target platform.
- Homebrew checksum mismatch: Homebrew reports the error. Automation re-generates checksums on each release, so this indicates a tampered or incomplete download. User retries.
- Outdated agent configuration template: templates are versioned alongside the release. Guide instructs users to check the template version matches their installed Cruxe version.
- Silent git hook failure: reference hook templates include error handling that logs failures to `~/.cruxe/logs/hook.log`. Users are guided to check this log in troubleshooting.
- Concurrent `cruxe sync` during active indexing: sync detects the active job and skips, logging a message. This is existing behavior from the core MVP.

### Affected Crates

- None. This spec is entirely distribution, documentation, and configuration -- no new Rust code in `crates/`.

### API Impact

- Additive only: MCP tool JSON schema published alongside release artifacts.

### Performance Impact

- No runtime performance impact. Distribution infrastructure only.

### Readiness Baseline

- Repository governance baseline is now present (`ci`, `security`, `pr-title`, and OpenSpec trace gate workflows plus `.pre-commit-config.yaml`).
- Parallel-development guardrails are documented under `openspec/meta/parallel-development-guardrails.md` for multi-stream release prep.
