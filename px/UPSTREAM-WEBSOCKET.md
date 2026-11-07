# Codex upstream WebSocket and native custom tools

The active `GPT-Private` Provider uses the ChatGPT OAuth channel:

- HTTP Base: `https://chatgpt.com/backend-api/codex`
- Provider → Advanced → WebSocket URL (`vendor_options.websocket_url`):
  `wss://chatgpt.com/backend-api/codex/responses`

This is the server-to-upstream endpoint. Client ingress stays at
`wss://masub-ex.retlinks.top/comments/responses` or
`wss://masub-ex.yuhatze.org/comments/responses`.
The API-key OpenAI endpoint `wss://api.openai.com/v1/responses` is a different
channel and must not be used with this ChatGPT OAuth Provider.

Save the advanced field through the Provider editor or management API; it is
stored with the Provider and survives restarts and backups. Do not rewrite an
invalid Provider merely to populate this field: changing vendor options clears
its invalid-credential marker. Configure credential-invalid `GPT-Team` after renewing its
OAuth authorization; do not redirect its keys to the private account.

## Custom tool compatibility

The former parser rejected `custom_tool_call`. Repeated parse errors then put
the model Target in cooldown, producing `503: Model has no eligible Target`.
The fix supports native custom call/output history, free-form input delta/done
events, namespace and call identifiers, typed completion, and terminal-state
consistency. Custom input is not coerced to JSON function arguments.
Custom events count as visible output, so streams publish promptly and cannot
be retried after client output is committed. Native custom history is rejected
when the selected upstream protocol cannot represent it.

Build and deploy the native host and Codex component together. The Codex
component is `0.6.4+masub.responses.2`; the upgrade manifest must retain
`positional_additional_tools`, `per_key_request_model_mapping_v1`, and
`native_custom_tools_v1`. Keep the GA Claude compatibility component enabled.

Run synthetic full round trips, not only WebSocket handshakes:

```sh
python3 verify-responses-streams.py --host masub-ex.retlinks.top \
  --model gpt-6.1-sol --custom-tools --report /root/custom-tools-main.json
python3 verify-responses-streams.py --host masub-ex.yuhatze.org \
  --model gpt-6.1-sol --custom-tools --report /root/custom-tools-cf.json
```

Each command verifies eight completed responses: plain and namespaced custom
tools over HTTP and WebSocket, each followed by a tool-result response. Logs
contain status, event count, output types, and timing only. Existing normal
Responses and Claude compatibility smoke tests remain required.

Reference: [Responses WebSocket connect](https://developers.openai.com/api/reference/resources/responses/methods/connect),
[custom tool call items](https://developers.openai.com/api/reference/resources/conversations/subresources/items/methods/create).
