# qmtacp

JSON CLI for a running QueryMT ACP WebSocket server (`qmtcode --acp-ws`).
Default endpoint is `ws://127.0.0.1:3030/ws`.

Each invocation connects, initializes, does one job, and exits. Session ids
are the only state. JSON goes to stdout; connection logs go to stderr.

```sh
cargo run -- caps
cargo run -- new --cwd . --profile default --mode build
cargo run -- sessions --cwd .
cargo run -- prompt --new --cwd . "fix the build"
cargo run -- prompt SESSION_ID "continue"
cargo run -- inspect SESSION_ID
cargo run -- set-mode SESSION_ID plan
cargo run -- cancel SESSION_ID
```

`--permission allow-once` is the default so coder tools can run. Elicitation
requests are cancelled and emitted as NDJSON events during `prompt`.
