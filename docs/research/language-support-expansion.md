# Language Support Expansion Research

Date: 2026-07-08

## Question

Two-part question about decombine's language coverage:

1. Do the 12 languages we already claim to support actually extract *good*
   units, or do some of them silently emit mis-named, mis-scoped, or missing
   units because they lean on the default (adapter-less) path?
2. Which new languages are worth adding, in what order, given the real state
   of the `tree-sitter-*` crate ecosystem?

This is research only. No code was changed.

## Method

Read the extraction machinery end to end rather than trusting the summary:

- `src/index/language.rs` (specs, adapters, registry, grammar bindings)
- `src/index/extractor.rs` (query execution, scope recovery, comment strip,
  complexity/size gates)
- every `assets/languages/<id>.toml` + `<id>/units.scm`
- `tests/fixtures/extract/*` (the golden units each language actually emits)
- `src/analyze/duplicate/mod.rs` + `src/analyze/paths.rs` (test/docs
  classification)
- `Cargo.toml` (grammar crate pins, host `tree-sitter` version)

Verified new-language crate existence, latest version, `tree-sitter` version
requirement, and rough maintenance signal against the crates.io API
(2026-07-08).

## How extraction actually works (verified, because the recommendations depend on it)

Four mechanisms decide unit quality. Only the fourth needs Rust code.

1. **`units.scm` decides what is captured.** A node kind not matched by a
   pattern is simply not a unit. This is the dominant source of *missing*
   units.
2. **Scope is declarative, not adapter-only.** `recover_scope` walks ancestors
   and, for each `[[scopes]]` rule `{kind, field}`, appends
   `ancestor.child_by_field_name(field)`, joined with `.`. So a method gets
   `ClassName.method` **without any adapter**, as long as the enclosing node
   kind is listed in the spec. Missing scope is usually a missing `[[scopes]]`
   entry, not a missing adapter.
