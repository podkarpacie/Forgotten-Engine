# Forgotten Engine container image. The build stage compiles the release binary; the runtime
# stage ships only the server, its data directory contract, and a non-root operator user.
FROM rust:1-slim AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --bin forgotten-engine

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --shell /usr/sbin/nologin forgotten
COPY --from=build /build/target/release/forgotten-engine /usr/local/bin/forgotten-engine
USER forgotten
WORKDIR /home/forgotten
EXPOSE 7171 7172 7173
# The world directory is a mounted volume in normal deployments:
#   docker run -v ./my-world:/data forgotten-engine run /data
ENTRYPOINT ["forgotten-engine"]
CMD ["run", "/data"]
