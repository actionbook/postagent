---
"postagent": patch
"postagent-darwin-arm64": patch
"postagent-darwin-x64": patch
"postagent-linux-arm64-gnu": patch
"postagent-linux-x64-gnu": patch
---

fix(send): auto-refresh expired OAuth tokens once on 401/403.

On a 401/403 from the upstream, if the request used a `$POSTAGENT.<SITE>.TOKEN` backed by an OAuth credential with a saved refresh_token, `postagent send` now refreshes the access_token once and retries. Static credentials and non-recoverable refresh failures fall back to the existing error path.
