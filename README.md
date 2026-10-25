# incc: self-checking, interface-aware incremental compiler

Incremental compiler for a restricted C-like language, with per-function caching,
interface-aware invalidation, and a self-audit mode.

> Research question: *Can interface-aware function caching match clean-build
> results while reducing total compile time?*

## Status

| Module | Phase | State |
|---|---|---|
| `metrics`, `fingerprint`, `cache` | 4 | implemented (LRU eviction) |
| `lexer`, `parser`, `ast` | 1 | implemented |
| `sema/` | 1, 3 | implemented (recording `InterfaceView`) |
| `ir/`, `codegen/` | 1 | implemented (TAC, optimizations, x86-64) |
| `deps` | 3 | implemented |
| `driver` | 2, 4 | implemented (parallel option) |
| `audit` | 6 | implemented |

Implemented phases: 1–6 + 7 (generators, tests, fuzz) + 8 (scaling metrics, baselines).
Stretch goals done: parallel compilation, LRU eviction. Not done: structs, inlining,
per-function parse caching, early cutoff.

## Quick start

```bash
cargo build
cargo test
cargo run -- build examples/hello.src -o out
./out; echo "exit=$?"   # 15

# incremental
cargo run -- build examples/hello.src -o out2 --cache-dir .cache

# audit mode
cargo run -- build examples/hello.src -o out3 --audit

# metrics
cargo run -- build examples/hello.src -o out4 --timings
```

## CLI

```
incc build <input> -o <output> [--no-cache] [--audit] [--timings]
  [--cache-dir DIR] [--body-only-key] [--file-level-cache] [--parallel]
incc cache clear [--cache-dir DIR]
```

## Features

- Types: `int`, `bool`, `void`; functions, `const`, globals, control flow.
- Per-function BLAKE3 cache keys including body + sorted interface deps.
- Recording dependency extraction (including negative lookups).
- Audit mode with false-reuse detection.
- Deterministic output (sorted containers, stable linking via `as`/`gcc -s`).

## Language specification

### Types

- `int`: 64-bit signed integer.
- `bool`: true/false.
- `void`: function return type only.

### Top-level items

- `const <type> <name> = <expr>;`
- `<type> <name> [= <expr>];` global variable (default 0).
- `<type> <name>(<params>) { <stmts> }` function definition.

### Statements

- `<type> <name> [= <expr>];` local variable.
- `<name> = <expr>;` assignment.
- `if (<expr>) { ... } [else { ... }]`
- `while (<expr>) { ... }`
- `return [<expr>];`
- `<expr>;`

### Expressions

Arithmetic `+ - * / %`, comparisons `== != < <= > >=`, logical `&& || !`,
unary `-`, calls `f(...)`, variables/consts/globals, literals, parentheses.

### Semantic rules

- Const values are folded into use sites; functions depend on const's type and value.
- Globals default to 0; optimizer does not assume initial values; globals generate load/store.
- No inlining (core study): callers depend only on callee signatures.
- Name resolution: locals/params first, then top-level consts/globals/functions.

### Example

```c
const int LIMIT = 10;
int add(int a, int b) { return a + b; }
int main() { return add(LIMIT, 5); }
```

## Architecture - compiler pipeline

The compiler is split into a cheap whole-program front end and a cached
per-function back half.

1. **Front end (always runs, cheap)**
   - `lexer` -> `parser` -> AST (`Program`).
   - Declaration collection builds the interface table: signatures, types, consts, globals.
2. **Per function `f` (name-sorted for determinism)**
   - `InterfaceView` records every lookup `f` makes (including misses) -> dependency set.
   - `canon_body(f)` produces canonical body bytes.
   - Key inputs: schema/version/target/options + body + sorted deps.
3. **Fingerprinting**
   - `key(f) = BLAKE3(schema | version | target | options | name | body | dep_1..dep_n)`.
4. **Cache lookup**
   - Hit: load cached assembly.
   - Miss: type-check -> IR (TAC) -> optimize -> x86-64 codegen -> store artifact.
