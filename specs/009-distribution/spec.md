# Feature Specification: Distribution & Release

**Feature Branch**: `009-distribution`
**Created**: 2026-02-23
**Status**: Draft
**Phase**: 4 | **Version**: v1.3.0
**Depends On**: 008-semantic-hybrid
**Input**: User description: "Build pipeline with cargo-dist + cross, Homebrew tap, MCP distribution docs, agent integration guides, local auto-indexing reference templates"

## Readiness Baseline Update (2026-02-25)

- Repository governance baseline is now present (`ci`, `security`, `pr-title`,
  and OpenSpec trace gate workflows plus `.pre-commit-config.yaml`).
- Parallel-development guardrails are documented under
  `specs/meta/parallel-development-guardrails.md` for multi-stream release prep.

## User Scenarios & Testing

### User Story 1 - Install CodeCompass via Prebuilt Binary (Priority: P1)

A developer downloads a prebuilt static binary for their platform from the GitHub
releases page or installs via Homebrew. The binary works immediately without
requiring a Rust toolchain, and the release includes an automated changelog
generated from conventional commits.

**Why this priority**: Binary distribution is the primary adoption path. Most users
will not have a Rust toolchain and should not need one.

**Independent Test**: Download the release binary for the current platform from
GitHub releases, run `codecompass --version`, then `codecompass doctor` -- both
must succeed without any additional setup.

**Acceptance Scenarios**:

1. **Given** a macOS arm64 machine without Rust installed, **When** the user
   downloads the release binary and runs `codecompass --version`, **Then** the
   binary executes successfully and prints the correct version.
2. **Given** a Linux x86_64 machine, **When** the user downloads the release binary,
   **Then** it is statically linked and has no runtime library dependencies beyond
   libc.
3. **Given** a new GitHub release is created from a tag, **When** the release
   workflow completes, **Then** binaries for all 5 target platforms are attached
   to the release, along with checksums and a changelog.
4. **Given** the changelog, **When** a user reads it, **Then** it is organized by
   conventional commit types (feat, fix, refactor, etc.) and includes links to
   relevant PRs.
5. **Given** a Windows x86_64 machine, **When** the user downloads and extracts the
   release archive, **Then** `codecompass.exe` runs without additional DLL
   dependencies.

---

### User Story 2 - Install CodeCompass via Homebrew (Priority: P1)

A macOS (or Linux with Homebrew) user installs CodeCompass via
`brew install signalridge/tap/codecompass`. The tap is automatically updated
when a new GitHub release is published.

**Why this priority**: Homebrew is the dominant package manager for macOS developers,
providing automatic updates and easy uninstall.

**Independent Test**: Run `brew install signalridge/tap/codecompass`, then
`codecompass --version` and `codecompass doctor` -- all must succeed.

**Acceptance Scenarios**:

1. **Given** a macOS user with Homebrew installed, **When** they run
   `brew install signalridge/tap/codecompass`, **Then** the latest release binary
   is downloaded and installed to `$(brew --prefix)/bin/codecompass`.
2. **Given** a new GitHub release is published, **When** the Homebrew tap automation
   runs, **Then** the formula is updated with the new version, checksums, and
   download URLs within 1 hour.
3. **Given** the user runs `brew upgrade codecompass`, **When** a newer version is
   available, **Then** the binary is updated to the latest release.
4. **Given** the Homebrew formula, **When** `brew audit --strict` is run on it,
   **Then** no errors or warnings are reported.

---

### User Story 3 - Configure MCP Server in AI Coding Agents (Priority: P1)

A developer reads the MCP distribution docs and configures CodeCompass as an MCP
server in their preferred AI coding agent. Configuration templates are provided
for Claude Code, Cursor, Copilot, and Codex. The JSON schema for all tool
definitions is published alongside the docs.

**Why this priority**: MCP configuration is the gateway to the primary use case.
If configuration is difficult, adoption stalls regardless of tool quality.

**Independent Test**: Copy the Claude Code configuration template, paste it into
the agent's config file, start the agent, and verify CodeCompass tools are listed
and callable.

**Acceptance Scenarios**:

1. **Given** the Claude Code configuration template, **When** a user adds it to
   their `mcp_servers` config, **Then** CodeCompass tools are discoverable via
   `tools/list` and functional.
2. **Given** the Cursor MCP configuration template, **When** a user adds it to
   their Cursor MCP config, **Then** CodeCompass tools are discoverable and
   functional.
3. **Given** the JSON schema file for all tool definitions, **When** validated
   against the MCP specification, **Then** it passes validation with no errors.
4. **Given** the configuration templates directory (`configs/mcp/`), **When**
   a user lists the files, **Then** they find templates for Claude Code, Cursor,
   Codex, and a generic MCP template.

---

### User Story 4 - Follow Agent Integration Guide (Priority: P2)

A developer reads the integration guide for their specific AI coding agent and
follows the step-by-step instructions to set up CodeCompass. The guide includes
recommended prompt rules, workflow tips, and troubleshooting steps.

**Why this priority**: Integration guides convert interest into active usage.
They bridge the gap between "installed" and "productively using."

**Independent Test**: A developer with no prior CodeCompass experience follows
the Claude Code integration guide from start to finish and successfully uses
`locate_symbol` within 10 minutes.

**Acceptance Scenarios**:

1. **Given** the Claude Code integration guide, **When** a new user follows all
   steps, **Then** they can successfully index a repo and locate a symbol via
   MCP within 10 minutes.
2. **Given** the integration guide includes a recommended prompt rule
   `"use CodeCompass tools before file reads"`, **When** an agent follows this
   rule, **Then** it calls `locate_symbol` or `search_code` before resorting to
   reading files directly.
