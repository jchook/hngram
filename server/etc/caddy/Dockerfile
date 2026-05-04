# Build frontend with Bun
# NOTE: client/src/gen/ (kubb-generated SDK) must already be populated
# in the build context. CI runs `bun run gen` before `docker build`
# because kubb misbehaves under bun inside this image.
FROM oven/bun:1.3.3 AS builder

WORKDIR /app

# Copy client package files
COPY client/package.json client/bun.lock* ./

# Install dependencies
RUN bun install --frozen-lockfile

# Copy client source (includes pre-generated src/gen)
COPY client/ ./

# Build static assets
RUN bun run build

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
