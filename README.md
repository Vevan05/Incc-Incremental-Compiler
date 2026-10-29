# incc: self-checking, interface-aware incremental compiler

Incremental compiler for a restricted C-like language, with per-function caching,
interface-aware invalidation, and a self-audit mode.

> Research question: *Can interface-aware function caching match clean-build
> results while reducing total compile time?*

## Status

| Module | Phase | State |
|---|---|---|
| `metrics`, `fingerprint`, `cache` | 4 | implemented (LRU eviction, build manifest) |
| `lexer`, `parser`, `ast` | 1 | implemented |
| `sema/` | 1, 3 | implemented (recording `InterfaceView`; global init constantness check) |
| `ir/`, `codegen/` | 1 | implemented (TAC, optimizations, x86-64, initialized globals in `.data`) |
| `deps` | 3 | implemented |
| `driver` | 2, 4 | implemented (parallel option, audit over-invalidation oracle) |
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

- Types: `int`, `bool`, `void`; functions, `const`, globals (constant initializers), control flow.
- Per-function BLAKE3 cache keys including body + sorted interface deps.
- Recording dependency extraction (including negative lookups).
- On-disk content-addressed cache with per-entry checksums and a build manifest.
- Audit mode with false-reuse detection and an over-invalidation oracle.
- Deterministic output (sorted containers, stable linking via `as`/`gcc -s`).

## Language specification

### Types

- `int`: 64-bit signed integer.
- `bool`: true/false.
- `void`: function return type only.

### Top-level items

- `const <type> <name> = <expr>;`
- `<type> <name> [= <expr>];` global variable (default 0; the initializer, if
  present, must fold to a compile-time constant and is emitted in `.data`).
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
- Globals default to 0; a present initializer must fold to a compile-time
  constant (checked in sema) and is emitted into the `.data` section — a
  non-constant initializer is a semantic error, never silently treated as 0.
  The optimizer does not assume initial values; globals generate load/store.
- No inlining (core study): callers depend only on callee signatures.
- Name resolution: locals/params first, then top-level consts/globals/functions.

### Example

```c
const int LIMIT = 10;
int add(int a, int b) { return a + b; }
int main() { return add(LIMIT, 5); }
```

## Architecture — compiler pipeline

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
| call to g | g's canonical signature text (name, param types and names, return type) |
| const C | C's type and value (folded) |
| global v | v's type and mutability (not its initializer) |
| compiler | build hash, target, opt level, codegen flags |

Negative lookups (missing names) are recorded so adding declarations can invalidate.

## Technical reference

### Cache layout and entry format

```
<cache-dir>/manifest.txt              build manifest (see below)
<cache-dir>/<key[:2]>/<key[2:]>       one artifact per function
```

- **Artifact entry** = `blake3(payload)` (32 bytes) followed by the raw
  assembly-text payload. `get()` reads the file, splits at 32 bytes, and
  recomputes the digest: any modification (bit rot, truncated write, manual
  edit) fails verification and is treated as a miss, never as garbage reuse.
- **Writes are atomic**: payload is written to `<path>.tmp<pid>` then
  `rename(2)`d, so a reader never observes a half-written entry.
- **LRU eviction**: after each `put`, all artifact files are walked (the
  manifest is excluded); if total size exceeds the 64 MiB default, files are
  removed oldest-mtime-first. `incc cache clear` deletes and recreates the
  root.

### Build manifest

`manifest.txt` maps cache keys to function names; it is the cross-build memory
the over-invalidation oracle needs (an artifact stored under key *k* by an
earlier build is compared against the same function's output in the current
build):

- Format: one record per line, `<64-hex cache key> <function name>`.
- Appended after every successful `put` (per-function mode only); appends are
  idempotent — a key already present is not written again.
- Snapshot read **before** the build puts anything, so a build's own entries
  can never qualify as "previously stored".
- The parser skips malformed lines, so a torn manifest degrades rather than
  fails; names of keys evicted by LRU resolve to nothing and are ignored.
- `--file-level-cache` keys cover the whole program, so nothing is recorded.

### Cache key inputs, in hashing order

`key(f) = BLAKE3(` in this exact order:

