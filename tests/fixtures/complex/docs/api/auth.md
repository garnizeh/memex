# Authentication

Authentication protocols and token management.

## OAuth2 Flow

Clients must obtain an access token before querying [Endpoints](endpoints.md).

```bash
curl -X POST /api/auth/token -d "grant_type=client_credentials"
```

## API Keys

Static API keys are supported for automation scripts. See [Security Settings](../security/policy.md).
