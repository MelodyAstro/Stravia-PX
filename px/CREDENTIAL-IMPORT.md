# Credential import and RT backups

In **Model services → Import credentials**, choose the account type, select a JSON
file or paste its contents, and preview before confirming selected rows. Each
document is limited to 1 MiB and 25 accounts. Names can be edited before import.

Codex subscription OAuth imports accept official `auth.json` (`tokens`), Cockpit
Tools and CLIProxyAPI/CPA portable records, and sub2api `accounts[].credentials`
exports, individually or in arrays. Access/refresh/ID tokens, the selected account
ID and OAuth client ID are retained. Refresh tokens are exchanged at OpenAI's fixed
OAuth endpoint; access-only credentials are checked against the usage endpoint and
cannot refresh automatically. API-key, Agent Identity and personal-access-token
records are explicitly rejected by this subscription importer. They are not
silently converted to OAuth.

Antigravity/Google accepts `{"type":"antigravity","refresh_token":"…",
"email":"…","project_id":"…"}`, arrays, the `mail` spelling, or a single
plain refresh token. Existing access tokens are replaced by a Google OAuth refresh.
Tokens must have the scopes required by Antigravity. A token issued to a different
Google OAuth client requires its matching `client_id` and `client_secret`; importing
it does not grant additional scopes. Project discovery follows the existing model
discovery flow. Missing account/project metadata is not fabricated.

The Antigravity account menu offers **Back up RT JSON** with explicit confirmation.
This downloads the current refresh token and available email/project/client
metadata. Access tokens and unrelated private metadata are omitted. The resulting
file can sign in to the account and must be kept private. Import/export endpoints
require administrator authentication and CSRF protection and return `no-store`.
Credential text is cleared from the dialog after submission or closing; it is not
written to browser storage or logs.

Only credential fields are imported. Proxy URLs, base URLs, routes, API Keys,
passwords, notes and fingerprint configuration in external exports are ignored.
An explicit switch selects this site's configured outbound proxy. Existing accounts
are detected by provider/account ID or refresh token and skipped; imports never
overwrite Providers, group Keys or model routes. Per-row results let a mixed batch
finish even if another row is unsupported or rejected. After a timeout, preview
again before retrying: an admitted import continues persisting rotated credentials
even if the browser disconnects.

Format references: [official Codex auth storage](https://github.com/openai/codex/blob/main/codex-rs/login/src/auth/storage.rs),
[Cockpit Tools export formats](https://github.com/jlcodes99/cockpit-tools/blob/main/src/utils/codexExportFormats.ts),
[CLIProxyAPI Codex token storage](https://github.com/router-for-me/CLIProxyAPI/blob/main/internal/auth/codex/token.go),
and the [requested Antigravity RT JSON example](https://chai.aihao666.com/archives/antigravity-rt-json-login).

CI history is limited to the newest **25 runs across the repository**. Older
completed runs (including their CI artifacts/logs) are deleted after builds and on
a daily schedule; active runs are left until completed. Published release assets
and compiled-tool caches are not removed by this policy.
