# Postagent

Postman CLI, but for AI Agents. Discover, browse, and send requests to thousands of APIs.

## Install

```bash
npm install -g postagent
```

## Quickstart

```bash
# Search related actions
postagent search "Create a document on Notion"

# Get detailed manual for an action
postagent manual notion pages create_page

# Auth and send request
postagent auth notion
postagent send -X POST https://api.notion.com/v1/pages \
  -H 'Authorization: Bearer $POSTAGENT.NOTION.API_KEY' \
  -H 'Notion-Version: 2022-06-28' \
  -H 'Content-Type: application/json' \
  -d '{"parent":{"page_id":"YOUR_PAGE_ID"},"properties":{"title":[{"text":{"content":"My Page"}}]}}'
```

The `send` command uses the same options as `curl`, so agents already know how to use it.

Postagent replaces the `API_KEY` placeholder with the actual key/token from local storage, keeping your credentials out of the LLM context entirely.

## Usage with Agents

The easiest way is to just tell your agent to use it:

```
Use postagent to make a marketing plan on Notion, then create and assign a task to me on Linear. Run postagent --help to see available commands.
```

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

## Development

```bash
pnpm install
pnpm dev:watch
```

## License

Apache-2.0
