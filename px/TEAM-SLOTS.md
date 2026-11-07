# Team slot configuration

`GPT-Team-5X-SOLT-1` is configured with its own OAuth credentials and the same
upstream WSS endpoint as Private: `wss://chatgpt.com/backend-api/codex/responses`.
The original GPT-Team Key is preserved and enabled only after assigning eight
concrete Team routes and saving their short-name mappings.

Both GPT-Team and GPT-Private clients can send `gpt-6.1-sol`, `gpt-6-luna`, or
any of their eight listed bare model names. The Key determines which account
is used. Team cannot request `private/...`; Private cannot request `team/...`.
The All group explicitly includes both pools; its existing bare GPT aliases
continue to select Private. Team canonical names remain available to All.

`ops/configure-team-slot.py` is a guarded, one-time completion helper for the
first authenticated Team slot. It writes a root-only configuration snapshot,
checks the Provider inventory, creates missing Team routes, sets explicit Key
permissions while disabled, saves aliases, and then enables the existing Key.
It refuses ambiguous multiple Team Providers; adding more slots requires a
reviewed pool configuration instead of guessing an account.

Verify the current permissions without consuming model quota:

```sh
python3 verify-team-key-isolation.py
```

Run complete tool calls using the Team group:

```sh
python3 verify-responses-streams.py --group GPT-Team \
  --host masub-ex.retlinks.top --model gpt-6.1-sol --custom-tools
```

The synthetic custom-tool test uses an exact grammar so model-generated
whitespace cannot be confused with a gateway input-transmission error.
