# qmtacp

JSON CLI for a running QueryMT ACP WebSocket server (`qmtcode --acp-ws`).
Default endpoint is `ws://127.0.0.1:3030/ws`.

Each invocation connects, initializes, does one job, and exits. Session ids
are the only state. JSON goes to stdout; connection logs go to stderr.

```sh
cargo run -- caps
cargo run -- new --cwd . --profile default --mode build
cargo run -- sessions --cwd . --limit 20
cargo run -- models --query grok --provider xai
cargo run -- prompt --new --cwd . "fix the build"
cargo run -- prompt SESSION_ID "continue"
cargo run -- runtime SESSION_ID
cargo run -- steer SESSION_ID --run-id RUN "stop and summarize"
cargo run -- queue SESSION_ID "next: run tests"
cargo run -- inspect SESSION_ID --messages 20
cargo run -- set-mode SESSION_ID plan
cargo run -- cancel SESSION_ID
```

`prompt` streams compact NDJSON (`text`, `tool`, `mode`, `done`). `exec` runs multiple
commands on one connection.

`--permission allow-once` is the default so coder tools can run. Elicitation
requests are cancelled and emitted as NDJSON events during `prompt`.