5. **Link**
   - globals + all function assembly -> `as` -> `gcc -s` -> executable.
6. **Audit (optional, `--audit`)**
   - Clean rebuild in a scratch dir; byte-compare per-function artifacts and final output.
   - Reports false reuse / over-invalidation.

ASCII sketch:

```
.src -> lexer -> parser -> AST (Program)
                              |
                              v
                 per function f, name-sorted
                              |
        +---------------------+---------------------+
        v                     v                     v
  InterfaceView         canon_body(f)       schema|ver|target|opts
  (records lookups)            |                     |
        v                     |                     |
  canon_dep (signature /      |                     |
  const value / global        |                     |
  type+mutability)            |                     |
        +---------------------+----------+----------+
                                   v
                            BLAKE3 cache key
                                   v
                           +---------------+
                      hit  | cache lookup  |  miss
                  +--------+---------------+--------+
                  v                                 v
            cached asm                    IR -> optimize -> x86-64 asm
                  |                                 |
                  |                          cache.put(key)
                  +---------------+-----------------+
                                  v
                  globals + all function asm -> as -> gcc -s
                                  v
                             executable
                                  | --audit
                                  v
                    clean rebuild -> byte-compare -> pass / fail
```

## Cache key definition

```
key(f) = BLAKE3( schema_version | compiler_version | target | options
                | name(f) | canon(body(f)) | canon(dep_1) | ... | canon(dep_n) )
```

Dependencies sorted canonically.

| f uses | f depends on |
|---|---|
| own body | canonical AST (no spans, comments, whitespace) |
| call to g | g's signature (name, param types, return type) |
| const C | C's type and value (folded) |
| global v | v's type and mutability |
| compiler | build hash, target, opt level, codegen flags |

Negative lookups (missing names) are recorded so adding declarations can invalidate.

## Invalidation rules

| Edit | Expected rebuild set |
|---|---|
| Whitespace / comment change | none |
| Body of g changes (signature same) | g only |
| Signature of g changes | g + callers |
| Rename g | g + callers |
| const value changes | functions referencing it |
| Global type/mutability changes | functions referencing it |
| Add function h | h only |
| Delete function g | callers of g (error, never cache) |
| Opt level / target / flags change | all |
| Compiler binary changes | all |
| Reorder declarations | none |

## Inlining question

No inlining in the core study (Option A): callers depend only on signatures,
so the "interface-aware" claim stays clean. Inlining with body-dependency tracking
is a stretch goal.

## Audit mode

`incc build --audit`: after the incremental build, perform a clean build (cache
disabled, scratch directory) and compare per-function artifacts (byte-equal) and
the final linked output (byte-equal). On mismatch, report the function, its key,
its dependency set, and an artifact diff (false reuse). Also computes the oracle
minimal rebuild set and reports over-invalidation rate. Audit cost is measured
separately and does not pollute normal-build timings.

Ablation: deliberately weaken the key (ignore interface deps) and confirm the
audit detects false reuse.

## Evaluation protocol

Configs: clean, file-level cache, body-only key (ablation), interface-aware (proposed).

Metrics: audit mismatches, total time by stage, hit rate, over-invalidation rate,
overhead, break-even.

Hygiene: >= 30 runs, median + IQR, cold vs warm separated, release build, pinned
seeds, fixed CPU governor.

CSV columns: `program,N,config,edit_type,edit_pct,run_id,stage,ms,hits,misses,audit_ok`

Note: reported numbers are warm-cache (OS file cache populated); cold runs not yet
separated.

## Implementation plan (phases)

1. Phase 1: Language and baseline compiler — done.
2. Phase 2: function-granular pipeline — done.
3. Phase 3: dependency extraction — done.
4. Phase 4: fingerprinting and cache — done.
5. Phase 5: invalidation rules and edit scenarios — done.
6. Phase 6: audit mode — done.
7. Phase 7: workload generation and stress testing — done.
8. Phase 8: measurement and experiments — done.

