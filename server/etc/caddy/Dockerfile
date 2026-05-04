# Build frontend with Bun
FROM oven/bun:1.3.3-alpine AS builder

# Real node is needed for kubb codegen — the bun image's "node" is a
# bun shim, and kubb's plugin-oas misreads argv under it.
RUN apk add --no-cache nodejs

WORKDIR /app

# Copy client package files
COPY client/package.json client/bun.lock* ./

# Install dependencies
RUN bun install --frozen-lockfile

# Copy client source
COPY client/ ./

# Copy OpenAPI spec for SDK generation (kubb config reads ../server/openapi.json)
COPY server/openapi.json ../server/openapi.json

# Generate SDK and build static assets
RUN bun run gen && bun run build

# Build Caddy with rate-limit module
FROM caddy:2-builder AS caddy-builder
RUN xcaddy build --with github.com/mholt/caddy-ratelimit

# Production image with custom Caddy
FROM caddy:2-alpine
COPY --from=caddy-builder /usr/bin/caddy /usr/bin/caddy

# Copy built frontend
COPY --from=builder /app/dist /srv

# Copy Caddy config
COPY server/etc/caddy/Caddyfile /etc/caddy/Caddyfile

EXPOSE 80 443
