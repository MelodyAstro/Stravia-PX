# Isolated account groups

Create separate Keys and authorized model routes for each account group. For example, one Key can map `gpt-6-luna` to `team/gpt-6-luna`, while another maps it to `private/gpt-6-luna`.

Keep each route's targets within its intended group. A failed or invalid account must not silently fall back to another group's credentials. After reauthorization, verify both a complete response and denied cross-group access.

ChatGPT OAuth connections use the upstream WebSocket endpoint described in [upstream WebSocket configuration](UPSTREAM-WEBSOCKET.md). No real account inventory or deployment endpoints are stored in this repository.
