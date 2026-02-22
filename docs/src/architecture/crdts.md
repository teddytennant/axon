# CRDTs

Axon includes three conflict-free replicated data types (CRDTs) for eventually-consistent shared state across the mesh.

## GCounter (Grow-only Counter)

Each node maintains its own counter slot. The total value is the sum of all slots.

```rust
let mut counter = GCounter::new();
counter.increment("node1");
counter.increment("node1");
// counter.value() == 2

// Merge from another node
let mut remote = GCounter::new();
remote.increment("node2");
counter.merge(&remote);
// counter.value() == 3
```

**Merge rule**: Take the max of each node's slot.

## LWWRegister (Last-Writer-Wins Register)

A single value where the most recent write wins, using timestamps.

```rust
let mut reg = LWWRegister::new();
reg.set("first", 100);
reg.set("second", 200);
// reg.get() == Some(&"second")

// Merge: higher timestamp wins
let mut remote = LWWRegister::new();
remote.set("remote", 150);
reg.merge(&remote);
// reg.get() == Some(&"second")  // 200 > 150
```

## ORSet (Observed-Remove Set)

A set where concurrent adds and removes are handled correctly: add wins over concurrent remove.

```rust
let mut set: ORSet<String> = ORSet::new();
set.add("node1", "apple".to_string());
set.add("node1", "banana".to_string());
set.remove(&"banana".to_string());
// set.contains("apple") == true
// set.contains("banana") == false
```

**Key property**: If one node adds an element while another removes it, the add wins. Only removes that have *observed* the specific add tag will succeed.

## Properties

All three CRDTs guarantee:

| Property | Meaning |
|----------|---------|
| Commutativity | `merge(a, b) == merge(b, a)` |
| Associativity | `merge(merge(a, b), c) == merge(a, merge(b, c))` |
| Idempotency | `merge(a, a) == a` |

These properties mean nodes can merge state in any order, any number of times, and converge to the same result.
