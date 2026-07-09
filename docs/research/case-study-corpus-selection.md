# Case-study corpus selection

Date: 2026-07-08

## Question

Before building the full case-study publishing pipeline, which repositories
should we study per supported language?

The corpus needs two properties that are in tension:

- It should contain recognizable, popular open-source projects so results are
  credible to readers.
- It should also contain awkward, fast-moving, AI-assisted, or "vibe-coded"
  projects so decombine is tested against codebases with less conventional
  structure.

This note chooses the initial corpus and the follow-up stress buckets. It is a
companion to [case-study-pipeline.md](case-study-pipeline.md), which covers
how to run, publish, and narrate the reports.

## Method

1. Start from the languages currently supported by local adapters:
   `c`, `cpp`, `csharp`, `go`, `javascript`, `java`, `kotlin`, `php`,
   `python`, `ruby`, `rust`, and `typescript`.
2. Query GitHub repository metadata with:

   ```sh
   gh api "repos/<owner>/<repo>" \
     --jq '[.full_name,.language,(.license.spdx_id // "NOASSERTION"),.stargazers_count,.size,.pushed_at,.html_url] | @tsv'
   ```

   The snapshot below was taken on 2026-07-08. Stars, size, and activity are
   screening signals only; the exact target entry must pin a commit SHA before
   a real run.
3. Prefer permissive licenses for any report we may publish. Repositories with
   `NOASSERTION`, source-available licenses, GPL/AGPL, or ambiguous mixed
   licensing are not public excerpt targets until manually cleared at the
   pinned commit.
4. Choose one "breadth" target per supported language first, then add a few
   deliberately messy stress targets.

## Selection principles

### Breadth first

The first public matrix should be one repository per supported language. That
gets parser coverage, report rendering, and LLM narration across the entire
language set before we spend a week on a single huge repo.

### Prefer recognizable but not pathological

The best first target is popular enough that readers know it, but not so huge
that every run is dominated by infrastructure, vendored code, generated code,
or timeouts. For very large ecosystems, keep a large "scale stress" alternate
instead of making it the default.

### Use domain parallels intentionally

Web frameworks are useful because several languages have a canonical framework
or HTTP library. Similar domains across languages give us a way to compare
whether decombine finds equivalent repeated shapes:

- request routing
- middleware
- serializers
- validation
- error handling
- test helpers

This is more useful than choosing twelve unrelated projects solely by star
count.

### Include libraries and applications

Duplicate detection behaves differently in a compact library than in a full
application:

- Libraries expose repeated overloads, builders, codecs, trait/interface
  families, and test matrices.
- Applications expose feature slices, controllers, adapters, command handlers,
  and generated or vendor-adjacent code.

The first matrix should skew toward libraries/frameworks because they run
cheaply. The second wave should add app-scale stress.

### Treat "vibe-coded" as a stress label, not a moral label

Do not publish a repo under a "bad code" framing unless the repository itself
explicitly describes the code that way. For the site, use a neutral section
name such as **AI-assisted / rapid-build stress cases**. Internally, these are
valuable because they may stress naming consistency, dead code, duplicated
feature scaffolds, generated-looking modules, and shallow abstractions.

## Recommended first public matrix

This is the recommended one-per-language breadth run.

