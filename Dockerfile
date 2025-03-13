FROM rust:1-bookworm AS backend-builder
COPY Cargo.toml Cargo.lock /usr/src/
WORKDIR /usr/src/

# Improves build caching, see https://stackoverflow.com/a/58474618/5088458
RUN sed -i 's#src/main.rs#src/dummy.rs#' Cargo.toml
RUN mkdir src && echo "fn main() {}" > src/dummy.rs
RUN cargo build --release

RUN sed -i 's#src/dummy.rs#src/main.rs#' Cargo.toml && rm src/dummy.rs
COPY src /usr/src/src
RUN cargo build --release

# Build whole application
FROM gcr.io/distroless/cc-debian12:debug
COPY --from=backend-builder /usr/src/target/release/oci-semver-tagging /usr/local/bin/
ENTRYPOINT [ "/usr/local/bin/oci-semver-tagging" ]
