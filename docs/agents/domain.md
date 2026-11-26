# Domain docs: single-context

One domain, one glossary.

- **`CONTEXT.md`** (repo root) — the glossary. Terms are canonical: code,
  docs and conversation use these words with exactly these meanings.
  Covers the graph framework, script graphs and animation graphs today; add a
  section when a task introduces new domain language (terrain, navigation,
  materials).
- **`docs/adr/`** — architectural decision records in the `NNNN-slug.md`
  format used by `/domain-modeling`. Older decisions (ADR-001 … ADR-016) are
  in `docs/DECISIONS.md`; new ones go in `docs/adr/`.
- **Binding rulings made inside a task** (D1…, R1…, U1…) are recorded in that
  task's plan document and its close-out in `docs/roadmap/COMPLETED.md`, not
  as ADRs, unless they constrain the whole engine.

Consumer rules: read `CONTEXT.md` before naming anything user-visible; when
a term is missing or used two ways, fix the glossary in the same change.
