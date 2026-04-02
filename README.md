# slashmem (`sm`)

A local memory store for AI agents. Slashmem gives agents persistent procedural memory backed by SQLite — rules are learned from experience, gain or lose confidence over time, and surface automatically when relevant.

## Install

```bash
cargo install --path .
```

The binary is called `sm`. Data is stored in `~/.slashmem/mem.db` (override with `SLASHMEM_DIR`).

## Quick start

```bash
# Add a rule from a past incident
sm rules add deploy-safety "Always run tests before deploying" --source postmortem

# After a task succeeds, reinforce the rule
sm ingest --task TASK-42 --body "Deployment succeeded after running full test suite" \
  --agent agent-0 --success deploy-safety

# After a task fails, mark the rule as harmful
sm ingest --task TASK-43 --body "Deploy failed — skipped tests" \
  --agent agent-0 --harm deploy-safety

# Before starting work, pull relevant context
sm context "deploying the payments service"

# Periodically decay stale knowledge and prune weak rules
sm distill

# Check database health
sm status
```

## Commands

### `sm context <description>`

Query memory for relevant context given a task description. Returns proven rules, identified anti-patterns, and recent working memory snippets that match the description via full-text search.

```bash
sm context "refactoring the auth middleware"
```

**Output fields** (JSON):

| Field | Description |
|-------|-------------|
| `relevant_rules` | Proven rules matching the query (confidence > 0.8) |
| `anti_patterns` | Anti-patterns matching the query (failure_count >= 3) |
| `history_snippets` | Recent working memory entries related to the query (up to 10) |

### `sm ingest`

Ingest an episodic record and optionally reinforce or penalize procedural rules.

```bash
sm ingest --task TASK-1 --body "Completed migration" --agent agent-0

# Reinforce one or more rules
sm ingest --task TASK-2 --body "Tests passed" --agent agent-0 \
  --success rule-a --success rule-b

# Penalize a rule
sm ingest --task TASK-3 --body "Cache caused stale reads" --agent agent-0 \
  --harm cache-aggressively

# Both at once
sm ingest --task TASK-4 --body "Mixed results" --agent agent-0 \
  --success rule-a --harm rule-b
```

| Flag | Required | Description |
|------|----------|-------------|
| `--task` | Yes | Task identifier |
| `--body` | Yes | Body text describing what happened |
| `--agent` | Yes | Agent identifier |
| `--success <ID>` | No | Rule ID to reinforce (repeatable) |
| `--harm <ID>` | No | Rule ID to penalize (repeatable) |

**Output fields** (JSON):

| Field | Description |
|-------|-------------|
| `episodic_id` | ID of the created episodic record |
| `proposed_rules` | Auto-proposed rules (reserved for future use) |
| `validated_rules` | Rules that were validated via `--success` / `--harm` |

### `sm distill`

Run confidence decay, maturity transitions, and pruning. Call this periodically (e.g. daily) to keep the knowledge base healthy.

```bash
sm distill
```

**What it does:**

1. **Decay** — recalculates confidence for all rules based on time elapsed since last validation
2. **Transition** — updates proven / anti-pattern flags based on new confidence scores
3. **Prune** — removes rules with confidence below 0.05 (anti-patterns are preserved)

**Output fields** (JSON):

| Field | Description |
|-------|-------------|
| `decayed` | Number of rules whose confidence was recalculated |
| `pruned` | Number of low-confidence rules removed |
| `transitioned` | Number of rules that changed proven/anti-pattern status |

### `sm status`

Show database health and record counts.

```bash
sm status
sm status --json
```

**Output fields** (JSON):

| Field | Description |
|-------|-------------|
| `ok` | `true` if the database is healthy |
| `db_path` | Path to the SQLite database file |
| `counts.episodic` | Number of episodic records |
| `counts.working` | Number of working memory entries |
| `counts.procedural` | Number of procedural rules |
| `schema_version` | Database schema version |

### `sm rules list [--query <QUERY>]`

List all procedural rules, optionally filtered by full-text search.

```bash
sm rules list
sm rules list --query "deploy"
sm rules list --json
```