3. **Given** the Cursor integration guide, **When** a user follows the setup
   steps, **Then** CodeCompass tools appear in Cursor's MCP tool list.
4. **Given** a troubleshooting section in the guide, **When** a user encounters
   a common issue (e.g., "tools not showing up"), **Then** the guide provides
   a resolution path.

---

### User Story 5 - Set Up Auto-Indexing for a Project (Priority: P3)

A developer uses reference templates to configure auto-indexing for their project.
Git hooks trigger `codecompass sync` on commit, and the project stays indexed
automatically. Reference configurations are provided for common project types
(Rust, TypeScript, Python, Go monorepos).

**Why this priority**: Auto-indexing eliminates the manual step of running
`codecompass index` and ensures the index is always fresh.

**Independent Test**: Install the provided git hook, make a commit, verify that
`codecompass sync` ran and the index is updated.

**Acceptance Scenarios**:

1. **Given** the reference git `post-commit` hook, **When** a user installs it
   and makes a commit, **Then** `codecompass sync` runs automatically and the
   index reflects the new commit.
2. **Given** a Rust project reference configuration, **When** a user applies it,
   **Then** the `.codecompassignore` patterns are appropriate for Rust (ignoring
   `target/`, `*.o`, etc.) and indexing is efficient.
3. **Given** the auto-indexing templates directory, **When** a user lists the
   files, **Then** they find templates for Rust, TypeScript, Python, Go, and
   a generic multi-language template.
4. **Given** a git pre-push hook template, **When** installed, **Then** it runs
   `codecompass doctor` before push to verify index health.

### Edge Cases

- What happens when the binary is downloaded for the wrong platform?
  The binary fails to execute with an OS-level error. The README and release page
  clearly label each binary's target platform.
- What happens when `brew install` fails due to checksum mismatch?
  Homebrew reports the error. The automation re-generates checksums on each release,
  so this indicates a tampered or incomplete download. User retries.
- What happens when an agent configuration template is outdated?
  Templates are versioned alongside the release. The guide instructs users to check
  the template version matches their installed CodeCompass version.
- What happens when a git hook fails silently?
  The reference hook templates include error handling that logs failures to
  `~/.codecompass/logs/hook.log`. Users are guided to check this log in
  troubleshooting.
- What happens when `codecompass sync` is triggered during an active indexing job?
  The sync detects the active job and skips, logging a message. This is existing
  behavior from the core MVP.

## Requirements

### Functional Requirements

- **FR-801**: System MUST provide prebuilt static binaries for: macOS arm64, macOS x86_64,
  Linux x86_64, Linux aarch64, Windows x86_64.
- **FR-802**: System MUST use cargo-dist and/or cross for cross-compilation and release
  artifact generation.
- **FR-803**: System MUST provide a GitHub Actions release workflow that triggers on tag
  push, builds all target binaries, generates changelog from conventional commits, and
  creates a GitHub release with attached artifacts and checksums.
- **FR-804**: System MUST provide a Homebrew tap at `signalridge/tap/codecompass` with
  automatic formula updates on new releases.
- **FR-805**: System MUST publish a JSON schema file for all MCP tool definitions,
  validated against the MCP specification.
- **FR-806**: System MUST provide MCP configuration templates in `configs/mcp/` for
  Claude Code, Cursor, Codex, and a generic MCP server configuration.
- **FR-807**: System MUST provide agent integration guides with step-by-step setup,
  recommended prompt rules, workflow tips, and troubleshooting sections.
- **FR-808**: System MUST provide integration guides for: Claude Code, Cursor, Copilot
  (if MCP support available), and Codex.
- **FR-809**: System MUST provide reference auto-indexing templates including git hooks
  (`post-commit` for sync, `pre-push` for doctor) and project-type-specific
  `.codecompassignore` files.
- **FR-810**: System MUST provide reference configurations for common project types:
  Rust, TypeScript, Python, Go, and multi-language monorepos.
- **FR-811**: System MUST generate changelogs organized by conventional commit type
  (feat, fix, refactor, docs, test, chore) with links to PRs.
- **FR-812**: System MUST include SHA-256 checksums for all release artifacts.

> **Cross-reference**: spec 001 US1 also requires `cargo install codecompass` as a
> source-based installation path. This spec (009) complements that with prebuilt
> binary distribution; the `cargo install` path remains supported but is not gated
> by this spec's release workflow.

### Key Entities

- **ReleaseBinary**: A platform-specific compiled binary with version, target triple,
  checksum, and download URL.
- **HomebrewFormula**: A Ruby formula describing the package, dependencies, installation
  method, and platform-specific download URLs.
- **MCPConfigTemplate**: A JSON/TOML configuration file for a specific AI coding agent
  that enables CodeCompass as an MCP server.
- **IntegrationGuide**: A markdown document with step-by-step instructions for setting
  up CodeCompass with a specific AI coding agent.
- **AutoIndexTemplate**: A collection of configuration files (git hooks, ignore patterns,
  config overrides) for automatic indexing in a specific project type.

## Success Criteria

### Measurable Outcomes

- **SC-801**: A user with no Rust toolchain can install and run CodeCompass on any of
  the 5 target platforms in under 2 minutes.
- **SC-802**: `brew install signalridge/tap/codecompass` succeeds and the installed
  binary passes `codecompass doctor` on macOS.
- **SC-803**: A new user following any integration guide can have CodeCompass working
  with their AI agent within 10 minutes.
- **SC-804**: The Homebrew formula is automatically updated within 1 hour of a new
  GitHub release.
- **SC-805**: All MCP configuration templates pass validation against their respective
  agent's configuration schema.
