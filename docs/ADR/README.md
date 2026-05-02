# NANO Architecture Decision Records (ADRs)

**Version:** 1.5.0  
**Last Updated:** 2026-05-02

---

## What are ADRs?

Architecture Decision Records (ADRs) capture significant architectural decisions made during the development of NANO. Each ADR documents:

1. **Context** — The problem and forces that drove the decision
2. **Decision** — What was decided
3. **Consequences** — The trade-offs, both positive and negative
4. **Alternatives** — Options considered and rejected

ADRs provide institutional knowledge for future developers and prevent repeated debates about settled questions.

---

## Index of ADRs

| ADR | Title | Status | Date |
|-----|-------|--------|------|
| [ADR-001](001-ept-fix.md) | ExternalPointerTable (EPT) SIGSEGV Fix | 🟢 Accepted | 2026-04-19 |
| [ADR-002](002-context-reset.md) | Context Reset for Request Isolation | 🟢 Accepted | 2026-04-19 |
| [ADR-003](003-thread-local-isolates.md) | Thread-Local Isolate Ownership | 🟢 Accepted | 2026-04-19 |
| [ADR-004](004-vfs-architecture.md) | Virtual File System Abstraction | 🟢 Accepted | 2026-04-20 |
| [ADR-005](005-crypto-ring.md) | Rust Crypto Implementation Strategy | 🟢 Accepted | 2026-04-19 |
| [ADR-006](006-sliver-format.md) | Sliver Snapshot Format | 🟢 Accepted | 2026-04-20 |
| [ADR-007](007-esm-strategy.md) | ESM Module Execution Strategy | 🟢 Accepted | 2026-04-21 |

---

## Status Legend

| Symbol | Status | Description |
|--------|--------|-------------|
| 🟢 | Accepted | Current approach — actively used |
| 🟡 | Proposed | Under consideration |
| 🔴 | Superseded | Replaced by newer ADR |

---

## ADR Template

```markdown
# ADR-XXX: [Title]

**Status:** [proposed | accepted | rejected | deprecated | superseded by ADR-YYY]  
**Date:** YYYY-MM-DD  
**Deciders:** [team members]  
**Technical Story:** [link to context if applicable]

## Context and Problem Statement

[Describe the context and problem statement, e.g., in free form using two to three sentences. You may want to articulate the problem in form of a question.]

## Decision Drivers

* [driver 1, e.g., a force, facing concern, or …]
* [driver 2, e.g., a force, facing concern, or …]
* …

## Considered Options

* [option 1]
* [option 2]
* [option 3]
* …

## Decision Outcome

Chosen option: "[option 1]", because [justification. e.g., only option which meets k.o. criterion decision driver | which resolves force force | comes out best (see below)].

### Positive Consequences

* [e.g., improvement of quality attribute satisfaction, follow-up decisions required, …]
* …

### Negative Consequences

* [e.g., compromising quality attribute, follow-up decisions required, …]
* …

## Pros and Cons of the Options

### [option 1]

* Good, because [argument a]
* Good, because [argument b]
* Bad, because [argument c]
* …

### [option 2]

* Good, because [argument a]
* Good, because [argument b]
* Bad, because [argument c]
* …

### [option 3]

* Good, because [argument a]
* Good, because [argument b]
* Bad, because [argument c]
* …

## Links

* [Link type] [Link to ADR]
* …
```

---

## Contributing

When making significant architectural decisions:

1. Create a new ADR following the template
2. Document context, alternatives, and trade-offs
3. Set status to "proposed"
4. Discuss with the team
5. Update status to "accepted" or "rejected"

For decisions that supersede existing ADRs:
1. Mark old ADR as "superseded by ADR-XXX"
2. Reference the old ADR in the new ADR's context

---

*Last updated: 2026-05-02*
