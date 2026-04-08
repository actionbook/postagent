# postagent

CLI tool for AI agents. Discover and browse API documentation for various services without WebSearch.

## Install

```bash
npm install -g postagent
```

## Usage

### Search

```bash
postagent search "code hosting"
```

### Manual (Progressive Discovery)

```bash
postagent manual                       # List all projects
postagent manual github                # List groups of a project
postagent manual github repo           # List actions of a group
postagent manual github repo list      # Get API doc for a specific action
postagent manual github repo list --format json  # Output as JSON
```

### Send

```bash
postagent send https://api.example.com
postagent send https://api.example.com -X POST -d '{"key":"value"}'
postagent send https://api.example.com -H "Authorization: Bearer token"
```

### Auth

```bash
postagent auth github                  # Save API key for a project
```

## Configuration

Environment variables:

```bash
export POSTAGENT_API_URL=https://api.postagent.dev
export POSTAGENT_API_KEY=pa_xxxxxxxxxxxx
```

## Development

```bash
pnpm install
pnpm dev                               # Run with POSTAGENT_DEV mode
pnpm dev:watch                         # Watch Rust code and auto-rebuild
pnpm build                             # Build for production
```

## License

ISC
