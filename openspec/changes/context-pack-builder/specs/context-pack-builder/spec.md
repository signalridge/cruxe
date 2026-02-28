## ADDED Requirements

### Requirement: System MUST provide budgeted context pack assembly
The system MUST provide a context-pack assembly capability that builds a structured minimal-sufficient context bundle for agent workflows.

Pack sections MUST support:
- `definitions`
- `key_usages`
- `dependencies`
- `tests`
- `config`
- `docs`

#### Scenario: Query returns sectioned context pack
- **WHEN** context pack generation is requested with a valid query and budget
- **THEN** response MUST return a sectioned context pack containing ranked items from one or more supported sections

### Requirement: Context pack MUST enforce deterministic token budgets
Pack assembly MUST enforce explicit token budget limits and deterministic truncation policy.

#### Scenario: Over-budget candidates are dropped deterministically
- **WHEN** candidate set exceeds requested token budget
- **THEN** runtime MUST keep higher-priority items according to deterministic ordering
- **AND** MUST report dropped candidate counts

### Requirement: Pack items MUST include provenance envelope
Each emitted pack item MUST include provenance fields sufficient for audit and verification.

Required fields:
- stable `snippet_id`
- `ref`
- `path`
- `line_start` / `line_end`
- `content_hash`
- `selection_reason`

#### Scenario: Pack item can be traced to source location
- **WHEN** a pack item is returned
- **THEN** client MUST be able to map it back to exact source ref and location via provenance fields

### Requirement: Section assignment MUST be deterministic and single-label
Each emitted snippet MUST be assigned to exactly one section using deterministic rule priority.

Assignment priority:
1. `definitions` (symbol definition spans)
2. `key_usages` (reference/call spans)
3. `dependencies` (imports/includes)
4. `tests` (test path/content heuristics)
5. `config` (manifest/config files)
6. `docs` (documentation sources)

#### Scenario: Snippet with multiple traits uses highest-priority section
- **WHEN** a snippet contains both an import line and a symbol definition span
- **THEN** the snippet MUST be assigned to `definitions` (higher-priority rule)
- **AND** MUST NOT be duplicated across multiple sections

#### Scenario: Test snippet is routed to tests section
- **WHEN** snippet path matches configured test heuristics and no higher-priority rule matched
- **THEN** snippet MUST be assigned to `tests`
