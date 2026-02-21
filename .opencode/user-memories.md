# Supermemory Export

**Exported:** 2026-02-24T18:36:41.946Z
**Scope:** user

---

## User Memories (43)

### [pRyfEoH9QJMhfHwkBa7Me6] (preference)

[USER] JJ recovery preference reaffirmed (2026-02-24): user wants recovery assumptions to be JJ-native and explicit—agents should not claim work is lost until JJ operation history has been checked. Preferred protocol is `jj op log` plus `jj op restore <op-id>` (and op-scoped file extraction when needed), with language that reflects JJ's persistence model instead of git-style loss assumptions. This should be treated as an execution guardrail in planning and status updates.

*Created: 2026-02-24T07:41:44.260Z*

### [mo3JNrqCZHccfr3rZkECf2] (preference)

[USER] Planning preference (2026-02-24): user wants iterative back-and-forth plan refinement instead of a one-shot finalization, especially for V6 coaching signal design. They prefer starting with fewer, high-quality primitives and improving through repeated discussion cycles, rather than introducing many detectors at once. This preference complements their existing signal-over-prose priority and should shape plan granularity decisions.

*Created: 2026-02-24T06:49:32.247Z*

### [D3KXo6JQrVLcWavpkEDzWo] (preference)

[USER] Preference reaffirmed in V6 planning (2026-02-23): prioritize signal quality and actionable detection outputs before prose. The user prefers machine-readable insight tags/metrics first and considers natural-language explanation optional/deferred because they can interpret the signals themselves. Scope planning should therefore allocate effort to detector accuracy, calibration, and verification ahead of narrative generation.

*Created: 2026-02-24T04:53:20.047Z*

### [e6YW6Yu7MTC9dvcj18zxo8] (error-solution)

[USER] error-solution: opencode-scheduler@1.3.0 has a Linux-only bug in cronToSystemdCalendars() at dist/index.js line 12985. When cron day-of-week is * (wildcard), it generates invalid OnCalendar syntax `* *-*-* HH:MM:00` — systemd rejects the leading `* ` prefix. Fix: when dowValue === "*", omit the prefix entirely so output is `*-*-* HH:MM:00`. Mac (launchd) and Windows (Task Scheduler) paths are unaffected. Plugin source at ~/.bun/install/cache/opencode-scheduler@1.3.0@@@1/dist/index.js.

*Created: 2026-02-23T15:36:34.374Z*

### [ky58NYAGU6vx8cqAA2RCjN] (project-config)

[USER] project-config: Tavily API key is stored in ~/.env as TAVILY_API_KEY and loaded by opencode MCP config via {env:TAVILY_API_KEY}. Key was rotated on 2026-02-23, and OpenCode restart is required because MCP servers read env at startup. Student plan budget is 4000 credits per month, so free tools should be preferred before Tavily spend.

*Created: 2026-02-23T15:20:00.424Z*

### [ZD3GJXuUx4MAg6VLcuP9BQ] (preference)

[USER] Routing: All Gemini models routed through Anthropic protocol (/v1) via @ai-sdk/anthropic — NOT through /v1beta Google protocol (which returns 400 errors). Antigravity Manager proxies this.

*Created: 2026-02-23T09:52:56.455Z*

### [aZLpaM4E6iFnTMiXKkKe5x] (learned-pattern)

[USER] OpenCode Plugin Management: NEVER manually run `npm install` or `bun install` in ~/.config/opencode/. OpenCode manages plugin installation automatically from the `plugin` array in opencode.json. Manual package manager invocations create a package.json/bun.lock that conflicts with OpenCode's own dependency resolution, causing plugins to fail loading (e.g., OMO showing as "dist" with zero features). Fix: delete any manually-created package.json, package-lock.json, bun.lock, and node_modules/ from ~/.config/opencode/ — OpenCode will reinstall everything cleanly on restart.

*Created: 2026-02-23T06:11:56.310Z*

### [gL6h62441c1et12DExpViv] (learned-pattern)

[USER] BASELINE CONTEXT SIZE: When a fresh conversation loads with 6 MCPs active (augment-context-engine, sourcegraph, tavily, context7, agent-browser, playwright), the starting context is ~51k tokens before any user work begins. This is the baseline overhead from system prompt + MCP tool definitions + supermemory injection + JJ primer + ultrawork mode + AGENTS.md.

*Created: 2026-02-23T04:47:08.725Z*

