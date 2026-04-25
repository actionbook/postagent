# Postagent

**One CLI for every app your agent needs.**

No more installing a new MCP server or CLI for each app your agent uses. Postagent connects your AI agent to any API — Notion, Linear, GitHub, Slack, and more.

- 🔌 **No per-site setup** — one `npm install -g postagent` covers every supported API
- 🔍 **Discover by intent** — `postagent search "create a doc on Notion"` instead of reading docs
- 🔐 **Credentials stay local** — API keys are injected at send time, never leaked into the LLM context

## Install

```bash
npm install -g postagent
```

## Quickstart

Postagent is meant to be driven by an agent, not by you. After installing, just tell your agent it has `postagent` available, then send it a task — for example:

> "Use CLI postagent to list my documents on notion"

The agent will discover, read, auth, and call the API on its own:

![Postagent demo](./assets/demo.png)

`send` mirrors `curl`, so agents already know how to drive it. The `$POSTAGENT.NOTION.API_KEY` placeholder is resolved from local storage at send time — your credentials never enter the model context. Add `--dry-run` to any `send` to let the agent preview the resolved request (with sensitive headers redacted) before firing.

## Configuration

You can try and test Postagent without setting an API key. However, the no API key mode is rate limited to 10 requests per minute.

If you need more requests, you can get a free API key from [Actionbook](https://actionbook.dev) and set it using the following command:

```bash
postagent config set apiKey ak_xxxxxxxxxxxx
```

Or via environment variables:

```bash
export POSTAGENT_API_KEY=ak_xxxxxxxxxxxx
```

## Commands

```bash
postagent search <query>                    # Search for related actions by natural language
postagent auth <site>                       # Complete an auth flow for a site
postagent manual <site> [group] [action]    # Get detailed manual for an action
postagent send [options]                    # Send a request to a site, the options is same as curl
```

## Supported sites

Visit [https://api.postagent.dev/supported-sites.md](https://api.postagent.dev/supported-sites.md) to get the full list of supported sites. We are continually expanding it.

## Stay tuned

We move fast. Star Postagent on Github to support and get latest information.

![star-postagent-original](https://github.com/user-attachments/assets/ba15bfe6-04ec-4e24-bce8-4f717547f8fd)

## Development

```bash
pnpm install
pnpm dev:watch
```

## License

Apache-2.0