### `sm rules add <ID> <RULE> [--source <SOURCE>]`

Add a new procedural rule.

```bash
sm rules add no-force-push "Never force-push to main" --source code-review
```

### `sm rules show <ID>`

Show full details of a single rule including confidence, success/failure counts, and timestamps.

```bash
sm rules show deploy-safety
```

### `sm rules rm <ID>`

Remove a procedural rule by ID.

```bash
sm rules rm stale-rule
```

## Global flags

| Flag | Description |
|------|-------------|
| `--json` | Force JSON output (automatic when stdout is not a TTY) |
| `--quiet` | Suppress normal stdout output |
| `-h`, `--help` | Print help |
| `-V`, `--version` | Print version |

## Output modes

- **Human-readable** — default when stdout is a TTY. Compact summaries with section headers and bullet points.
- **JSON (robot mode)** — automatic when stdout is piped or redirected, or when `--json` is passed. All commands produce structured JSON. Errors use a standard envelope:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "Rule 'xyz' not found",
    "suggestions": ["Check that the resource ID exists", "Run: sm rules list to see all known IDs"],
    "exit_code": 1
  }
}
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Not found |
| 2 | Invalid input |
| 3 | Database error |
| 4 | I/O error |
| 5 | Parse error |
| 127 | Internal error |

## Confidence model

Rules gain and lose confidence through a decay-weighted scoring formula:

```
Confidence = (successes - 4 × failures) × 0.5 ^ (days_since_last_validation / 90)
```

- **Half-life**: 90 days — confidence halves every 90 days without validation
- **Harm weight**: 4× — one failure cancels four successes
- **Proven threshold**: confidence > 0.8
- **Anti-pattern threshold**: failure_count >= 3

Anti-patterns are never pruned, even at low confidence — they represent valuable negative knowledge.

## Three memory layers

| Layer | Purpose | Retention |
|-------|---------|-----------|
| **Episodic** | Raw event log — every `ingest` creates a record | Permanent |
| **Working** | Short-term task summaries, full-text searchable | Permanent (searched with recency bias) |
| **Procedural** | Learned rules with confidence scoring | Pruned when confidence < 0.05 (except anti-patterns) |

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `SLASHMEM_DIR` | `~/.slashmem` | Directory for the SQLite database |

---

## Agent integration prompt

Copy the block below into your agent's system prompt or CLAUDE.md to give it access to slashmem:

````markdown
# Memory — slashmem

You have access to `sm`, a local memory store. Use it to persist and retrieve procedural knowledge across sessions.

## Before starting a task

Query for relevant context:
```bash
sm context "<brief description of the task>"
```
Review the returned `relevant_rules` (proven best practices) and `anti_patterns` (known pitfalls) before proceeding. Adjust your approach accordingly.

## After completing a task

Record what happened and reinforce/penalize rules:
```bash
sm ingest --task "<task-id>" --body "<what happened and why>" --agent "<your-agent-id>" \
  [--success <rule-id>...] [--harm <rule-id>...]
```
- Use `--success <rule-id>` for each rule that contributed to a good outcome
- Use `--harm <rule-id>` for each rule that led to a bad outcome or was proven wrong

## When you discover a reusable lesson

Add it as a procedural rule:
```bash
sm rules add "<rule-id>" "<rule text>" --source "<where you learned this>"
```
Choose a short, descriptive kebab-case ID (e.g. `always-run-migrations`, `no-force-push`).

## Periodic maintenance

Run distill to decay stale rules and prune weak ones:
```bash
sm distill
```

## Checking status

```bash
sm status
```

## Managing rules

```bash
sm rules list                    # list all rules
sm rules list --query "deploy"   # search rules
sm rules show <rule-id>          # inspect a rule
sm rules rm <rule-id>            # remove a rule
```

## Output format

All commands output JSON when piped (or with `--json`). Parse output with `jq` or your JSON library. Errors follow this envelope:
```json
{"error": {"code": "...", "message": "...", "suggestions": [...], "exit_code": N}}
```

## Exit codes

0 = success, 1 = not found, 2 = invalid input, 3 = db error, 4 = I/O error, 5 = parse error.
````
