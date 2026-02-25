## ADDED Requirements

### Requirement: Runtime SQLite access MUST use managed connection lifecycle
MCP runtime paths MUST use a shared connection lifecycle abstraction for SQLite
access, rather than opening ad-hoc connections at each call site.

Managed lifecycle behavior MUST include:
- lazy open on first use
- safe reuse for subsequent requests in the same runtime scope
- deterministic reopen behavior after connection failure
- consistent error classification when reconnect attempts fail

#### Scenario: Connection failure triggers deterministic reopen path
- **WHEN** a request path encounters a broken or unavailable SQLite connection
- **THEN** runtime MUST attempt managed reopen and continue with canonical error semantics if reopen fails

#### Scenario: Repeated request path avoids repeated ad-hoc open churn
- **WHEN** multiple tool requests are served in the same runtime process
- **THEN** runtime MUST use the managed lifecycle abstraction instead of opening a new independent connection per handler branch