### [7HB79Dwwk71qbHxNGvFUoG] (preference)

[USER] Environment preference: Antigravity-Manager build/test verification should be run in Windows-native environment; Linux/WSL system dependency failures are not authoritative for this tool.

*Created: 2026-02-20T22:35:57.896Z*

### [Cknv4AQB9JDkJRsQ5SHs5t] (preference)

[USER] Paths: When commands require path-dependent steps, user wants absolute paths provided in instructions.

*Created: 2026-02-20T21:52:00.556Z*

### [NbmWDsneWQaktEdSAjwzRr] (learned-pattern)

[USER] Learned Mistakes: (1) Never declare "root cause found" before running tests/build — say "Hypothesis: X" and verify with evidence before claiming fixed. (2) Background/no-reply notifications (opencode-pty, etc.) must always propagate originating agent/model in session.prompt({ noReply: true }) — omitting causes stored message with default model, flipping UI to base Sisyphus.

*Created: 2026-02-20T04:10:03.966Z*

### [EanW2c6YBkRfcRQDwPBRuX] (preference)

[USER] Workflow: Before broad aggressive dead-code refactors, create a commit snapshot of current work first.

*Created: 2026-02-19T23:02:55.791Z*

### [J96byhsVc9zDDVmq1anm4v] (preference)

[USER] Memory Initialization Workflow: For codebase memory initialization, ask upfront for must-follow rules and communication preference updates, then export existing project memories and perform deep research with incremental memory saves before final reflection.

*Created: 2026-02-18T22:27:59.978Z*

### [uv5dDwJqRvw3oz4GdgVSEA] (preference)

[USER] SPED 117 assignment delivery preference: For design-heavy submissions (e.g., disability arts flyer), user is open to HTML-first output that can be exported to PDF, and prefers higher artistic polish over plain text blocks.

*Created: 2026-02-18T21:50:55.182Z*

### [UcttRHPXrqvpvtK8ZAiNY6] (preference)

[USER] Workflow: For replay corpus collection/qualification, do not use automation scripts; run commands manually line-by-line via CLI.

*Created: 2026-02-18T21:01:07.484Z*

### [BLFpgQhP336koPJefJNnZd] (preference)

[USER] VCS command preference: use JJ status/reporting commands (e.g., `jj status`) and avoid saying or using `git status` in workflow updates.

*Created: 2026-02-18T20:28:08.648Z*

### [mbxeqqRMLsjxpELxUjicMJ] (preference)

[USER] Supermemory export path: Does not want snapshot/export markdown saved under RepoRoot .opencode unless explicitly requested; keep focus on data migration only.

*Created: 2026-02-18T19:50:24.882Z*

### [vw3icVFUEUmrGM6AVpKihj] (preference)

[USER] PR Bots: Uses Macroscope + Qodo 2-bot stack (Greptile dropped 2026-02-17). Silent Sentinel philosophy: bots stay quiet until high-confidence issues. Macroscope.md is single source of truth for Macroscope config. Strictness level 3; logic-only comments; ignore style/syntax noise.

*Created: 2026-02-18T19:15:19.318Z*

### [pdz8Vjx5z6MAWLJbxRReKM] (preference)

[USER] Pref: When using ultrawork mode, user expects concise responses (3-6 sentences or up to 5 bullets), strict scope adherence, and no extra features.

*Created: 2026-02-17T08:30:30.405Z*

### [bU6oYuFsA3D8hpbhKanVWo] (learned-pattern)

[USER] DCP Compress Boundary Matching: compress tool searches ALL message content (text + tool outputs + tool inputs + summaries) via string.includes(). Short or common strings match multiple messages → "Found multiple matches" error. Fix: use 30+ char strings from own assistant text. Fallback: distill + prune instead of compress. Root cause is in findExactMatches() in lib/tools/utils.ts — no code-level fix exists upstream as of v2.1.3.

*Created: 2026-02-16T09:54:38.501Z*

### [PRShgJJiYMLntFiUxjhs8E] (learned-pattern)

[USER] Write vs Edit Tool Clash: OpenCode write tool rejects existing files ("File already exists"). write requires a prior read of the same file still present in active context — if the read was pruned/compressed/distilled, write fails. For full-file rewrites of existing files: use edit with entire content as oldString, or re-read immediately before write in same turn.

*Created: 2026-02-16T09:54:38.331Z*

