# Tasks: Distribution & Release

**Input**: Design documents from `/specs/009-distribution/`
**Prerequisites**: plan.md (required), spec.md (required), contracts/mcp-distribution.md (required)

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1-US5)
- Include exact file paths in descriptions

## Phase 1: Build Pipeline (cargo-dist + cross)

**Purpose**: Cross-platform binary builds and release workflow

> Execution status (2026-02-27): 39/39 tasks completed in-repo.

- [x] T412 [US1] Initialize cargo-dist configuration: created `dist.toml` with 5 release targets and cargo-dist settings
- [x] T413 [US1] Configure static linking in `dist.toml`: Linux targets use musl; macOS/Windows stay on native target defaults
- [x] T414 [US1] Set up cross-compilation config if needed: created `Cross.toml` with Linux musl/gnu cross images for CI runners
- [x] T415 [US1] Install and configure git-cliff for changelog generation: created `cliff.toml` with conventional commit grouping + PR links
- [x] T416 [US1] Create GitHub Actions release workflow in `.github/workflows/release.yml`: tag-triggered cargo-dist release pipeline with changelog + checksums + GH release publish
- [x] T417 [P] [US1] Create GitHub Actions CI workflow in `.github/workflows/ci.yml`: ensured push/PR coverage, cargo test/clippy/fmt checks, Linux/macOS build matrix, and auto-index hook template test job
- [x] T418 [US1] Test release workflow: created test tags (e.g. `v0.1.0-rc.0-009-test-20260227u`) and verified release workflow success with 5 platform binaries + `.sha256` artifacts + generated release notes
- [x] T419 [P] [US1] Verify static linking: `release-e2e` Linux job extracts musl archive, runs `codecompass --version`, and validates `ldd` output has no unresolved shared libraries

**Checkpoint**: Tag push produces GitHub release with 5 platform binaries + checksums + changelog

---

## Phase 2: Homebrew Tap

**Purpose**: Homebrew distribution for macOS (and Linux Homebrew) users

- [x] T420 [US2] Create Homebrew tap repository: verified `signalridge/homebrew-tap` exists on GitHub and contains initial `README.md`
- [x] T421 [US2] Write Homebrew formula in `Formula/codecompass.rb`: added multi-platform formula template with URL/checksum slots and `codecompass --version` test block
- [x] T422 [US2] Create Homebrew auto-update workflow in `.github/workflows/homebrew-update.yml`: added release/workflow_dispatch updater that opens a PR against `signalridge/homebrew-tap`
- [x] T423 [US2] Test Homebrew formula: CI `homebrew-audit-install` job runs `brew install --build-from-source signalridge/tap/codecompass`, then verifies `codecompass --version` and `codecompass doctor`
- [x] T424 [US2] Run `brew audit --strict Formula/codecompass.rb` and fix any issues: CI `homebrew-audit-install` executes `brew audit --strict signalridge/tap/codecompass` successfully

**Checkpoint**: `brew install signalridge/tap/codecompass` works, auto-updates on release

---

## Phase 3: MCP Configuration Templates

**Purpose**: Ready-to-use config templates for AI coding agents

- [x] T425 [P] [US3] Create Claude Code MCP config template in `configs/mcp/claude-code.json`: `mcp_servers` format with `serve-mcp` command and env placeholders
- [x] T426 [P] [US3] Create Cursor MCP config template in `configs/mcp/cursor.json`: added Cursor `mcpServers` template
- [x] T427 [P] [US3] Create Codex MCP config template in `configs/mcp/codex.json`: added Codex template with timeout + env placeholders
- [x] T428 [P] [US3] Create generic MCP config template in `configs/mcp/generic.json`: added commented generic stdio template
- [x] T429 [US3] Generate JSON schema for all MCP tool definitions in `configs/mcp/tool-schemas.json`: generated directly from MCP `tools/list` via `scripts/generate_mcp_tool_schemas.sh`
- [x] T430 [US3] Write human-readable MCP tool reference in `docs/reference/mcp-tools-schema.md`: documented tool catalog, required fields, examples, and regeneration commands

**Checkpoint**: Config templates work when pasted into each agent's configuration

---

## Phase 4: Agent Integration Guides

**Purpose**: Step-by-step setup guides for each supported AI coding agent

- [x] T431 [P] [US4] Write Claude Code integration guide in `docs/guides/claude-code.md`: includes prerequisites, setup, prompt rule, and troubleshooting
- [x] T432 [P] [US4] Write Cursor integration guide in `docs/guides/cursor.md`: includes setup, verification, usage flow, and troubleshooting
- [x] T433 [P] [US4] Write Copilot integration guide in `docs/guides/copilot.md`: includes current MCP status + CLI fallback workflow
- [x] T434 [P] [US4] Write Codex integration guide in `docs/guides/codex.md`: includes setup, verification, and recommended workflow
- [x] T435 [US4] Write auto-indexing setup guide in `docs/guides/auto-indexing.md`: includes hook install, IDE suggestions, and troubleshooting