3. **Comment stripping is by node kind, inside the unit range only.**
   `comment_nodes` are stripped from embedding/hash text. Crucially, a doc
   comment that sits *above* a declaration (`///` Rust, `/** */` Javadoc/PHPDoc,
   `///` C# XML docs, `/** */` C/C++) is a *sibling* outside the `@unit` byte
   range, so it is already excluded from both display and embedding for free.
   Only docstrings that live *inside* the body (Python's leading string
   statement) need an adapter to strip. This is why most languages need no
   docstring adapter.
4. **Adapters (`LanguageAdapter::refine`) fix what queries cannot express:**
   anonymous naming from context, receiver/qualified scopes, in-body docstring
   strip, and `#[cfg(test)]` scope tagging. Four exist: `python`, `js-like`
   (shared by JavaScript **and** TypeScript), `go`, `rust`.

Two more gates, both language-agnostic: `body_node_count_threshold` (default
10 named AST nodes under `@unit.body`) drops trivial units, and
`max_body_chars` (default 10000) drops giant ones. Test/docs classification is
by path (`test/`, `tests/`, `spec/`, `test_*`, `*_test`, `*.test.*`,
`*.spec.*`, `conftest.py`) **or** a scope whose first segment is `test`/`tests`.
Only the Rust adapter produces that scope; every other language relies purely
on path.

### Correction to the premise

TypeScript is **not** adapter-less. `typescript.toml` sets
`adapter = "js-like"`, the same adapter JavaScript uses. So the real count is
**7 languages with no adapter at all**: `c`, `cpp`, `csharp`, `java`,
`kotlin`, `php`, `ruby`. TypeScript is covered below anyway, because sharing
the JS adapter leaves TS-specific gaps.

---

## Part 1 — Quality gaps in the existing 12 languages

Ranked worst-first by how much the gaps distort real corpora. Fixture evidence
is quoted from `tests/fixtures/extract/*.expected` where it exists (it exists
for all 12).

### Tier A — needs an adapter, gaps hit idiomatic code

#### 1. C++ (`cpp`, no adapter) — highest priority

Fixture confirms the lambda gap: `sample.cpp` emits
`closure <anonymous> scope=demo`. Concrete problems:

- **Out-of-line method definitions lose their class.** The query captures the
  `name` field of a `qualified_identifier`, so `void Store::merge() {...}`
  defined in a `.cpp` becomes name `merge` with scope from the *namespace*
  only — the `Store` qualifier is dropped, because out-of-line defs are not
  lexically nested in `class_specifier`. This is the dominant C++ style in real
  code (declaration in header, definition in `.cpp`). An adapter must recover
  scope from the `qualified_identifier` prefix.
- **Lambdas are always `<anonymous>`.** STL-heavy code (`std::for_each`,
  `std::sort` comparators, `std::ranges`) is full of them. Needs call-argument
  / assignment naming like `js-like`.
- **Pointer/reference-returning functions are missed.** The pattern requires
  `declarator: (function_declarator ...)` *directly*; `int* foo() {...}` wraps
  it in a `pointer_declarator`, so it never matches. Same bug in C.
- **Operator overloads and destructors are missed** (`operator_name`,
  `destructor_name` are neither `identifier` nor `field_identifier`).
- Templates are fine (the inner `function_definition` still matches).

Adapter must: recover scope from `qualified_identifier`; name lambdas from
context; and the query must add `pointer_declarator`/`reference_declarator`
unwrapping and `operator_name`/`destructor_name` name captures.

#### 2. Kotlin (`kotlin`, no adapter)

Fixture confirms two `lambda <anonymous>` units from a single `describe`
function. Problems:

- **Trailing lambdas are the language's primary control-flow idiom**
  (`list.map { ... }`, `runBlocking { ... }`, Compose `Column { ... }`). Every
  one is `<anonymous>`, producing high-cardinality noise and weak cluster
  names. Needs a `js-like`-style adapter naming from the enclosing call and
  from `val`/property binding.
- **Extension-function receivers are dropped.** `fun String.slugify()` captures
  name `slugify` with no scope; the `String` receiver — the thing that makes it
  a distinct unit — is lost. Adapter should lift the receiver type into scope.
- Missing scope kinds: `companion_object`, and `interface`/`enum` bodies are
  not in `[[scopes]]` (only `class_declaration`, `object_declaration`).

#### 3. Ruby (`ruby`, no adapter)

Fixture confirms the block gap: in `sample.rb` the method `merge` is captured,
but the `items.each do |k,v| ... end` block inside it and the top-level
`rows.filter_map do |row| ... end` block are **not** captured at all, and the
`->(row)` lambda is `closure <anonymous>`.

- **Blocks (`do_block`, `block`) are Ruby's main unit of logic and are
  entirely uncaptured.** Methods are fine, but a huge fraction of behavior —
  and essentially *all* RSpec test bodies (`describe/context/it do ... end`) —
  lives in blocks. This both under-captures product logic and, worse, means
  RSpec suites contribute almost no units, so test/docs classification and the
  Mixed-cluster false-positive guard get little to work with for Ruby.
- Recommendation: capture `do_block`/`block` above a size threshold, and add a
  `js-like`-style adapter to name them from the enclosing call
  (`it("...")`, `each`, `map`) and from assignment. Lambdas/procs need the
  same naming.

### Tier B — usable, one clear gap each

#### 4. C# (`csharp`, no adapter)

Scopes are good (class/struct/interface/record all covered; fixture shows
`scope=Ledger` on methods, constructors, and local functions). Gaps:

- **Expression-bodied members are missed.** `public int Area() => w * h;` and
  expression-bodied properties use `arrow_expression_clause`, not `block`, so
  the `method_declaration`/`property` patterns don't match. This is pervasive
  in modern C#.
- **Property accessors with bodies** (`get { ... } set { ... }`) are not
  captured.
- Lambdas are `<anonymous>` (LINQ). Needs naming.
- `namespace`/`file_scoped_namespace` not in `[[scopes]]` (minor).
- Note: XML doc comments (`///`) above members are outside the unit range, so
  already excluded — no docstring adapter needed.

#### 5. Java (`java`, no adapter)

Scopes good (class/interface/enum). Main gap:

- **Lambdas are `<anonymous>`** (Java 8+ streams). Needs call/assignment
  naming.
- Anonymous-class methods (`new Runnable() { public void run() {...} }`) are
  captured as `run` but with no scope — reads as a bare `run` with no anchor.
- `record` declarations are not a scope kind (rarely have large bodies, low
  impact).
- Javadoc `/** */` sits above the method, outside the range — already
  excluded, good.

#### 6. PHP (`php`, no adapter)

Scopes good (class/interface/trait). Gaps:

- **Arrow functions `fn($x) => ...` (`arrow_function`) are missed** — common
  in modern PHP array pipelines.
- Closures (`anonymous_function`) are `<anonymous>`; often assigned to a
  variable and nameable.
- `namespace_definition` not a scope kind (minor).
- PHPDoc `/** */` above members already excluded.

#### 7. TypeScript (`js-like` adapter, shared with JS)

The shared adapter handles anonymous naming well. TS-specific residue:

- **`abstract_class_declaration` is not a scope kind** (only
  `class_declaration`), so methods of abstract classes lose their class name.
- `namespace`/`module` blocks (`internal_module`) are not scopes.
- Decorators (`@Component`) sit above the member — already excluded, fine.
- Overload *signatures* have no body and are correctly ignored; the
  implementation is captured. Good.

### Tier C — fine as-is (adapter-less but the default path is correct)

- **C (`c`)** — no classes, no lambdas, so no scope/naming need. One real bug
  shared with C++: **pointer-returning functions (`char *foo()`) are missed**
  because `declarator` is a `pointer_declarator`. Worth a one-line query fix,
  not an adapter. K&R definitions also missed (rare). Function-like macros not
  captured (acceptable).
- **Python, Rust, Go, JavaScript** — already have adapters; not re-audited
  here beyond confirming the fixtures pass.

### Cross-cutting classification gap (not per-language)

Test detection is path-based for every language except Rust. Common test
filenames that our `paths.rs` rules **miss**: Java/C# `FooTest.java`,
`FooTests.cs`, Go is fine (`_test.go` dir/suffix rules catch it), Kotlin
`FooTest.kt`. The stem rules only fire on `test_*`, `*_test`, `test`, `tests`
(underscore-delimited) — camel/Pascal `SomethingTest` slips through and lands
in Product clusters. This is a `TEST_DOC_DIRS`/stem-rule fix in `paths.rs`, not
a language adapter, but it disproportionately hurts the JVM/.NET languages,
which is exactly where we have no `tests`-scope adapter to compensate.

### Ranked adapter backlog

1. **C++ adapter** — qualified-scope recovery + lambda naming (plus query fix
   for pointer/operator/destructor). Biggest correctness win.
2. **Kotlin adapter** — trailing-lambda naming + extension-receiver scope.
3. **Ruby** — query change (capture blocks) + adapter (block/lambda naming).
   Largest *coverage* win; today we barely see Ruby test suites.
4. **C# query change** — expression-bodied members + accessors; lambda naming.
5. **Java / PHP / TS** — mostly lambda/arrow naming and a couple missing scope
   kinds; lower urgency.
6. **`paths.rs`** — PascalCase `*Test`/`*Tests` filename detection.

Most of items 1–5 are the *same* two adapter behaviors we already wrote three
times (call-argument naming, assignment naming, receiver→scope). The
`name_from_call_argument` / `name_from_enclosing_function` helpers in
`language.rs` are already generic over the arguments/string node kinds — a
shared "call-and-binding naming" adapter parameterized by node-kind names
would cover C++, Kotlin, Ruby, C#, Java, and PHP closures with little new code.

---

## Part 2 — What "proper" support means (acceptance checklist)

A language is *properly* supported, not just *listed*, when all of these hold.
Treat it as the PR checklist for any new language.

1. **Grammar crate**: a maintained `tree-sitter-<lang>` on crates.io that
   depends on `tree-sitter >= 0.23` (so it exposes the `LANGUAGE: LanguageFn`
   constant our `bundled!` macro expects) and was published within roughly the
   last 12–18 months.
2. **`units.scm` captures the right node kinds**: all of the language's real
   "unit of behavior" constructs — free functions, methods, constructors,
   closures/lambdas, and any idiom-dominant construct (Ruby blocks, Kotlin
   trailing lambdas). Verify against the grammar's `node-types.json`, not
   guesswork; wrapper declarators (pointer/reference returns) must be unwrapped.
3. **Naming**: no construct that a human would name silently becomes
   `<anonymous>`. Anonymous constructs get a context name (assignment,
   enclosing call, enclosing function) via an adapter.
4. **Scope**: methods display as `Scope.name`. Every class-like / module-like /
   receiver container is either a `[[scopes]]` rule or handled by an adapter
   (out-of-line C++, Kotlin/Go receivers).
5. **Comment/docstring handling**: `comment_nodes` lists every comment node
   kind. In-body docstrings (only Python so far) get an adapter strip;
   above-declaration doc comments need nothing (outside the range).
6. **Test-code classification**: the language's conventional test layout is
   detected — by path if idiomatic (`*_test.go`, `spec/`), otherwise by a
   `tests` scope from an adapter (Rust model). Confirm real suites (RSpec,
   JUnit, xUnit) land in Test/Mixed, not Product.
7. **Threshold sanity**: check that `body_node_count_threshold = 10` is
   reasonable for the language's node density. Terse languages (Ruby, Elixir,
   Lua) pack more behavior per node; verbose ones (Java, Go) fewer. A fixture
   that is "obviously worth flagging" must survive the gate, and a one-liner
   must not.
