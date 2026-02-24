## Why

`specs/001` 到 `specs/003` 已经统一为新的协议契约（canonical metadata enums、`compact`、`ranking_explain_level`、`truncated` 等），但当前 runtime 仍保留部分旧行为。现在不对齐会让 agent 看到“文档正确、运行不一致”的状态，影响稳定性与可预期性。

## What Changes

- 将 runtime 的 `indexing_status` 与 `result_completeness` 输出对齐到 001-003 文档中的 canonical 枚举（`idle` → `ready`、`partial_available` → `ready`；新增 `not_indexed`、`failed`、`truncated`），并保留对旧值的兼容读取路径。
- 将 `search_code` / `locate_symbol` 的 explainability 配置迁移为 `ranking_explain_level`（`off|basic|full`），保留 legacy `debug.ranking_reasons` 的兼容兜底。
- 完成 002 中已标注但未落地的 FR-105b/FR-105c：结果去重、payload safety limit、`truncated` 语义与 deterministic `suggested_next_actions`。
- 对齐 002 的 `compact` 输入与序列化行为，并确保 003 工具继续按 token budget 方案运行（不引入新的 `compact` 参数）。
- 增补测试，覆盖协议字段、兼容行为与 payload 限流行为，确保文档与实现持续一致。

## Capabilities

### New Capabilities

无。本次不新增 capability，聚焦已存在规范的 runtime 对齐。

### Modified Capabilities

- `001-core-mvp`: Protocol v1 metadata 的 canonical 枚举与兼容映射要求落地到实际输出。
- `002-agent-protocol`: `compact`、`ranking_explain_level`、去重与 payload safety 相关 REQUIREMENTS 从文档约束变为运行时保证。
- `003-structure-nav`: 结构化工具返回的 metadata completeness 语义与 001/002 保持一致，不改变其“无 `compact` 参数”的阶段边界。

## Impact

- Affected code: `codecompass-core`（types/config）、`codecompass-query`（detail/ranking/search）、`codecompass-mcp`（protocol/tool handlers/schema）、`codecompass-cli`（serve-mcp 配置入口）。
- API impact: MCP tool input/output contract 兼容升级（新增字段与枚举值规范化，保留 legacy 兼容）。
- Test impact: 增加契约一致性与回归用例，更新与 001-003 对应的实现任务追踪。
