# Remail

A modern email development environment with API, UI, and SMTP services.

## Quick Start

```bash
# Start all services with live reloading
docker compose up --build
```

That's it! All services will be available at:

- **API**: http://localhost:3000
- **UI**: http://localhost:8080
- **SMTP**: localhost:2525
- **Database**: localhost:5432

## Development

The Docker setup includes:

- **Live reloading** for all services
- **Volume mounts** for instant code changes
- **Shared dependencies** between services
- **Health checks** for proper startup order

## Services

- **API** (Axum) - REST API for email management
- **UI** (Dioxus) - Web interface for viewing emails
- **SMTP** (Custom) - SMTP server for receiving emails
- **Database** (PostgreSQL) - Persistent storage

## Stopping

Press `Ctrl+C` to stop all services, or run:

```bash
docker compose down
```

## Individual Services

To run just one service:

```bash
# API only
docker compose up api

# UI only
docker compose up ui

# SMTP only
docker compose up smtp
```