8. **Golden fixture is mandatory**: `tests/fixtures/extract/sample.<ext>` +
   `.expected` exercising functions, methods, closures, a scope, comment
   stripping, and a below-threshold unit. Regenerate with `UPDATE_GOLDEN=1` and
   eyeball the diff — this is where mis-naming/mis-scoping is caught.
9. **Extension mapping** is unambiguous and doesn't collide with an existing
   language (watch `.h` C-vs-C++, `.rs`, `.kt`/`.kts`).

---

## Part 3 — New languages, prioritized

Crate facts verified via crates.io API on 2026-07-08 (version / last publish /
`tree-sitter` req). Host pins `tree-sitter 0.26.10`; existing grammar crates
already span `0.23`–`0.25` and coexist fine, so any grammar on `tree-sitter
>= 0.23` is ABI-safe with our host.

| Candidate | Crate | Version | Published | ts req | Verdict |
| --- | --- | --- | --- | --- | --- |
| Swift | `tree-sitter-swift` | 0.7.3 | 2026-06 | ^0.23 | **Add now** |
| Bash/Shell | `tree-sitter-bash` | 0.25.1 | 2025-12 | ^0.25 | **Add now** |
| Scala | `tree-sitter-scala` | 0.26.0 | 2026-04 | ^0.26 | **Add now** |
| Lua | `tree-sitter-lua` | 0.5.0 | 2026-02 | ^0.26.3 | **Add now** |
| SQL | `tree-sitter-sequel` | 0.3.11 | 2025-10 | ~0.25 | Add later |
| HCL/Terraform | `tree-sitter-hcl` | 1.1.0 | 2025-05 | ^0.25.3 | Add later |
| Elixir | `tree-sitter-elixir` | 0.3.5 | 2026-03 | ^0.23 | Add later |
| Dart | `tree-sitter-dart` | 0.2.0 | 2026-04 | ^0.26 | Add later |
| Solidity | `tree-sitter-solidity` | 1.2.13 | 2025-08 | ^0.25 | Add later |
| Objective-C | `tree-sitter-objc` | 3.0.2 | 2024-12 | ^0.24 | Add later |
| R | `tree-sitter-r` | 1.3.0 | 2026-06 | ^0.24.7 | Add later |
| OCaml | `tree-sitter-ocaml` | 0.25.0 | 2026-05 | ^0.26 | Add later |
| Nix | `tree-sitter-nix` | 0.3.0 | 2025-07 | >=0.23 | Add later |
| Perl | `tree-sitter-perl` | 1.1.2 | 2025-12 | ^0.26.3 | Skip/low |
| Haskell | `tree-sitter-haskell` | 0.23.1 | 2024-11 | ^0.23 | Skip/low |
| Julia | `tree-sitter-julia` | 0.23.1 | 2024-11 | ^0.24 | Skip/low |
| Zig | `tree-sitter-zig` | 1.1.2 | 2024-12 | ^0.24.5 | Skip/low |
| Groovy | `tree-sitter-groovy` | 0.1.2 | 2024-11 | — | Skip |
| Clojure | `tree-sitter-clojure` | 0.1.0 | 2025-07 | — | Skip |
| Svelte | `tree-sitter-svelte-ng` | 1.0.2 | 2024-09 | ^0.23 | Skip |
| Vue | `tree-sitter-vue` | 0.0.3 | 2022-09 | — | Skip |