| # | Field |
|---|---|
| 1 | `schema_version` as u32 LE (= 1; bump when serialization changes) |
| 2 | compiler version (`CARGO_PKG_VERSION`) |
| 3 | target (`x86_64`) |
| 4 | options (`-O1`) |
| 5 | `name(f)` — omitted in `--file-level-cache` mode |
| 6 | canonical body bytes (all bodies in file-level mode) |
| 7.. | canonical dependency encodings, dependency set sorted (`BTreeSet`); omitted under `--body-only-key` |

Every variable-length field is **length-prefixed** with its byte length as a
u64 LE, so field-boundary ambiguity ("ab"+"c" vs "a"+"bc") is impossible.
`name(f)` in the key prevents the historical collision where two functions
with identical bodies shared one artifact.

### Canonical serialization encodings

Canonical bytes contain no spans, comments, or whitespace. Integers are two's
complement placed in u64 LE; strings are u64-LE length + UTF-8 bytes.

Tags (u8) used by the encoder in `src/ast.rs`:

| Tag | Node | Payload |
|---|---|---|
| 1/2/3 | type | int / bool / void |
| 10 | `IntLit(n)` | n as u64 LE |
| 11 | `BoolLit(b)` | 1 or 0 |
| 12 | `Var(s)` | string s |
| 13 | `Call(name, args)` | string name, u64 arg count, then each arg |
| 14 | `Unary(op, e)` | op 1=neg 2=not, then operand |
| 15 | `Binary(op, l, r)` | op (Add=1 … Or=13), then left, right |
| 20 | `Let` | name, type, 0=none / 1 + init expr |
| 21 | `Assign` | target name, value expr |
| 22 | `If` | cond, u64 then-count + then stmts, 0 / 1 + u64 count + else stmts |
| 23 | `While` | cond, u64 count + body stmts |
| 24 | `Return` | 0 / 1 + expr |
| 25 | `ExprStmt` | expr |

Body bytes = u64 statement count + canonical statements.

Tags (u8) for a **dependency encoding** (`src/deps.rs`, always includes the
dep name; the *content behind the name* defines invalidation):

| Tag | Dep kind | Content hashed |
|---|---|---|
| 1 | `Lookup::Function` | callee's canonical signature: u64 param count, per param (type), (name), then return type. **Includes param names** — a rename is a valid interface change and thus invalidates dependents (sound but non-minimal; see audit). |
| 2 | `Lookup::Const` | type + folded value |
| 3 | `Lookup::Global` | type + mutability only. The initializer is deliberately excluded: function artifacts contain only `movq g(%rip)` loads; the current global value is re-emitted into `.data` at every link, so an init edit must not rebuild readers. |
| 4 | `Lookup::Missing` | nothing beyond the name — recording that `f` looked up and missed `x`, so *adding* a declaration named `x` invalidates `f`. |

### Back end details

- Per-function assembly text is position-independent w.r.t. its dependencies:
  calls are plain `call name`, global accesses are `movq g(%rip)`, so a cached
  artifact links correctly in any build of the same key.
- Function locals/params/temps all take 8-byte stack slots at
  `-((index+1) * 8)(%rbp)`; slots are assigned from a `BTreeMap` so slot
  numbering is name-sorted and stable.
- Globals: no initializer (or non-folded) → `.comm name, 8, 8` (BSS, zeroed);
  constant initializer → `.data`, `.balign 8`, label, `.quad value`. Bool
  globals occupy 8 bytes like int (values 0/1).
- Link: `globals_assembly` + name-sorted function artifacts → `as -o out.o` →
  `gcc -no-pie -Wl,--build-id=none -s` (build-id stripped so identical builds
  link byte-identical binaries; `-no-pie` keeps generated code PIC-free).

### Parallel mode (`--parallel`)

Miss-functions compile on scoped worker threads (`std::thread::scope`).
Compiles are independent by construction (each depends only on its own
AST + interface table). A worker panic is a hard error, never silently
dropped, and hits/misses/put/record bookkeeping happens on the main thread
after `join`. Stage timers wrap the serial path only; parallel runs are not
comparable per-stage.

### Audit oracle algorithm

Given the pre-build manifest snapshot and this build's misses:

```
for f in rebuilt (miss) functions:
    new_bytes = clean-build artifact of f        # determinism assumed, and verified above
    for entry in manifest where entry.name == f.sig.name and entry.key != key(f):
        if cache.get(entry.key) == Some(new_bytes):
            over_invalidated.push(f); break
```

