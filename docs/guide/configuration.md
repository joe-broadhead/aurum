# Configuration

## Precedence

1. CLI flags  
2. Environment variables  
3. Config file  
4. Built-in defaults  

## Environment

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | Remote auth (preferred over file) |
| `OPENROUTER_BASE_URL` | Override API base (tests / proxies) |
| `RUST_LOG` / tracing filters | Diagnostics |

## Config file

Path via the `directories` crate (`ProjectDirs` app name `aurum`).

```toml
[default]
provider = "local"
model = "base"
language = "auto"
output = "txt"

[openrouter]
# api_key = "sk-or-..."   # prefer env var
# model = "google/gemini-2.5-flash"
# base_url = "https://openrouter.ai/api/v1"
```

## Safety limits

| Limit | Default |
|-------|---------|
| Max duration | ~2.25 hours |
| Max decoded PCM | ~500 MB |
| Max remote upload | ~24 MB compressed |