### Add now (Tier 1)

- **Swift** — Large iOS/macOS/server-side demand; `tree-sitter-swift 0.7.3` is
  actively maintained (published 2026-06, ~3.3M downloads). Adapter work:
  methods get scope from `class_declaration`/`struct_declaration`/`extension`/
  `enum` (`extension` is the important one — Swift spreads a type across
  extensions much like C++ out-of-line defs). Trailing closures are the
  language idiom, so a `js-like`-style closure-naming adapter is needed.
  `///` doc comments are above the declaration → no docstring adapter.
- **Bash/Shell** — Ubiquitous in every repo (CI, scripts, Dockerfiles adjacent).
  `tree-sitter-bash 0.25.1` is the reference grammar, heavily used (7M+
  downloads). Units are `function_definition`; no scope, no closures — this is
  a Tier-C, adapter-less language like C. Cheapest high-value add. Watch the
  `10`-node threshold: many shell functions are short but still worth flagging.
- **Scala** — Strong JVM/data-engineering demand; `tree-sitter-scala 0.26.0`
  tracks the current tree-sitter line. Needs class/object/trait scopes and
  closure/lambda naming (Scala is lambda-dense). Comparable adapter effort to
  Kotlin.
- **Lua** — Widely embedded (Neovim, games, Redis, nginx/OpenResty).
  `tree-sitter-lua 0.5.0`, current. Units are `function_declaration` /
  `function_definition`; `function foo.bar()` and `function T:method()` need an
  adapter to split the table/scope from the name (analogous to Go receivers).

