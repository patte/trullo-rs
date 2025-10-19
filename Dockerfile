FROM docker.io/library/rust:1.90-slim AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
RUN apt-get update && apt-get install -y --no-install-recommends \
  ca-certificates curl unzip pkg-config libssl-dev build-essential \
  && rm -rf /var/lib/apt/lists/*

# direct install not possible because aarch64 bin missing
# RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/DioxusLabs/dioxus/refs/heads/main/.github/install.sh | bash
# RUN cat install-dx.sh | bash
# RUN /.cargo/bin/dx bundle --platform web

# also no possible because: error[E0432]: unresolved import `serde::__private`
# RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
# RUN cargo binstall dioxus-cli --root /.cargo -y --force

COPY Cargo.toml Cargo.lock ./

# pin to a known good version to avoid serde issues
RUN cargo install \
  --git https://github.com/DioxusLabs/dioxus \
  --tag v0.6.3 \
  dioxus-cli \
  --locked

ENV PATH="/.cargo/bin:$PATH"
COPY . .
RUN dx bundle --platform web

FROM chef AS runtime
COPY --from=builder /app/target/dx/trullo-rs/release/web/ /usr/local/app

ENV PORT=8080
ENV IP=0.0.0.0
EXPOSE 8080

WORKDIR /usr/local/app
ENTRYPOINT [ "/usr/local/app/server" ]