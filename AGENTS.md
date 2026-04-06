# AGENTS

## Commit Message Policy
- Always use **detailed conventional commits**.
- Format: `<type>(<scope>): <subject>`
- Allowed `type`: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `revert`.
- Keep `<subject>` imperative and specific (what changed + where), not generic.
- Include a commit body for non-trivial changes with:
  - `Why:` user-visible issue or engineering rationale.
  - `What:` key implementation changes.
  - `Impact:` performance/risk/behavior notes.
- Use `!` and `BREAKING CHANGE:` footer when behavior is intentionally breaking.

## Examples
- `fix(navigation): coalesce rapid arrow presses into single target open`
- `perf(loader): add recursive perf-transition benchmark with percentile output`
- `docs(agents): enforce detailed conventional commit format`

## Disallowed Style
- `Update code`
- `Fix stuff`
- `changes`

## Performance + Build Constraints
- Treat performance as a default requirement for all code in this repository.
- Prefer designs that keep UI interactions feeling immediate under rapid input.
- Avoid adding work on hot paths unless measured and justified.
- Benchmark when changing loading/navigation/decoding behavior.
- Keep runtime memory growth bounded and predictable.
- Optimize for small, efficient release binaries:
  - Avoid unnecessary dependencies.
  - Remove dead code and unused assets.
  - Prefer efficient data structures and algorithms.
  - Validate changes with release builds (`cargo run --release`, `cargo test --release` where practical).