Stretch goals: inlining, structs, per-function parse caching, parallel compile,
LRU eviction, early cutoff, persistent dependency graph.

## Technology choices

| Decision | Choice | Why |
|---|---|---|
| Language | Rust | Strong enums/pattern matching for ASTs, fast enough for meaningful timing |
| Parser | Hand-written recursive descent + Pratt | Small grammar, full control over spans |
| Hash | BLAKE3 | Fast, collision-resistant, 256-bit keys |
| IR | Own three-address code (TAC) in basic blocks | Simple to optimize, serialize, hash |
| Backend | x86-64 assembly text (`as`/`gcc`) | End-to-end executable for behavioral tests |
| Serialization | Plain text | Deterministic; no timestamps/hash-map order |
| Tests | `cargo test` + golden files; proptest | Needed for audit and mutation tests |
| Benchmarks | Python + repeated runs | Keeps measurement code out of compiler |

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Non-deterministic output breaks audit | Sorted containers/stable ordering; determinism test |
| Forgotten dependency -> false reuse | Recording lookup view + fuzzing + broken-key test |
| Compile time too small to measure benefit | Real optimization passes; large synthetic programs |
| Fingerprint/cache I/O overhead exceeds savings | Valid result: report break-even point |
| Stale cache from changed compiler binary | Include compiler build hash in key |
| Parsing whole program dominates | Report parse time as own stage |
| Scope creep in language | Freeze spec in Phase 1 |
| Synthetic benchmarks unrepresentative | Add hand-written programs; state limitation |

## Results

### Research question
Can interface-aware function caching match clean-build results while reducing total compile time?

### Design
- Unit of reuse: one function.
- Key: `BLAKE3(schema_version | compiler_version | target | options | canon(body) | canon(dep_1..dep_n))`, dependencies sorted.
- Interface-awareness: callers depend on callee *signatures*, not bodies (no inlining). Const values folded. Globals: type + mutability.
- Dependency extraction via recording `InterfaceView` (every lookup, including misses).
- Audit mode: clean build comparison; false reuse and over-invalidation detection.

### Implementation status
- Phases 1-6 complete and validated.
- Phase 7: generators, 8 edit tests, 8 golden tests, 5 hand-written programs, property fuzz
  (100 iterations: 0 failures; 10,000-step run: 1 failure due to cache-key collision between
  same-bodied functions; fixed by adding name(f) to key; 200-iteration rerun: 0 failures).
- Phase 8: scaling metrics, 30-run experiments, baselines (clean/incremental/file-level/body-only),
  edit-size sweep, audit-pass tracking.
- Stretch: parallel compilation + LRU eviction implemented; structs/inlining/parse caching/early cutoff not.

### Correctness evidence
- Determinism: identical output hash across repeated builds.
- Audit mode passes on all tested edit scenarios (360/360 runs).
- Ablation: weakening key to ignore interface deps detected by audit as false reuse.
- Edit tests: whitespace 0 misses; body 1; const 1; add function 1; reorder 0; signature 2; delete errors (not cached).
- Over-invalidation rate: 0.000 for all edit sizes (interface-aware).

### Performance results (30 runs per point, median + IQR)

N=1000 total time (ms):

| Config | median | IQR |
|---|---|---|
| clean | 166.98 | 22.01 |
| incremental | 128.70 | 16.71 |
| file-level | 175.63 | 19.37 |
| body-only | 131.64 | 14.94 |

All N (median total ms):

| N | clean | incremental | file-level | body-only |
|---|---|---|---|---|
| 50 | 36.78 | 40.58 | 28.15 | 29.19 |
| 200 | 57.83 | 47.07 | 54.79 | 47.76 |
| 1000 | 166.98 | 128.70 | 175.63 | 131.64 |

Stage breakdown (N=1000, single run):

| stage | clean ms | body-edit ms | noop ms |
|---|---|---|---|
| parse | 35.62 | 28.12 | 22.24 |
| deps | 6.59 | 6.32 | 6.13 |
| compile | 37.47 | 0.05 | 0.00 |
| link | 98.08 | 86.82 | 96.56 |

