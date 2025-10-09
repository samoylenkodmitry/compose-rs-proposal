# Recomposition Walkthrough

The diagram below illustrates how slot indices shift when a button click updates
state in a simple counter example. Each box represents a slot table entry with
its index shown above the box.

```
Before Click (initial composition)              After Click (recomposition)

  idx:  0      1      2      3                     0      1      2      3
       ┌───┐ ┌───┐ ┌───┐ ┌───┐                   ┌───┐ ┌───┐ ┌───┐ ┌───┐
       │ G │ │ V │ │ N │ │ G │                   │ G │ │ V │ │ N │ │ G │
       └─┬─┘ └─┬─┘ └─┬─┘ └─┬─┘                   └─┬─┘ └─┬─┘ └─┬─┘ └─┬─┘
         │     │     │     │                       │     │     │     │
         │     │     │     └─ child group          │     │     │     └─ child group reused
         │     │     └─ node id for button         │     │     └─ node id reused, props updated
         │     └─ remembered State<i32>            │     └─ same State<i32> with new value (1)
         └─ root group frame                       └─ root group frame reused
```

The recomposition reuses the same slot indices; only the value stored inside the
state slot (index 1) changes from `0` to `1` after the click.
