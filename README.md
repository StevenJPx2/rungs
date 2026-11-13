# rungs

A small step machine for programs that run a plan of steps, where each step
waits on the outcomes of earlier ones. It has no I/O, no dependencies and no
allocation (`no_std`), and Kani proves that every run ends.

**Status: Planned.** See [the plan](https://github.com/StevenJPx2/rungs/issues/1).

Two projects use it:

- [atelier](https://github.com/StevenJPx2/atelier) runs pipelines such as
  worktree, builder, checks, ready, merge, with steps in parallel, retries up
  to a limit and a restart when the builder pushes.
- [Chauffeur](https://github.com/StevenJPx2/chauffeur) runs strategies such as
  asking Jev, then GLiDE only if Jev was unsure or failed, then a reasoning
  model.

```rust
use rungs::{Need, Outcome, Plan, Run, Step};

// fast, then sure only when fast was unsure (1) or failed
let plan = Plan::new([
    Step::new(2),
    Step::new(2).after([Need::outcome(0, [Outcome(1), Outcome::FAILED])]),
])?;

let mut run = Run::new(&plan);
assert_eq!(run.advance(&plan), [0]);   // start fast

run.settle(&plan, 0, Outcome(1))?;     // unsure
assert_eq!(run.advance(&plan), [1]);   // start sure
```

rungs knows steps only by position and outcomes only by number. Names,
models, prompts, saving state and time stay with the program that uses it.

## License

MIT
