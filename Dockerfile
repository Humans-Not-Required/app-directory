# Stage 1: Build frontend
FROM node:22-slim AS frontend-builder

WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build backend
FROM rust:1.83-slim-bookworm AS backend-builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY tests/ tests/

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/app-directory /usr/local/bin/app-directory

# Stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/bash appuser
WORKDIR /app

COPY --from=backend-builder /usr/local/bin/app-directory /app/app-directory
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Data directory for SQLite
RUN mkdir -p /app/data && chown appuser:appuser /app/data
VOLUME /app/data

ENV DATABASE_PATH=/app/data/app_directory.db
ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=8002
ENV STATIC_DIR=/app/frontend/dist

USER appuser
EXPOSE 8002

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8002/api/v1/health || exit 1

CMD ["./app-directory"]
