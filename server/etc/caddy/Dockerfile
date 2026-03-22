# Build frontend with Bun
FROM oven/bun:1 AS builder

WORKDIR /app

# Copy client package files
COPY client/package.json client/bun.lockb* ./

# Install dependencies
RUN bun install --frozen-lockfile

# Copy client source
COPY client/ ./

# Build static assets
RUN bun run build

# Production image with Caddy
FROM caddy:2-alpine

# Copy built frontend
COPY --from=builder /app/dist /srv

# Copy Caddy config
COPY server/etc/caddy/Caddyfile /etc/caddy/Caddyfile

EXPOSE 80 443