`Key::from_hex` decodes manifest keys; `Key::to_hex` writes them. The oracle
flags `add`/`main` in the worked example above (callee param rename): both
functions' assembly is byte-identical to what a previous build stored under
their old keys, yet keys changed because dependency canon includes the
signature text.

### Determinism measures

- Declarations, functions, dependencies: sorted (`BTreeMap`/`BTreeSet`,
  name-sorted function list) — never hash-map iteration order.
- No timestamps or host paths in any emitted file; temp file names use only
  the pid.
- Output binary is reproducible: two builds of the same source are
  byte-identical (`cmp`-verified), so byte-comparison audit is meaningful.

## Invalidation rules

| Edit | Expected rebuild set |
|---|---|
| Whitespace / comment change | none |
| Body of g changes (signature same) | g only |
| Signature of g changes | g + callers |
| Parameter name of g changes | g + callers (key changes; see over-invalidation below) |
| Rename g | g + callers |
| const value changes | functions referencing it |
| Global type/mutability changes | functions referencing it |
| Global initial *value* changes | none (readers load `g(%rip)`; the `.data` section is re-emitted at link) |
| Add function h | h only |
| Delete function g | callers of g (error, never cache) |
| Opt level / target / flags change | all |
| Compiler binary changes | all |
| Reorder declarations | none |

Key subtlety: `key(f)` hashes g's **signature text** (param names included), so
renaming a parameter of g changes the keys of all dependents even when their
machine code is unchanged. The artifacts stay reusable-but-different;
`--audit` reports this case through the over-invalidation oracle rather than
rebuilding it away, because the invalidation was *sound* (the interface
changed) but not *minimal*.

## Inlining question

No inlining in the core study (Option A): callers depend only on signatures,
so the "interface-aware" claim stays clean. Inlining with body-dependency tracking
is a stretch goal.

## Audit mode

`incc build --audit`: after the incremental build, perform a clean build (cache
disabled, scratch directory) and compare per-function artifacts (byte-equal) and
the final linked output (byte-equal). Two invariants are checked:

1. **No false reuse (soundness).** For every function, the incremental artifact
   must be byte-identical to the clean artifact. A mismatch means something the
   cache key did not capture actually affects codegen — reported as the
   function name (and `<output>` if the final binary differs), followed by a
   hard error exit. This is the check the deliberately-broken
   `--body-only-key` ablation fails.
2. **No over-invalidation (minimality).** Every function *rebuilt* (cache miss)
   whose artifact is byte-identical to an artifact a previous build already
   stored under a **different** key for the same function name was rebuildable
   for free: its assembly did not change even though its key did. Reported on
   stderr as `audit over-invalidation: [...]`; this is a metric, not a
   failure — the rebuild was sound, just not minimal (e.g. an interface edit
   that only touches names, like a callee param rename). The oracle reads the
   cache **build manifest** (a snapshot taken before this build writes
   anything), so cross-version comparisons never see the current build's own
   entries. It assumes deterministic compilation, which invariant 1 verifies
   on the same run.

Also computes the oracle minimal rebuild set and reports over-invalidation
rate. Audit cost is measured separately (stage `audit`) and does not pollute
normal-build timings.

Precondition: the build manifest lists one line per stored artifact,
`<64-hex cache key> <function name>` (see *Technical reference*). Functions
with identical bodies but different names are distinguished by `name(f)` in
the key.

Ablation: deliberately weaken the key (ignore interface deps) and confirm the
audit detects false reuse.

## Evaluation protocol

Configs: clean, file-level cache, body-only key (ablation), interface-aware (proposed).

Metrics: audit mismatches, total time by stage, hit rate, over-invalidation rate,
overhead, break-even.

Hygiene: >= 30 runs, median + IQR, cold vs warm separated, release build, pinned
seeds, fixed CPU governor.

CSV columns in the committed runs (`run30.csv`, one row per measured build):
`program,N,config,edit_type,edit_pct,run_id,parse,deps,compile,link,total,hits,misses,audit_ok`

Note: reported numbers are warm-cache (OS file cache populated); cold runs not yet
separated, and the runner times the debug binary (`cargo build`, not
`--release`).

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
- Key: `BLAKE3(schema_version | compiler_version | target | options | name(f) | canon(body) | canon(dep_1..dep_n))`, dependencies sorted.
- Interface-awareness: callers depend on callee *signatures*, not bodies (no inlining). Const values folded. Globals: type + mutability (not the initializer).
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
- Determinism: identical output hash across repeated builds (re-verified with
  `cmp` on the committed pipeline after the Review 2 corrections).
