# Account reset and site spend controls

Strinova extends the installable Codex and Claude Code vendor components. The
host patch adds authenticated admin actions, durable account admission and the
small shared WebUI controls. No reset is redeemed by a refresh or automatic
retry policy. GA-Claude uses a different upstream and does not advertise these
official Claude-account controls.

## Reset inventory and manual redemption

The allowance page displays a count only when the authenticated upstream
actually supplies a valid inventory. Missing, unsupported or malformed data
stays unknown. Scheduled quota reset times are separate from consumable resets;
zero consumable resets cannot suspend inference.

An explicit confirmation is required. Before the irreversible call, both host
and component persist the UUID; Claude also persists the selected grant. Known
results are cached. An ambiguous result keeps the original UUID and grant and
blocks a different action until that result is resolved. Changing the OAuth
connection/account does not authorize retrying a pending mutation against the
new account. Provider-scoped atomic component state and host compare/exchange
protect concurrent writes; upstream idempotency protects network retries.

Codex follows the [official backend-client reset contract](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs):
`wham/usage` inventory and `wham/rate-limit-reset-credits/consume`. A successful
HTTP status alone is not a confirmed redemption.

Claude uses the `cedar_ember` inventory and the organization reset endpoint.
The inventory parser was implemented against the primary
[CodexBar reset implementation](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Claude/ClaudeRateLimitResetCredits.swift);
the mutation contract was cross-checked against
[TeamClaude's account reset implementation](https://github.com/KarpelesLab/teamclaude/blob/master/src/claude-resets.js).
These account-management endpoints are not a public Messages API contract.
See also [Anthropic's limit-reset explanation](https://support.claude.com/en/articles/17007452-what-is-a-limit-reset).
The code implements the contracts independently and does not bundle either project.

## Balances and lifetime site estimate

Official balances retain their original units. Codex credits are not USD.
Claude's legacy `extra_usage` amounts are minor currency units, whereas the newer
`spend` object has an explicit amount and currency. Remaining monthly spend
headroom is not labelled a prepaid cash balance. A balance unavailable through
the signed-in OAuth account stays unknown; the server does not import browser
cookies or infer a balance from a subscription percentage.

The site cap is opt-in and initially off. The first save starts a lifetime
ledger, with no historical backfill or monthly reset. Disabling or changing the
cap never erases accumulated estimates. Duplicate Providers for one Codex
account ID / Claude organization share a ledger. Account identifiers are hashed
in storage and omitted from the public budget view. All inference and native
compaction attempts through the host's prepared-vendor execution boundary use
the same admission path, including HTTP, SSE and WebSocket clients.

Per-request credit debits are not part of the canonical upstream usage result.
As selected for this deployment, token usage is therefore conservatively priced
and visibly marked **estimated**. Included subscription calls may count as
credit-equivalent usage. Activity outside this site is not attributed to the
ledger. This is not an upstream billing budget or a statement of actual charges.

The pinned official rate card was checked on **2026-11-11** against
[OpenAI's Codex credit rates](https://learn.chatgpt.com/docs/pricing) and
[Anthropic's USD pricing](https://platform.claude.com/docs/en/about-claude/pricing).
It includes documented speed, long-context and cache-write premiums
conservatively; cached token categories may be counted again where protocols
report them separately. Do not substitute API dollar prices for Codex credits.
New or unpriced models fail closed while a cap is enabled. When the cap is off,
unpriced requests are counted separately rather than recorded as zero-cost.
A concurrently enabled cap prevents an unpriced request from committing its
accounting admission. Rate-card changes
should be reviewed and shipped through CI; GPT-5.6 Sol's published promotion is
currently guaranteed only through November 21, 2026.

Each attempt reserves the cost of its verified maximum model context (including
separate cache categories) before contacting the upstream. This deliberately
means a short request can be rejected when the remaining cap cannot cover that
reservation. Settlement uses complete reported token counts. Unknown counts,
server-tool charges and interrupted attempts retain the full conservative
reservation as uncertain spend. A process crash leaves the durable reservation
intact; restart never silently refunds it. The WebUI displays reserved and
uncertain amounts separately. Account-cap denials are local quota errors and do
not invalidate credentials or count as an upstream failure.

SQLite and PostgreSQL use the existing settings table with atomic
compare/exchange; no custom SQL migration or schema drift is introduced. The
ledger is part of the normal full-data backup and rollback snapshot.
