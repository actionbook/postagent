---
name: postagent
description: Discover, inspect, authenticate, and call third-party APIs with the Postagent CLI. Use when you need to find the right API action from natural language, browse an API manual progressively, save a site credential locally, or send a curl-like HTTP request while keeping secrets out of the model context.
---

# Postagent

## Overview

Use Postagent as an agent-friendly API client. The normal flow is: search for the action you want, inspect the manual for the target site/group/action, authenticate the site if needed, then send the request with curl-style flags.

## Installation

```bash
npm install -g postagent
```

## Quick Start

Start with natural language search:

```bash
postagent search "create a document on notion"
postagent search "create github issue"
```

Drill into the manual progressively:

```bash
postagent manual notion
postagent manual notion pages
postagent manual notion pages create_page
postagent manual notion pages create_page --format json
```

Authenticate the site before sending requests that need credentials:

```bash
postagent auth notion
```

Send the request with curl-style options:

```bash
postagent send -X POST https://api.notion.com/v1/pages \
  -H "Authorization: Bearer $POSTAGENT.NOTION.API_KEY" \
  -H "Notion-Version: 2022-06-28" \
  -H "Content-Type: application/json" \
  -d '{"parent":{"page_id":"YOUR_PAGE_ID"},"properties":{"title":[{"text":{"content":"My Page"}}]}}'
```

## Core Commands

Use `postagent search <query>` to map a user goal to likely site/group/action combinations. Prefer this when you know the intent but not the exact API shape.

Use `postagent manual <site> [group] [action]` to browse progressively:

- Site only: list groups and top actions.
- Site + group: list actions in that group.
- Site + group + action: get full endpoint details, parameters, request body, and response info.

Use `--format json` on `search` or `manual` when another tool or agent needs structured output instead of markdown text.

Use `postagent auth <site>` to save a credential locally for one site. Postagent will prompt for the secret without echoing it in a normal TTY.

Use `postagent send <url> [curl-like options]` to execute the real HTTP request. It accepts `-X/--method`, repeated `-H/--header`, and `-d/--data`.

## Recommended Workflow

Search first unless you already know the exact site, group, and action.

Read the manual before sending a request so you can confirm the method, path, auth header, version header, and request schema.

Authenticate only the site you need. Saved credentials are referenced later with placeholders instead of being pasted into commands.

Use placeholder substitution in the URL, headers, or body:

```text
$POSTAGENT.<SITE>.API_KEY
```

Example:

```bash
-H "Authorization: Bearer $POSTAGENT.GITHUB.API_KEY"
```

Prefer placeholders over raw secrets so credentials stay in local storage rather than the model context.

## Send Behavior

If `-X/--method` is omitted and `-d/--data` is present, Postagent sends `POST`.

If both method and body are omitted, Postagent sends `GET`.

Headers can be provided as repeated `Key: Value` strings or as a JSON object string.

Postagent prints the raw response body on success. On HTTP error responses it prints the status and body, then exits non-zero.

## Practical Examples

Create a Notion page:

```bash
postagent search "create a notion page"
postagent manual notion pages create_page
postagent auth notion
postagent send -X POST https://api.notion.com/v1/pages \
  -H "Authorization: Bearer $POSTAGENT.NOTION.API_KEY" \
  -H "Notion-Version: 2022-06-28" \
  -H "Content-Type: application/json" \
  -d '{"parent":{"page_id":"YOUR_PAGE_ID"},"properties":{"title":[{"text":{"content":"My Page"}}]}}'
```

Inspect GitHub issue creation before making the call:

```bash
postagent search "create github issue"
postagent manual github issues create
```