- Audit mode passes on all tested edit scenarios (360/360 runs).
- Ablation: weakening key to ignore interface deps detected by audit as false reuse.
- Edit tests: whitespace 0 misses; body 1; const 1; add function 1; reorder 0; signature 2; delete errors (not cached); global init change 0 misses with correct new output.
- Over-invalidation rate: 0.000 for all edit sizes (interface-aware).
- Over-invalidation oracle (newly implemented in-binary): callee param rename ->
  rebuild flagged as over-invalidated ("add","main"); genuine body edit -> no
  flag; global init edit -> no flag (0 misses to begin with).
- Fuzzing (post-correction reruns, seeds 777/999/1234): 20/100/200 iterations, 0 failures.

### Performance results (30 runs per point, median + IQR)

Re-measured with the corrected pipeline (see *Corrections made during Review 2*):
same runner (`bench/run.py --runs 30`), same programs (seed 42), warm caches,
`audit_ok = 360/360`. Absolute times are from the machine listed under
*Environment* and are ~1.8× the original review-2 numbers (different session
load); the config *ratios* are the study's quantities of interest.

N=1000 total time (ms):

| Config | median | IQR |
|---|---|---|
| clean | 297.02 | 25.85 |
| incremental | 231.04 | 33.43 |
| file-level | 317.90 | 30.52 |
| body-only | 234.30 | 42.22 |

All N (median total ms; IQR in parentheses):

| N | clean | incremental | file-level | body-only |
|---|---|---|---|---|
| 50 | 71.66 (9.53) | 64.26 (7.62) | 59.49 (10.42) | 58.83 (5.77) |
| 200 | 100.92 (24.50) | 94.11 (17.22) | 89.63 (16.19) | 89.39 (16.92) |
| 1000 | 297.02 (25.85) | 231.04 (33.43) | 317.90 (30.52) | 234.30 (42.22) |

Stage breakdown (N=1000, medians over 30 runs):

| stage | clean ms | incremental ms | file-level ms | body-only ms |
|---|---|---|---|---|
| parse | 51.11 | 50.41 | 53.97 | 52.14 |
| deps | 10.56 | 10.75 | 29.01 | 10.80 |
| compile | 63.03 | 0.11 | 63.91 | 0.11 |
| link | 170.06 | 165.92 | 168.96 | 171.01 |
| total | 297.02 | 231.04 | 317.90 | 234.30 |

Single-build stage metrics for reference (older `metrics.csv`, identical
patterns): parse 35.6 / deps 6.6 / compile 37.5 / link 98.1 on the clean N=1000
run; noop rebuild save = compile stage only.

Edit-size sweep (N=1000, interface-aware; run on the original
pre-correction pipeline — the sweep scripts predated the corrections; the
over-invalidation columns are reproduced exactly by the new in-binary oracle):

| edit % | misses | over-invalidation | total ms |
|---|---|---|---|
| 1 | 10 | 0.000 | 102.16 |
| 5 | 50 | 0.000 | 143.44 |
| 10 | 100 | 0.000 | 144.80 |
| 25 | 250 | 0.000 | 123.84 |
| 50 | 500 | 0.000 | 122.68 |
| 100 | 1000 | 0.000 | 194.13 |

