---
"postagent": patch
"postagent-darwin-arm64": patch
"postagent-darwin-x64": patch
"postagent-linux-arm64-gnu": patch
"postagent-linux-x64-gnu": patch
---

fix: surface friendlier errors when the postagent server is slow or unreachable.

- `postagent search` and `postagent manual` now use a dedicated HTTP client with a 45s request timeout, so a hung connection fails with an actionable message instead of hanging indefinitely behind reqwest's default (no timeout).
- Request failures are categorized: timeouts print "Request to postagent server timed out after 45s. The server may be busy; try again in a few seconds.", connect failures print "Could not reach postagent server. Check your network connection, then try again.", and other reqwest errors fall through to a labelled "Request to postagent server failed: …" line so unusual cases stay diagnosable.
- HTTP 502/503/504 responses from the postagent server now print a transient-retry hint ("This is usually transient; try again in a few seconds.") instead of being surfaced as a generic API error, which was misleading when the underlying cause was a cold-start or upstream blip.
