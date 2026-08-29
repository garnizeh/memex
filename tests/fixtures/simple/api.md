# API Reference

This document describes the primary API endpoints and data structures.

## Endpoints

### GET /api/v1/health

Returns the health status of the service.

```json
{
  "status": "healthy",
  "version": "0.1.0"
}
```

### POST /api/v1/query

Performs a documentation search query.

- Parameter: `query` (string)
- Parameter: `limit` (integer, optional)
