---
"postagent": minor
"postagent-darwin-arm64": minor
"postagent-darwin-x64": minor
"postagent-linux-arm64-gnu": minor
"postagent-linux-x64-gnu": minor
---

feat(send): support `-d @file` and `-d @-` to read the request body from a file or stdin (curl-compatible).

- `postagent send ... -d @./payload.json` reads the body from `./payload.json` instead of treating `@./payload.json` as a literal string. Previously the only way to send a file's contents was `-d "$(cat payload.json)"`, which was easy to miss and produced confusing API errors (e.g. GitHub returning `400 Problems parsing JSON`) when forgotten.
- `postagent send ... -d @-` reads the body from stdin, matching curl's shorthand for piping payloads in.
- Resolution happens before `$POSTAGENT.<SITE>.TOKEN` substitution and the token-presence check, so templates inside the file are still resolved and the body's actual contents (not the path) participate in validation.
- Inline `-d <value>` continues to work unchanged for values that don't start with `@`.