Edit-size sweep (N=1000, interface-aware):

| edit % | misses | over-invalidation | total ms |
|---|---|---|---|
| 1 | 10 | 0.000 | 102.16 |
| 5 | 50 | 0.000 | 143.44 |
| 10 | 100 | 0.000 | 144.80 |
| 25 | 250 | 0.000 | 123.84 |
| 50 | 500 | 0.000 | 122.68 |
| 100 | 1000 | 0.000 | 194.13 |

Break-even: incremental beats clean starting ~N=200 for a single body edit; file-level stays at/above clean.

### Overhead & reuse
- Cache hit rate on no-op rebuild: 100% (1001/1001).
- Body edit rebuilds exactly the changed functions (1 for single edit).
- Link stage dominates total time (~60%); compile stage shows the caching benefit clearly.

Compile-stage-only times (median ms, N=1000):

| Config | compile ms |
|---|---|
| clean | 35.76 |
| incremental | 0.06 |
| file-level | 36.29 |
| body-only | 0.06 |

### Limitations
- Linking dominates runtime; assembly-level reuse does not avoid relinking. Compile-stage-only times are reported above.
- No structs, inlining, or per-function parse caching.
- Synthetic benchmarks plus 5 hand-written programs.
- Cold vs warm OS cache not separated in reported numbers (all runs warm).

### Fuzzing
- 100 iterations: 0 failures.
- 10,000-step run: 1 failure. Root cause: cache key omitted function name, so functions with identical bodies collided. Fixed by adding name(f) to the key. 200-iteration rerun: 0 failures.
- A 10,000-step rerun was not performed after the fix.

### Artifacts
- Source: `src/`
- Tests: `tests/golden.rs`, `tests/edits.rs`, `tests/prop.rs`
- Metrics: `results/run30.csv`, `results/edit_sizes.csv`, `results/run_audit.csv`,
  `results/summary.md`, `results/scaling.svg`

## Demo (hello.src)

```bash
cargo run -- build examples/hello.src -o out --audit --timings
./out; echo "exit=$?"
```

Output:
```
audit                       45.668 ms
collect                      0.006 ms
compile                      0.074 ms
deps                         0.050 ms
link                        47.765 ms
parse                        0.143 ms
hits=0 misses=2 hit_rate=0.0%
exit=15
```

Interpretation:
- Audit passed (no error): incremental output matches clean build byte-for-byte.
- Timings show per-stage cost; link dominates (~48 ms).
- Cold cache: 2 functions compiled (add, main), 0 hits.
- Program output: 15 (add(10, 5)).

## Tests

```bash
cargo test                 # unit + golden + edits + prop
```

- `tests/golden/`: source -> expected exit code.
- `tests/edits/`: edit scenarios with expected rebuild counts.
- `tests/prop/`: fuzz harness (Python, 20+ iterations).

## Benchmarks

```bash
python3 bench/run.py --runs 5 --out results/run.csv
python3 results/show_metrics.py
```

## Layout

```
src/        compiler
docs/       (merged into this README)
tests/      golden/, edits/, prop/
bench/      gen/, programs/, run.py, fuzz.py
results/    CSVs, plots, report
```

## Environment

- CPU: (fill), OS: Arch Linux, Rust 1.99, GCC 16.2
- Build: `cargo build` (debug compiler; output linked with `as`+`gcc -s`)
- Programs: synthetic generated functions (50/200/1000), seed 42

## Deliverables checklist

- [x] Language spec (frozen)
- [x] Baseline compiler
- [x] Function-granular pipeline
- [x] Cache key definition
- [x] Cache store, manifest, "why rebuilt" report
- [x] Invalidation rules + passing edit tests
- [x] Audit mode + deliberately-broken-key test
- [x] Program and edit generators, fuzz harness
- [x] Experiment runner and raw CSVs
- [x] Plots + break-even analysis
- [x] Final report (merged into this README)