### Add later (Tier 2 — real demand, more adapter or grammar caveats)

- **SQL** — High demand, but the ecosystem is fragmented. Plain
  `tree-sitter-sql` is abandoned (0.0.2, 2021). `tree-sitter-sequel 0.3.11`
  (the renamed DerekStride grammar) is the maintained option. "Units" are
  fuzzy — CREATE FUNCTION/PROCEDURE bodies are the natural unit; whole
  statements are not. Needs a bespoke `units.scm` and probably a dialect
  decision. Valuable but not a clean fit for the function/closure model.
- **HCL/Terraform** — `tree-sitter-hcl 1.1.0` covers Terraform. Units would be
  `block` constructs (resource/module/variable), not functions; duplicate
  detection across Terraform modules is a genuine use case but the extraction
  model is different enough to design carefully.
- **Elixir** (`0.3.5`), **Dart** (`0.2.0`, Flutter demand), **Solidity**
  (`1.2.13`, strong in web3), **Objective-C** (`3.0.2`, legacy Apple),
  **R** (`1.3.0`), **OCaml** (`0.25.0`), **Nix** (`0.3.0`) — all have
  current, installable crates on `tree-sitter >= 0.23`. Each is a
  straightforward `units.scm` + a scope/closure adapter, gated on demand. Dart
  and Solidity have the strongest current signal of this group.

### Skip / low priority (Tier 3)

- **Perl** (`1.1.2`) and **Haskell** (`0.23.1`) — grammars exist, but low
  demand for *duplicate detection* and Haskell's extraction model (where's the
  "unit"? top-level bindings, but heavy point-free/where-clause nesting) is
  awkward.
- **Julia** (`0.23.1`), **Zig** (`1.1.2`) — niche audiences; revisit on demand.
- **Groovy** (`0.1.2`), **Clojure** (`0.1.0`) — only `0.1.x` grammars, and no
  `tree-sitter` dependency metadata resolved on crates.io (`tree-sitter-groovy`
  / `tree-sitter-clojure` returned no ts requirement), i.e. immature/unclear
  bindings. Clojure's S-expression structure is also a poor fit for the
  function/method/closure model.