**Checkpoint**: Each guide enables a new user to go from zero to working in 10 minutes

---

## Phase 5: Auto-Indexing Templates

**Purpose**: Reference configurations for automatic indexing in common project types

- [x] T436 [P] [US5] Create Rust auto-indexing template in `configs/templates/rust/`: added `.codecompassignore` + fail-soft hook templates
- [x] T437 [P] [US5] Create TypeScript auto-indexing template in `configs/templates/typescript/`: added `.codecompassignore` + fail-soft hook templates
- [x] T438 [P] [US5] Create Python auto-indexing template in `configs/templates/python/`: added `.codecompassignore` + fail-soft hook templates
- [x] T439 [P] [US5] Create Go auto-indexing template in `configs/templates/go/`: added `.codecompassignore` + fail-soft hook templates
- [x] T440 [P] [US5] Create monorepo auto-indexing template in `configs/templates/monorepo/`: added `.codecompassignore` + fail-soft hook templates
- [x] T441 [US5] Add error handling to all git hook templates: hooks log to `~/.codecompass/logs/hook.log` and always exit `0`
- [x] T442 [US5] Write test: added `tests/test_auto_indexing_hook_templates.sh` and wired it into CI

**Checkpoint**: Template git hooks auto-sync the index on commit

---

## Phase 6: Polish & Validation

**Purpose**: End-to-end validation of the full distribution pipeline

- [x] T443 End-to-end test on macOS arm64: downloaded `codecompass-aarch64-apple-darwin.tar.gz`, verified `init + index + search`, and confirmed `serve-mcp` startup log (`MCP server started`)
- [x] T444 [P] End-to-end test on Linux x86_64: validated in `docker run --platform linux/amd64 ubuntu:latest` using release binary `codecompass-x86_64-unknown-linux-musl.tar.gz` with successful `init + index + search`
- [x] T445 [P] End-to-end test on Windows x86_64: `release-e2e` Windows job downloads release zip and verifies `init + index + search` on `windows-2022`
- [x] T446 Verify Homebrew formula: `release-e2e` macOS job verifies `brew install signalridge/tap/codecompass` and `codecompass doctor`
- [x] T447 [P] Validate all MCP config templates: verified all templates map to `codecompass serve-mcp --workspace ${CODECOMPASS_WORKSPACE}` and confirmed tools are listed via `tools/list` (18 tools)
- [x] T448 [P] Proofread all integration guides: reviewed command paths/steps and ensured guide structure consistency
- [x] T449 Validate `configs/mcp/tool-schemas.json` against MCP specification: schema generated from MCP `tools/list`, JSON-validated, and documented with regeneration/validation commands
- [x] T450 [P] Verify changelog generation: created 10 conventional commits in a temp repo, tagged `v0.0.1`, and verified `git-cliff` groups output into `Features`, `Fixes`, and `Documentation`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1** (Build Pipeline): No dependencies - can start immediately
- **Phase 2** (Homebrew): Depends on Phase 1 (release binaries must exist)
- **Phase 3** (MCP Config): Independent of Phases 1-2 (templates are static files)
- **Phase 4** (Integration Guides): Independent of Phases 1-2, but benefits from Phase 3 (references config templates)
- **Phase 5** (Auto-Indexing): Independent of all other phases
- **Phase 6** (Validation): Depends on all phases

### Parallel Opportunities

- Phase 1: T417 and T419 can run in parallel with release workflow development
- Phase 3: All config templates (T425-T428) can be created in parallel
- Phase 4: All integration guides (T431-T434) can be written in parallel
- Phase 5: All project-type templates (T436-T440) can be created in parallel
- Phase 6: T444, T445, T447, T448, T450 can run in parallel
- Phases 3, 4, 5 can all be developed in parallel (independent content)

## Implementation Strategy

### Incremental Delivery

1. Phase 1 -> Release pipeline works (binary distribution available)
2. Phase 2 -> Homebrew tap works (easiest install path)
3. Phase 3 -> MCP config templates ready (agent configuration enabled)
4. Phase 4 -> Integration guides ready (user onboarding complete)
5. Phase 5 -> Auto-indexing templates ready (power user workflow)
6. Phase 6 -> End-to-end validation

### Parallel Work Streams

Three independent work streams can proceed simultaneously:
- **Stream A**: Build pipeline + Homebrew (Phases 1-2)
- **Stream B**: MCP templates + Integration guides (Phases 3-4)
- **Stream C**: Auto-indexing templates (Phase 5)

## Notes

- Total: 39 tasks, 6 phases
- No new Rust code in `crates/` -- this is entirely distribution, documentation, and configuration
- The Homebrew tap is a separate repository (`signalridge/homebrew-tap`)
- Phases 3, 4, 5 can be done entirely in parallel with the build pipeline
- Git hook templates must exit 0 on failure to avoid blocking developer workflow
