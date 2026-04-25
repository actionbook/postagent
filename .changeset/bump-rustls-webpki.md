---
"postagent": patch
"postagent-darwin-arm64": patch
"postagent-darwin-x64": patch
"postagent-linux-arm64-gnu": patch
"postagent-linux-x64-gnu": patch
---

chore(deps): bump `rustls-webpki` from 0.103.10 to 0.103.13.

Pulls in three upstream security fixes flagged by Dependabot: a high-severity DoS via panic on malformed CRL BIT STRING (GHSA in `< 0.103.13`), and two low-severity name-constraint bypasses (GHSAs in `< 0.103.12`). `rustls-webpki` is a transitive dependency of `rustls`, so this is a `Cargo.lock`-only patch with no API change.
