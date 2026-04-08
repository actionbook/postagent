# postagent

CLI collection tool for AI agents. Discover and browse OpenAPI documentation for various services without WebSearch.

## Install

```bash
npm install -g postagent
```

## Usage

```bash
# Search for services by keyword
postagent search "code hosting"

# List resources of a site
postagent get github

# List actions of a resource
postagent get github repo

# Get OpenAPI doc for a specific action
postagent get github repo list

# Output as JSON
postagent get github repo list --format json
```

## Configuration

```bash
postagent config set apiUrl https://api.postagent.dev
postagent config set apiKey pa_xxxxxxxxxxxx
```

Or via environment variables:

```bash
export POSTAGENT_API_URL=https://api.postagent.dev
export POSTAGENT_API_KEY=pa_xxxxxxxxxxxx
```

## Development

```bash
npm install
npm run dev          # Run with tsx
npm run build        # Build with tsup
npm run typecheck    # Type check
```

## License

ISC
