# The cloud: the editor (at `/`) and its API on one origin, as one image.
#
# Three stages. The portal and the Rust binary build in their own toolchains;
# the runtime is a slim Debian with nothing but the binary, the built portal,
# and CA roots. Deployed by .github/workflows/server.yml to Easypanel, which
# puts its proxy (domain + TLS) in front — hence `--trust-proxy`.
#
# Data (pairing identities, desktop registrations) lives in /data: mount a
# volume there, or every redeploy starts from nothing.

# The editor alone, at `/`: the public pages are the static site's job.
# `SITE_URL` is where the editor's wordmark leads back to.
FROM node:24-bookworm-slim AS portal
ARG SITE_URL=https://flex-deck.com
ENV VITE_EDITOR_ONLY=true VITE_SITE_URL=$SITE_URL
WORKDIR /src
RUN corepack enable
COPY . .
RUN pnpm install --frozen-lockfile --filter "@flexdeck/portal..." \
 && pnpm --filter @flexdeck/portal build

FROM rust:1.89-bookworm AS cloud
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p flexdeck-cloud

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --home /data flexdeck \
 && mkdir -p /data && chown flexdeck /data
COPY --from=cloud /src/target/release/flexdeck-cloud /usr/local/bin/flexdeck-cloud
COPY --from=portal /src/apps/portal/dist /srv/portal
USER flexdeck
VOLUME /data
EXPOSE 8787
ENTRYPOINT ["flexdeck-cloud", "--bind", "0.0.0.0", "--port", "8787", "--portal-dir", "/srv/portal", "--data-dir", "/data", "--trust-proxy"]
