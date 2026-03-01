# MCP Distribution Contract: Distribution & Release

This contract defines how MCP-related distribution artifacts are produced in
Phase 4 (`009-distribution`) and how they are validated.

> Scope note: this spec introduces **no new MCP tools**. It standardizes
> packaging, template distribution, and schema publication for existing tools.

## Contract Scope

Applies to:

- `configs/mcp/tool-schemas.json`
- `configs/mcp/claude-code.json`
- `configs/mcp/cursor.json`
- `configs/mcp/codex.json`
- `configs/mcp/generic.json`
- `docs/reference/mcp-tools-schema.md`

Out of scope:

- Runtime tool behavior changes (owned by `001`-`008` tool contracts)
- Protocol evolution (owned by feature specs that add/modify tools)

## Requirements Mapping

| Distribution Requirement | Source Spec |
|--------------------------|-------------|
| Publish machine-readable MCP tool schema | FR-805 |
| Ship MCP config templates for target agents | FR-806 |
| Ship integration guides and troubleshooting | FR-807, FR-808 |
| Validate template/schema compatibility | SC-805 |

## Artifact Contract

### 1) Tool Schema Publication

`configs/mcp/tool-schemas.json` MUST:

- be generated from the effective `tools/list` surface of the released binary;
- include every public MCP tool name with input schema;
- include required/optional field constraints as represented by the MCP server;
- be valid JSON and pass schema sanity checks in CI.

### 2) Agent Config Templates

Each template in `configs/mcp/*.json` MUST:

- define one `cruxe` MCP server entry;
- invoke `cruxe serve-mcp` (stdio mode by default);
- include an overridable workspace/project path parameter;
- avoid embedding secrets or host-specific absolute paths.

### 3) Human-Readable Reference

`docs/reference/mcp-tools-schema.md` MUST:

- list all MCP tools with short purpose and expected input shape;
- point to `configs/mcp/tool-schemas.json` as the canonical machine format;
- stay version-aligned with released binaries.

## Validation Rules

CI/release validation MUST verify:

1. `tool-schemas.json` exists and parses as JSON.
2. All template files exist and parse as JSON.
3. Template tool names align with the released MCP `tools/list` surface.
4. Documentation links to MCP schema files are not broken.

## Versioning Rule

When MCP tool schemas change in future specs:

- update `configs/mcp/tool-schemas.json`,
- update affected agent templates,
- update `docs/reference/mcp-tools-schema.md`,
- and note the change in release notes.