### [bf9ceAx2t3zCni1193Zmob] (learned-pattern)

[USER] OpenCode Plugin Best Practices (2026-02-15): Single-file bundled plugins use `export const Plugin = async (ctx) => { return { ...hooks } }` pattern. Critical: ctx is a context object `{ client, project, directory, worktree, $ }` - NOT the client itself. Destructure: `async ({ client })` not `async (client)`. Tool registration via `tool: { myTool: tool({ description, args: { ...zod schemas }, execute }) }`. Hook orchestration: multiple hooks returned in single object, OMO uses factory pattern `createHooks()` that merges `...core, ...continuation, ...skill`. Config schema: use zod with `.optional()` for all fields, add `disabled_hooks` array for enable/disable flags. Error handling: (1) `if (!output.output) return` guard in tool.execute.after hooks to prevent undefined crashes, (2) `safeCreateHook()` wrapper for hook creation with try/catch, (3) session-keyed Maps for state (not globals). Hook conflicts: OMO uses `isHookEnabled(hookName)` check before registering hooks, `safe_hook_creation` experimental flag wraps creation in try/catch. Plugin load order: global opencode.json → project opencode.json → global plugins/ → project plugins/. Use `client.app.log()` not `console.log` for structured logging.

*Created: 2026-02-15T10:10:35.300Z*

### [fhmpSAZu6YX8fDWYRcNvEJ] (preference)

[USER] PR safety preference (2026-02-14): For mosaic-facing updates, only apply changes that are 100% definite and clearly owned by our recent commits; avoid touching uncertain or likely external work-in-progress items even if bots flag them.

*Created: 2026-02-14T08:43:19.572Z*

### [cNGD92XspYd5aFxQyd3ajt] (preference)

[USER] Context limit rule: Keep session context safely below 67% by proactively using DCP/compaction and continuation when needed during long debugging loops.

*Created: 2026-02-13T11:22:12.634Z*

### [1yxkkEsCWj3XkiYE2Nf2pQ] (learned-pattern)

[USER] Search Tool Division of Labor: Local tools (grep/ast-grep/augment) first, then Context7 for library docs (free), then Sourcegraph for GitHub/code search (free, regex/symbol/language filters, no Tavily needed for GitHub code), then Tavily search for general web content (1 credit), Tavily extract for specific URLs (0.2 credit/URL), and Tavily crawl only when needed (3-5 credits). Always exhaust free tools before spending Tavily credits. For GitHub implementation searches specifically, Sourcegraph is the default and Tavily should be avoided.

*Created: 2026-02-13T01:38:35.870Z*

### [TGhcTne5EkG4PifoN7fifT] (project-config)

[USER] MCP MIGRATION (2026-02-13): Docker Gateway DISABLED to eliminate WSL2 crash cycle (14 error dialogs/24h caused by container networking churn). Replaced with standalone MCPs: tavily (npx, enabled), context7 (npx, enabled), notion (npx, disabled/on-demand). Active standalone MCPs: sourcegraph, augment-context-engine, tavily, context7. Docker Desktop can stay off unless needed for actual container work.

*Created: 2026-02-13T01:29:18.310Z*

### [Fme2BGC87JcUCvKcskgwTt] (preference)

[USER] TAVILY USAGE: User has Student Project plan (4,000 queries/month). Librarian agents consume 3-5 Tavily queries each internally. Avoid launching multiple librarian agents for well-understood topics. One targeted search is better than five shotgun searches. User prefers efficiency over exhaustiveness.

*Created: 2026-02-13T01:26:37.168Z*

### [gxtPtQ394gcQxsdV9jBzgQ] (architecture)

[USER] Gemini Deep Research architecture insight (2026-02-12): User's Gemini Ultra ($249.99/mo) gives 200 DR/day with Gemini 3 Pro model (#1 on MMDR-Bench at 49.41). API access exists via Interactions API (agent: deep-research-pro-preview-12-2025). Key feature: Can connect Google Drive/Gmail as sources, can deselect Google Search to research only personal data. Known bug: consumer web client uses aggressive context slicing — use Google AI Studio for heavy file work. MCP support announced but no date. User wants to build: Tavily (routine search) → auto-escalate to Gemini DR (complex queries) → sync context to Google Drive for DR to access.

*Created: 2026-02-12T23:26:35.330Z*

### [GX7oqM4DN4sPNhJL4rTsHv] (preference)