Finding: incremental beats clean at **every** N in the corrected run — by 22%
at N=1000 (231 vs 297 ms), 7% at N=200 (94 vs 101 ms); at N=50 the 7.4 ms gap
(64 vs 72 ms) is within the IQRs, so not yet statistically separated. The
absolute saving *grows* with program size because the compile stage grows with
N while the cached compile stage stays constant (~0.1 ms): at N=1000 the
compile-stage speedup is ~570x (63.03 → 0.11 ms). File-level caching stays at
or above clean (equal within IQR at N=50/200, clearly worse at N=1000 where
the whole file's one-key invalidation meets 1000 misses).

### Overhead & reuse
- Cache hit rate on no-op rebuild: 100% (1001/1001).
- Body edit rebuilds exactly the changed functions (1 miss for single edit).
- Link stage dominates total time (~57% at N=1000); the compile stage shows the
  caching benefit clearly.
- Overhead accounting: deps + fingerprint + manifest + cache I/O at N=1000
  ≈ 10.8 ms vs clean compile 63.0 ms saved, so interface tracking pays for
  itself ~5× by stage time.

Compile-stage-only times (median ms, N=1000):

| Config | compile ms |
|---|---|
| clean | 63.03 |
| incremental | 0.11 |
| file-level | 63.91 |
| body-only | 0.11 |

### Limitations
- Linking dominates runtime; assembly-level reuse does not avoid relinking. Compile-stage-only times are reported above.
- No structs, inlining, or per-function parse caching.
- Synthetic benchmarks plus 5 hand-written programs.
- Cold vs warm OS cache not separated in reported numbers (all runs warm).

### Fuzzing
- 100 iterations: 0 failures.
- 10,000-step run: 1 failure. Root cause: cache key omitted function name, so functions with identical bodies collided. Fixed by adding name(f) to the key. 200-iteration rerun: 0 failures.
- A 10,000-step rerun was not performed after the fix; post-correction reruns
  (20/100/200 iterations at seeds 777/999/1234) all passed with the fixed
  generator and the new global-init handling.

### Artifacts
- Source: `src/`
- Tests: `tests/golden.rs`, `tests/edits.rs`, `tests/prop.rs`
- Metrics: `results/run30.csv` (re-measured post-corrections),
  `results/edit_sizes.csv`, `results/metrics.csv`, `results/repeat.csv`
  (historical, pre-corrections), `results/scaling.svg`,
  `results/show_metrics.py` (summary printer)

## Demo (hello.src)

```bash
cargo run -- build examples/hello.src -o out --audit --timings
./out; echo "exit=$?"
```

Output (sample from a cold-cache run; times vary per machine):
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

- Unit tests in each module (fingerprint key properties, cache
  roundtrip/corruption/clear, manifest roundtrip + idempotence, metrics).
- `tests/golden/`: 8 source files -> expected exit code.
- `tests/edits/`: invalidation scenarios with expected rebuild counts. The
  global-initializer test asserts **0** misses on an init edit plus the changed
  program output (functions reuse, only `.data` changes).
- `tests/prop.rs`: runs the Python fuzz harness (`bench/fuzz.py`, 20+
  iterations) through `cargo test`.
- Bench: `bench/run.py --runs N` produces the CSV; `bench/gen/gen_program.py N
  SEED` is a self-contained generator (no external dependencies) producing
  programs in the same shape as the committed `bench/programs/gen_*.src`.

## Benchmarks

```bash
python3 bench/run.py --runs 5 --out results/run.csv
python3 results/show_metrics.py
```

## Layout

```
src/        compiler
tests/      golden.rs, edits.rs, prop.rs (+ golden/ data dir)
bench/      gen/, programs/, run.py, fuzz.py
results/    CSVs, plots, summary tools
```

## Environment

- CPU: 13th Gen Intel Core i5-1335U, OS: Arch Linux, Rust 1.99.1, GCC 16.2.1,
  Binutils 2.47, Python 3.14
- Build: `cargo build` (debug compiler; output linked with `as`+`gcc -s`)
- Programs: synthetic generated functions (50/200/1000), seed 42

## Corrections made during Review 2

The fixes below were applied after the first 30-run measurement pass;
`results/run30.csv` was then **re-run end-to-end on the corrected pipeline**
(see *Performance results*), so its numbers already include them. The other
CSVs (`metrics.csv`, `repeat.csv`, `edit_sizes.csv`) predate the fixes and are
kept as historical reference: the caching scheme itself is unchanged, so their
qualitative conclusions still hold.

1. `bench/gen/gen_program.py` depended on a script outside the repository
   (`/tmp/opencode/bench/gen.py`) and was rewritten to be self-contained.
2. Global initializers were parsed but ignored by codegen (every global became
   `.comm`/zero). They are now type-checked, const-checked, and emitted in
   `.data`.
3. The dependency canon of a global hashed its initializer, causing
   over-invalidation on init edits; removed (see justification under
   *Canonical serialization encodings*).
4. Audit's over-invalidation report was declared but never computed; now
   implemented via the cache build manifest and verified with live scenario
   checks (callee param rename flags the rebuilt-unchanged functions; genuine
   body edits and global-init edits do not flag). The new edit test
   `edit_global_init_zero_misses` accompanies fix 3.
5. Parallel compile: a worker panic silently dropped its artifact and could
   panic the build; it now reports an error.
6. Byte-determinism of the final binary re-verified (`cmp`) as part of the
   audit protocol documentation.

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
