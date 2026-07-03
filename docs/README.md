# backbone-tax — Handbook

> **Reader:** all. **Mode:** navigation.
> The region-neutral **tax engine**: it computes tax *lines* from effective-dated templates and
> withholding categories, and hands them to a caller who attaches them to an `AccountingPost`.
> **Tax never posts to the GL.** Indonesia rates/rules are deferred overlay data — this module
> ships the engine now.

This is the map. Every page below names its reader and its mode; start with the row that matches
why you're here.

## Start by who you are

| You are… | You want… | Read, in order |
|----------|-----------|----------------|
| **Evaluator** | Why this module exists, what it refuses to do | [prd.md](prd.md) → [ADR-001](adr/ADR-001-tax-engine-boundary.md) → [Architecture](architecture.md) |
| **App developer** | Compute tax for an invoice, wire the module into a service | [Developer guide](developer-guide.md) → [Extension guide](extension-guide.md) → [Business flows](business-flows/tax.md) |
| **Maintainer** | Add an entity, change a rate, keep regen safe | [fsd.md](fsd.md) → [Architecture](architecture.md) → [Extension guide](extension-guide.md) → [ADR-002](adr/ADR-002-effective-window-overlap-and-inclusive-reconciliation.md) |
| **Contributor** | Set up, test, and open a correct PR | [Contributing](contributing.md) → [Glossary](glossary.md) |

## The whole handbook

**Explanation — why it is the way it is** *(Evaluator, Maintainer)*
- [prd.md](prd.md) — problem, scope (engine in / Indonesia rules out), success criteria.
- [fsd.md](fsd.md) — entities, endpoints, the engine, the seam, non-goals.
- [architecture.md](architecture.md) — C4 view: context, containers, the DDD 4-layer module, a
  `POST /tax/calculate` traced end-to-end.
- [adr/](adr/) — the load-bearing decisions:
  - [ADR-001](adr/ADR-001-tax-engine-boundary.md) — engine now, rules deferred; tax is called, never posts.
  - [ADR-002](adr/ADR-002-effective-window-overlap-and-inclusive-reconciliation.md) — no overlapping
    effective windows; exact inclusive reconciliation.

**How-to — I have a goal, give me steps** *(App developer, Maintainer)*
- [developer-guide.md](developer-guide.md) — install → quickstart (compute a tax line) → recipes →
  troubleshooting.
- [extension-guide.md](extension-guide.md) — compose the module into a service; the seam contract;
  layering the Indonesia overlay later.

**Reference — the exact shape** *(all)*
- [schema/](schema/) — the schema-YAML DSL reference (types, rules, generation, error codes).
- [business-flows/tax.md](business-flows/tax.md) — the engine's flows in business terms.
- [business-flows/golden-cases.md](business-flows/golden-cases.md) — the numeric oracle (TGC-1…9).
- `../schema/models/*.model.yaml` — **the single source of truth** for every entity.

**Contribution & language** *(Contributor, all)*
- [contributing.md](contributing.md) — dev setup, commit conventions, tests/lint, PR checklist.
- [glossary.md](glossary.md) — one term, one meaning: TaxLine, effective-dating, the seam, inclusive.

## The one thing to internalize

Tax is **upstream** of the general ledger, not part of it. `calculate()` returns
`[TaxLine]`; the producing module (billing/selling/buying) maps each line onto its own
`AccountingPost` and posts *that*. Tax imports no caller; callers import no tax internals. The
contract is `calculate(template, base, date) → [TaxLine]`. Everything in this handbook serves that
boundary — see [ADR-001](adr/ADR-001-tax-engine-boundary.md).