| Language | Target | License signal | Why this one | What decombine should learn |
| --- | --- | --- | --- | --- |
| C | [valkey-io/valkey](https://github.com/valkey-io/valkey) | BSD-3-Clause | Publicly usable Redis-family corpus without current Redis licensing ambiguity. Popular enough, server-scale, real C. | State-machine false positives, command families, protocol helpers, cross-file duplication in a large C service. |
| C++ | [fmtlib/fmt](https://github.com/fmtlib/fmt) | MIT | Compact, popular C++ library with templates and formatting logic. | Template-heavy idioms, overload families, test matrices, small-library signal quality. |
| C# | [PowerShell/PowerShell](https://github.com/PowerShell/PowerShell) | MIT | Recognizable real application, C# primary, active, moderately large. | Command/cmdlet families, provider abstractions, naming consistency across a large CLI-oriented app. |
| Go | [gin-gonic/gin](https://github.com/gin-gonic/gin) | MIT | Already in the oss-eval corpus; popular Go web framework. | Router/middleware idioms, binder/render families, test helper noise. |
| JavaScript | [expressjs/express](https://github.com/expressjs/express) | MIT | Already in the oss-eval corpus; canonical JS web framework, manageable size. | Middleware repetition, callback conventions, legacy JS naming and tests. |
| Java | [spring-projects/spring-boot](https://github.com/spring-projects/spring-boot) | Apache-2.0 | Canonical Java framework; large but valuable. | Annotation-heavy code, configuration families, test slices, package-level duplication. |
| Kotlin | [square/okhttp](https://github.com/square/okhttp) | Apache-2.0 | Popular Kotlin-first HTTP client, substantial but not absurd. | Builder APIs, platform abstraction, protocol codecs, Kotlin/Java mixed boundaries. |
| PHP | [laravel/framework](https://github.com/laravel/framework) | MIT | Canonical PHP framework with high recognition. | Facades, service providers, validation/routing/controller patterns, test scaffolds. |
| Python | [pallets/flask](https://github.com/pallets/flask) | BSD-3-Clause | Already in oss-eval; compact, well-known Python framework. | Decorator-heavy routing, test helpers, app/context idioms. |
| Ruby | [rails/rails](https://github.com/rails/rails) | MIT | Canonical Ruby framework and a strong Rails-convention stress test. | Convention-over-configuration families, DSL-style code, repeated tests and adapters. |
| Rust | [BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) | Unlicense | Already in oss-eval; popular, compact, CLI-oriented Rust. | Trait/impl idioms, cfg(test) classification, command/test duplication. |
| TypeScript | [microsoft/TypeScript](https://github.com/microsoft/TypeScript) | Apache-2.0 | The canonical TypeScript compiler and a self-hosted codebase. | AST/compiler-pass families, naming consistency, large-file behavior, generated/baseline-test boundaries. |

## Metadata snapshot for first-matrix candidates

Snapshot from GitHub API on 2026-07-08:

| Repository | Primary language | License | Stars | Size KiB | Last push |
| --- | --- | --- | ---: | ---: | --- |
| [valkey-io/valkey](https://github.com/valkey-io/valkey) | C | BSD-3-Clause | 26,462 | 198,809 | 2026-07-08 |
| [fmtlib/fmt](https://github.com/fmtlib/fmt) | C++ | MIT | 23,648 | 17,285 | 2026-07-05 |
| [PowerShell/PowerShell](https://github.com/PowerShell/PowerShell) | C# | MIT | 54,304 | 104,502 | 2026-07-08 |
| [gin-gonic/gin](https://github.com/gin-gonic/gin) | Go | MIT | 88,870 | 10,600 | 2026-06-26 |
| [expressjs/express](https://github.com/expressjs/express) | JavaScript | MIT | 69,265 | 9,822 | 2026-07-06 |
| [spring-projects/spring-boot](https://github.com/spring-projects/spring-boot) | Java | Apache-2.0 | 81,086 | 216,419 | 2026-07-07 |
| [square/okhttp](https://github.com/square/okhttp) | Kotlin | Apache-2.0 | 46,994 | 66,945 | 2026-07-08 |
| [laravel/framework](https://github.com/laravel/framework) | PHP | MIT | 34,792 | 101,904 | 2026-07-08 |
| [pallets/flask](https://github.com/pallets/flask) | Python | BSD-3-Clause | 71,860 | 12,008 | 2026-06-10 |
| [rails/rails](https://github.com/rails/rails) | Ruby | MIT | 58,681 | 285,421 | 2026-07-08 |
| [BurntSushi/ripgrep](https://github.com/BurntSushi/ripgrep) | Rust | Unlicense | 65,925 | 5,609 | 2026-07-08 |
| [microsoft/TypeScript](https://github.com/microsoft/TypeScript) | TypeScript | Apache-2.0 | 109,526 | 2,960,582 | 2026-07-08 |

## Scale alternates

These are useful after the first breadth matrix is green. Do not put all of
them in the first run.

| Language | Target | License signal | Use when | Risk |
| --- | --- | --- | --- | --- |
| C | [curl/curl](https://github.com/curl/curl) | `NOASSERTION` from API; curl license requires manual clearance | Need a network-protocol C corpus with deep portability logic. | GitHub license metadata is not a simple SPDX allowlist result; manually clear before publishing excerpts. |
| C | [redis/redis](https://github.com/redis/redis) | `NOASSERTION` from API on 2026-07-08 | Internal continuity with existing oss-eval data or a pinned BSD-era commit. | Do not publish current Redis excerpts without legal/license review. Prefer Valkey for public C. |
| C++ | [microsoft/terminal](https://github.com/microsoft/terminal) | MIT | Need app-scale C++ with UI, platform, and terminal-emulation code. | Large, mixed-language, likely build/test/generated-code noise. |
| C# | [dotnet/runtime](https://github.com/dotnet/runtime) | MIT | Need very large C# runtime/library corpus. | Huge repo, mixed C/C++/C#, generated code, likely too expensive early. |
| Python | [django/django](https://github.com/django/django) | BSD-3-Clause | Need a larger Python web-framework comparison after Flask. | Large test surface; framework conventions may dominate. |
| Rust | [tokio-rs/tokio](https://github.com/tokio-rs/tokio) | MIT | Need async/runtime idioms after ripgrep. | Trait and macro patterns may produce many benign families. |
| TypeScript | [microsoft/vscode](https://github.com/microsoft/vscode) | MIT | Need app-scale TypeScript. | Huge, generated/bundled areas, many extensions, likely needs excludes. |

## AI-assisted / rapid-build stress cases

This bucket is not the first public corpus. It is a deliberate stress suite
for "messy but interesting" code. Run it after the breadth matrix so we know
which failures are corpus-specific rather than parser/reporting bugs.

| Target | Language signal | License signal | Why include it | First-run caveat |
| --- | --- | --- | --- | --- |
| [openclaw/openclaw](https://github.com/openclaw/openclaw) | TypeScript | GitHub API returned `NOASSERTION`; repository [LICENSE](https://github.com/openclaw/openclaw/blob/main/LICENSE) is MIT | Extremely popular AI-agent codebase; current README describes a personal assistant with live Canvas and cross-device channels. It is the obvious "OpenClaw or something" stress target. | Manually verify license at pinned SHA because API metadata did not classify it. Also exclude unsupported Swift/native directories unless adapters exist. |
| [prodlint/prodlint](https://github.com/prodlint/prodlint) | TypeScript | MIT | Tiny tool explicitly aimed at production readiness for vibe-coded apps. Good for quick signal on naming/security-rule duplication. | Very small; useful as a smoke/stress target, not a credibility anchor. |
| [asg017/pdf-lib-rs](https://github.com/asg017/pdf-lib-rs) | Rust | GitHub API returned Apache-2.0; search result shows Apache-2.0 and MIT license files | README explicitly describes it as a vibe-coded Rust port. Good for "AI-assisted port" detection and naming/functionality divergence. | Tiny and low-star; use only in the stress bucket. Confirm dual-license files at pinned SHA. |
| [SamurAIGPT/Vibe-Workflow](https://github.com/SamurAIGPT/Vibe-Workflow) | JavaScript | MIT | Small, explicitly branded AI workflow builder. Good for detecting repeated generated UI/workflow scaffolding. | Very small GitHub-reported size; may not contain enough code units. |
| [AnandChowdhary/vibe-coding](https://github.com/AnandChowdhary/vibe-coding) | JavaScript | MIT | Explicitly a collection of vibe-coding scripts. Good for one-off-script duplication and naming mismatch experiments. | Probably too small for the duplicate report; better for name/code consistency experiments. |
| [ai-ecoverse/vibe-coded-badge-action](https://github.com/ai-ecoverse/vibe-coded-badge-action) | Shell | MIT | Explicitly about measuring vibe-coded commit percentage. Interesting project, but not analyzable with current adapters. | Exclude until shell support exists, or use only as a documentation/process reference. |

## Metadata snapshot for AI-assisted stress candidates

Snapshot from GitHub API on 2026-07-08:

| Repository | Primary language | License | Stars | Size KiB | Last push |
| --- | --- | --- | ---: | ---: | --- |
| [openclaw/openclaw](https://github.com/openclaw/openclaw) | TypeScript | NOASSERTION | 382,214 | 1,697,938 | 2026-07-08 |
| [prodlint/prodlint](https://github.com/prodlint/prodlint) | TypeScript | MIT | 15 | 355 | 2026-07-05 |
| [ai-ecoverse/vibe-coded-badge-action](https://github.com/ai-ecoverse/vibe-coded-badge-action) | Shell | MIT | 6 | 115 | 2026-06-23 |
| [asg017/pdf-lib-rs](https://github.com/asg017/pdf-lib-rs) | Rust | Apache-2.0 | 2 | 164 | 2026-03-08 |
| [SamurAIGPT/Vibe-Workflow](https://github.com/SamurAIGPT/Vibe-Workflow) | JavaScript | MIT | 491 | 95 | 2026-06-12 |
| [AnandChowdhary/vibe-coding](https://github.com/AnandChowdhary/vibe-coding) | JavaScript | MIT | 7 | 31 | 2025-06-18 |

## Why not just choose the biggest repo per language?

Because the first goal is to validate decombine's public case-study machinery,
not win a scale contest. Huge repos are useful, but they hide basic issues:

- license allowlisting
- parser gaps
- generated-file exclusion
- vendored dependency exclusion
- report size limits
- LLM narrative grounding
- site navigation and search

The first public matrix should get all twelve adapters exercised at manageable
cost. Then the scale alternates can expose runtime and report-volume problems.

## Expected findings by language family

### Web framework family

Targets: Gin, Express, Flask, Laravel, Rails, Spring Boot.

Expected signals:

- route registration and middleware duplication
- request/response object handling
- validation helpers
- controller/action test scaffolds
- false positives from conventional lifecycle hooks

This is the best cross-language family for a narrative site because readers
can compare similar product shapes across language ecosystems.

### Systems/runtime family

Targets: Valkey, ripgrep, PowerShell, TypeScript, OkHttp, fmt.

Expected signals:

- protocol and parser families
- command dispatch and option handling
- formatter/encoder/decoder axes
- state-machine false positives
- test-matrix repetition

This family is better for proving that decombine is not only a web-framework
toy.

### AI-assisted stress family

Targets: OpenClaw, prodlint, pdf-lib-rs, Vibe-Workflow, vibe-coding scripts.

Expected signals:

- duplicated feature scaffolds
- inconsistent names for similar functions
- functions whose names overpromise relative to their bodies
- repeated generated-looking glue
- many tiny files with shallow abstractions
- unsupported-language pockets that need clean exclusion

This family should pair with the name/code embedding research: it is likely to
show naming divergence and inconsistent terminology more strongly than classic
well-maintained projects.

## Rollout recommendation

### Phase 0: use existing oss-eval reports internally

Use the already-cloned Flask, Express, Gin, and ripgrep reports to validate
site generation and LLM narration quickly.

Do not publish current Redis-derived excerpts until the license issue is
resolved. For public C, use Valkey or a manually cleared pinned Redis commit
from the permissive-license era.

### Phase 1: breadth corpus

Run the one-per-language matrix:

1. Valkey
2. fmt
3. PowerShell
4. Gin
5. Express
6. Spring Boot
7. OkHttp
8. Laravel
9. Flask
10. Rails
11. ripgrep
12. TypeScript

Each target entry should record:

- repository URL
- pinned commit SHA
- SPDX license at that commit
- enabled language(s)
- include/exclude globs
- rationale tag: `breadth`, `web-framework`, `systems`, `library`, or
  `app-scale`

### Phase 2: scale alternates

Add one or two scale alternates only after the first matrix runs cleanly:

- VS Code for TypeScript app scale
- Django for larger Python framework scale
- Microsoft Terminal for C++ app scale
- Tokio for Rust async/runtime idioms

### Phase 3: AI-assisted stress suite

Add OpenClaw first, because it is the most visible target and large enough to
produce meaningful reports. Then add tiny stress cases to test name/code
embedding and query workflows cheaply:

- prodlint
- pdf-lib-rs
- Vibe-Workflow
- Anand Chowdhary's vibe-coding scripts

This phase should explicitly compare duplicate-analysis output with the
name/code consistency ideas:

- Do misleading-name candidates correlate with duplicate clusters?
- Do AI-assisted repos have more near-duplicate feature scaffolds?
- Are terminology axes less coherent than in mature frameworks?
- Does query-by-example over "agent", "skill", "workflow", "provider",
  "decoder", or "router" expose code organization problems?

## Guardrails before cloning

1. **License must be verified at the pinned commit.** GitHub API metadata is a
   screening signal, not the final authority.
2. **Do not publish excerpts from `NOASSERTION` repos** until the actual
   license file is manually checked and recorded.
3. **Exclude generated, vendored, bundled, fixture, and build-output trees**
   before indexing. This is especially important for VS Code, TypeScript,
   Rails, Spring Boot, OpenClaw, and PowerShell.
4. **Record exact commit SHAs**, not branches or tags, in `targets.toml`.
5. **Keep tiny vibe-coded repos in the stress bucket.** They are useful for
   edge cases, but they should not define the public credibility of the tool.
6. **Report unsupported-language pockets honestly.** OpenClaw appears
   TypeScript-primary but may contain Swift or native app code; run only the
   supported adapters and record what was skipped.
7. **Use Valkey, not current Redis, for the public C slot** unless a specific
   Redis commit is manually cleared.

## Decision

Adopt a three-tier corpus plan:

1. **Internal bootstrap:** existing Flask, Express, Gin, ripgrep reports, plus
   no-publication Redis if useful for continuity.
2. **Public breadth matrix:** Valkey, fmt, PowerShell, Gin, Express, Spring
   Boot, OkHttp, Laravel, Flask, Rails, ripgrep, TypeScript.
3. **Stress suites:** scale alternates and AI-assisted rapid-build targets,
   starting with OpenClaw after license/manual-exclude review.

This gives decombine a defensible public demo while preserving the weird
corpora needed to test future embedding-driven exploration ideas.