- **Svelte / Vue** — `tree-sitter-svelte-ng 1.0.2` exists but Vue is stuck at
  `0.0.3` (2022). Both are multi-language single-file components; the JS/TS
  inside is what we'd actually want, which is an embedded-language extraction
  problem (injections), not a new top-level grammar. Skip until we have an
  injection story.

---

## Part 4 — Risks

- **Grammar version churn / ABI.** Our host is `tree-sitter 0.26.10`; bundled
  grammar crates already range `0.23`–`0.25` and load fine because the
  `LANGUAGE: LanguageFn` ABI is stable across `0.23+`. The real risk is a
  grammar crate that lags on an *old* `tree-sitter` (< 0.23, `fn language()`
  API) — none of the Tier-1/2 candidates do, but re-verify on every add. A
  grammar bumping a major node-name (e.g. `function_declaration` renamed) will
  silently drop units with no compile error; the golden fixture is the only
  guard, so fixtures are mandatory (checklist item 8).
- **Silent capture regressions on grammar upgrade.** Because a non-matching
  node kind is just "not a unit," a grammar update that renames or restructures
  nodes degrades recall without any error. Dependabot-style grammar bumps must
  re-run `cargo test` and eyeball fixture diffs, not auto-merge.
- **Binary size.** Each grammar compiles a large generated `parser.c`
  (hundreds of KB to a few MB of tables) into the binary. Twelve grammars
  already; adding Swift/Scala/Bash/Lua is acceptable, but an open-ended
  long-tail (20+ grammars) will bloat the single static binary and slow cold
  builds. If the count grows past ~20, revisit the `architecture.md` "plugin
  mode" idea (runtime WASM grammars) rather than bundling everything.
- **Query complexity for idiom-dense languages.** Ruby blocks, Kotlin/Swift
  trailing closures, and C++ out-of-line defs need real adapter logic, not just
  a query. Underestimating this ships a language that "works" on a fixture but
  produces `<anonymous>`-spam and mis-scoped clusters on real corpora — the
  same trap the current adapter-less C++/Kotlin/Ruby support already fell into.
- **Threshold portability.** `body_node_count_threshold = 10` was tuned on the
  existing set. Terse languages (Lua, Elixir, Ruby blocks) pack more meaning
  per node and may need a per-language override, or they'll over-drop small but
  meaningful units. This interacts with the CLAUDE.md note that changing
  extraction does not refresh already-indexed units — a threshold change needs
  the `DELETE FROM files; DELETE FROM code_units;` refresh to take effect.
- **Test-classification blind spots widen with each JVM/.NET-style language.**
  Adding Scala/Swift without fixing the PascalCase `*Test`/`*Tests` filename
  rule in `paths.rs` means their test suites land in Product clusters and
  inflate false positives.

---

## Decision

1. **Fix the existing set before adding to it.** The adapter-less C++, Kotlin,
   and Ruby support is the weakest link: out-of-line C++ methods lose scope,
   Kotlin trailing lambdas and Ruby blocks are `<anonymous>`/uncaptured, and
   Ruby test suites barely register. Build one **shared "call + binding + receiver
   naming" adapter** (generalizing the helpers already in `language.rs`) and
   apply it to C++, Kotlin, Ruby, C#, Java, PHP.
2. **Cheap query fixes**: unwrap pointer/reference declarators (C, C++);
   capture C# expression-bodied members and accessors; capture Ruby
   `do_block`/`block`; add missing scope kinds (Kotlin `companion_object`,
   TS `abstract_class_declaration`, C# `namespace`).
3. **`paths.rs`**: add PascalCase `*Test`/`*Tests` stem detection.
4. **Then add Tier 1**: Swift, Bash, Scala, Lua — all have current, ABI-safe
   crates, real demand, and (except Bash) reuse the shared naming adapter.
5. Gate Tier 2 (SQL, HCL, Elixir, Dart, Solidity, ...) on demand and design
   their non-function unit models deliberately. Skip Tier 3.
6. Every language ships with a golden fixture. No fixture, no support.