[USER] Student status: User is a student and wants to leverage student free trials, academic discounts, and educational pricing for search/web APIs.

*Created: 2026-02-12T06:50:18.833Z*

### [dMdeC3geCDrfiV2pMs9jBL] (preference)

[USER] Warning Treatment Policy: Treat ALL warnings as errors that must be resolved, unless the warning is genuinely unsolvable (e.g., third-party upstream issue with no workaround). Never dismiss warnings as "just warnings" — always fix them. User wants zero-warning output from all commands.

*Created: 2026-02-10T20:47:44.431Z*

### [haM8hATpb9Y9BfxRCqXSqM] (preference)

[USER] User uses React and web design — agents must NOT assume stack is limited to OpenCode/OMO plugin dev. Skills vercel-react-best-practices, web-design-guidelines, platform-design-web are intentionally kept.

*Created: 2026-02-10T05:40:54.075Z*

### [k2kaSfZcJm5jWpfX4KtQuk] (preference)

[USER] Writing preference: avoid em dash characters in prose; use commas, semicolons, colons, or sentence restructuring instead.

*Created: 2026-02-09T22:30:09.068Z*

### [UKAUeMRNoA31juCeGNseY3] (preference)

[USER] Quality preference: prioritize elegant, high-rigor implementations with thorough verification and minimal sloppy mistakes.

*Created: 2026-02-09T22:30:09.031Z*

### [aJgAEQR6vUvoG27vDpXyJb] (preference)

[USER] Preference: always persist research findings/roadmaps into markdown files during work rather than relying only on active context.

*Created: 2026-02-09T22:30:08.892Z*

### [GU6p1h2dCuwLHgAbADKGD7] (learned-pattern)

[USER] PR review feedback handling: use `gh api .../reactions` for false-positive signaling; PR review comments do not support a `/replies` endpoint, use issue comments instead.

*Created: 2026-02-09T22:30:08.821Z*

### [Ptp4WPzsdqQ6DZmMjmXJah] (preference)

[USER] Context Transition Pref: Wants automatic handoff-first flow; use /handoff as the main transition mechanism instead of preemptive compaction, with capture before and reinjection after handoff.

*Created: 2026-02-09T02:23:04.476Z*

### [RypWyBR6Jw7GVpNwy8ixbQ] (preference)

[USER] Supermemory curation preferences: keep user/global memories only if they apply across all projects; move project-specific items to project scope rooted at ~/.config/opencode; remove overlapping entries and sensitive data; prefer redacted summaries for sensitive topics; delete obsolete plugin info while preserving useful best practices/patterns.

*Created: 2026-02-08T18:54:44.908Z*

### [d9q4Z7jvJK2k548eMfgoPg] (preference)

[USER] HARD RULE: No AI or AI-generated mentions in code, docs, or commits. Treat all work as if it's upstream open source - written by a human engineer. No "AI assistance", "generated by", "Claude", etc.

*Created: 2026-02-05T12:01:20.679Z*

### [5Y4iKicx5VMXDmpCShszvT] (preference)

[USER] PR Detail: Prefers very detailed PR bodies with folder-by-folder breakdown and comprehensive context.

*Created: 2026-01-30T19:56:33.485Z*

### [qmMd2DM9n6V44Xdx7nxoNL] (preference)

[USER] Workflow: Wants a report/confirmation before applying any code changes.

*Created: 2026-01-30T19:56:32.202Z*

### [NgAu86CTjEKwfPZoGKwjpw] (preference)

[USER] PR Workflow Pref: Prefers squashing changes to avoid extra history. With JJ: use jj squash to combine commits, or jj describe --reset-author to amend.

*Created: 2026-01-26T22:13:59.740Z*

### [BBEzNDaTNSKSy2TqfjqiVy] (preference)

[USER] PR Workflow Pref: Push new branch for review first with multi-commit history. With JJ: jj bookmark create name && jj git push -b name.

*Created: 2026-01-26T21:13:41.815Z*

### [QScyeZCxwMsZj1nNq1qKBp] (error-solution)

[USER] GitHub Actions permissions: Workflows that add labels or post comments need explicit `permissions: issues: write, pull-requests: write, contents: read` block. Without this, fails with 403 "Resource not accessible by integration". Template at opencode/template/pr-bot-aggregator.yml has this fix.

*Created: 2026-01-16T11:15:01.647Z*
