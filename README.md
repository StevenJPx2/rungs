# rungs

A small step machine for programs that run a plan of steps, where each step
waits on the outcomes of earlier ones. It has no I/O, no dependencies and no
allocation (`no_std`), and Kani proves that every run ends.

**Status: Implemented in `v0.1.0`.** See [the plan](https://github.com/StevenJPx2/rungs/issues/1).

```toml
rungs = { git = "https://github.com/StevenJPx2/rungs", tag = "v0.1.0" }
```

Two projects use it:

- [atelier](https://github.com/StevenJPx2/atelier) runs pipelines such as
  worktree, builder, checks, ready, merge, with steps in parallel, retries up
  to a limit and a restart when the builder pushes.
- [Chauffeur](https://github.com/StevenJPx2/chauffeur) runs strategies such as
  asking Jev, then GLiDE only if Jev was unsure or failed, then a reasoning
  model.

```rust
use rungs::{Need, Outcome, Plan, Step};

// fast, then sure only when fast was unsure (1) or failed
let plan = Plan::new([
    Step::new(2),
    Step::new(2).after([Need::outcome(0, [Outcome(1), Outcome::FAILED])]),
])?;

let mut run = plan.start();
assert_eq!(run.advance(), [0]);   // start fast

run.settle(0, Outcome(1))?;       // unsure
assert_eq!(run.advance(), [1]);   // start sure

let saved = run.progress();       // plain Copy data, to save
let run = plan.resume(saved)?;    // checked against the plan
```

rungs knows steps only by position and outcomes only by number. Names,
models, prompts, saving state and time stay with the program that uses it.

## Checking it

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo clippy --target wasm32-unknown-unknown --locked -- -D warnings
cargo nextest run --all-targets --all-features --locked
cargo test --doc --locked
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --locked
cargo kani                                    # 32 harnesses, a few minutes
cargo +nightly-2026-10-12 miri test --locked  # Chauffeur's pinned nightly
cargo build --target wasm32-unknown-unknown --locked
cargo +nightly-2026-08-21 check --all-targets --locked  # Kani 0.68's compiler
cargo mutants
```

The Kani harnesses in `src/proofs.rs` cover every plan of up to four steps,
with up to three outcomes and three attempts each, every progress it can
reach and every call.

## License

MIT
