# Sandbox image for this repository's agent runtime — Agentic Engineer.
#
# Adapted from the course starter Dockerfile
# (LaunchCodeEducation/LaunchCodeAgenticEngineer, module_1/Dockerfile).
# The course original is a Python 3.12 + Streamlit base; this copy is re-based on
# Rust and extended with the toolchains this runtime needs. It is kept tool-for-tool
# identical to the image this repository's runtime was built from.
#
# What this image deliberately does NOT contain:
#   * project secrets (.env, config.toml) — those stay on the host
#   * the host's SSH keys, cloud credentials, or home directory
#   * Python/Streamlit/ngrok — this repository runs its own Python inside the image
#     as the MCP servers, and needs no tunnel

FROM rust:1.95-slim-bookworm

ARG NODE_VERSION=22.23.2
ARG WASM_PACK_VERSION=0.15.0

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_TERM_COLOR=always \
    NPM_CONFIG_UPDATE_NOTIFIER=false \
    NPM_CONFIG_FUND=false

WORKDIR /workspace

# ---------------------------------------------------------------- OS packages
# ca-certificates/curl/git: fetching toolchains and the agent's own tooling.
# pkg-config + libssl-dev: openssl-sys (axum/jsonwebtoken chain) link deps.
# postgresql-client: psql/pg_isready so the agent can inspect the dev database
#                    container without installing a server.
# procps/nano: ps/top and an editor inside the sandbox.
RUN apt-get update && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        nano \
        procps \
        xz-utils \
        pkg-config \
        libssl-dev \
        postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# ------------------------------------------------------------- rust components
# cargo/rustc come from the base image; the quality gates need both of these.
RUN rustup component add clippy rustfmt \
    && rustc --version && cargo --version && cargo clippy --version && cargo fmt --version

# ------------------------------------------------------------------ Node 22 LTS
# web/ is SvelteKit 5 + Vite 6 + vitest; the distro node is far too old.
RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
        -o /tmp/node.tar.xz \
    && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 --no-same-owner \
    && rm /tmp/node.tar.xz \
    && node --version && npm --version

# ------------------------------------------------------------------- wasm-pack
# node is here for the agent CLI, and wasm-pack is kept so this image stays the same
# as the one the runtime was built from.
# so the frontend cannot build until the WASM crate is packed.
RUN curl -fsSL "https://github.com/rustwasm/wasm-pack/releases/download/v${WASM_PACK_VERSION}/wasm-pack-v${WASM_PACK_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
        -o /tmp/wasm-pack.tar.gz \
    && tar -xzf /tmp/wasm-pack.tar.gz -C /tmp \
    && install -m 0755 "/tmp/wasm-pack-v${WASM_PACK_VERSION}-x86_64-unknown-linux-musl/wasm-pack" /usr/local/bin/wasm-pack \
    && rm -rf /tmp/wasm-pack* \
    && wasm-pack --version

# ---------------------------------------------------------------- coding agents
# Same agents the course image ships. They are the tools; their auth arrives at
# run time (mounted read-only or via the claude-auth volume), never baked in.
RUN npm install -g @anthropic-ai/claude-code opencode-ai \
    && claude --version && opencode --version

# ------------------------------------------------------- Claude Code config
# Course scaffolding, unchanged from module_1.
RUN mkdir -p /root/.claude
COPY settings.json /root/.claude/settings.json
COPY statusline.sh /root/.claude/statusline.sh
RUN chmod +x /root/.claude/statusline.sh

# Persists the Claude Code credential on a volume so login survives container exits.
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Student shell quality-of-life improvements (from module_1).
RUN echo 'export PS1="ai-course:\\w# "' >> /root/.bashrc && \
    echo 'alias ll="ls -alF"' >> /root/.bashrc && \
    echo 'alias la="ls -A"' >> /root/.bashrc && \
    echo 'alias l="ls -CF"' >> /root/.bashrc

ENV CARGO_TARGET_DIR=/workspace/target

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["/bin/bash"]
