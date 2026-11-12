# Codex upstream WebSocket

The ChatGPT OAuth channel uses:

- HTTP Base: `https://chatgpt.com/backend-api/codex`
- Provider advanced WebSocket URL: `wss://chatgpt.com/backend-api/codex/responses`

These are upstream service addresses, independent of your site's public hostname. If your reverse proxy uses the example `/comments` prefix, a client WebSocket endpoint is `wss://gateway.example.com/comments/responses`.

The API-key endpoint `wss://api.openai.com/v1/responses` belongs to another authentication channel. Keep account groups isolated, and do not change an invalid account's options merely to clear its credential marker; reauthorize it through the management interface.

The native host and Codex component must both support custom calls, free-form input events, namespaces, tool results and terminal completion. Retain `positional_additional_tools`, `per_key_request_model_mapping_v1` and `native_custom_tools_v1` in managed upgrade manifests.

Validate complete streams and tool round trips using your own public hostname:

```sh
python3 px/ops/verify-responses-streams.py --host gateway.example.com \
  --model gpt-6-luna --custom-tools --report /root/custom-tools.json
```

The script reads credentials from a private local file and reports status/type/timing metadata. A successful handshake alone does not establish response compatibility.

Reference: [Responses WebSocket](https://developers.openai.com/api/reference/resources/responses/methods/connect).
