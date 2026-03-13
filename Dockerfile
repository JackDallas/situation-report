# syntax=docker/dockerfile:1.4
# Situation Report Docker Build
#
# Build from repo root with BuildKit:
#   DOCKER_BUILDKIT=1 docker build -t situationreport .
#
# Uses ort-load-dynamic: ONNX Runtime is loaded at runtime (not linked statically).
# The GPU-enabled ORT shared library is installed in the runtime image.

# ============================================================================
# Stage 1: Build the SvelteKit UI
# ============================================================================
FROM node:22-alpine AS ui-builder

RUN corepack enable && corepack prepare pnpm@latest --activate

WORKDIR /app/frontend

# Copy package files first for layer caching
COPY frontend/package.json frontend/pnpm-lock.yaml ./

# Install dependencies with pnpm store cache mount
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile

# Copy source and build
COPY frontend/ ./
RUN pnpm run build

# ============================================================================
# Stage 2: cargo-chef prepare (compute dependency recipe)
# ============================================================================
FROM rust:1-trixie AS chef

RUN cargo install cargo-chef

WORKDIR /app/backend

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/crates/ ./crates/

RUN cargo chef prepare --recipe-path recipe.json

# ============================================================================
# Stage 3: cargo-chef cook (build only dependencies)
# ============================================================================
FROM rust:1-trixie AS cook

RUN cargo install cargo-chef && \
    apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app/backend

COPY --from=chef /app/backend/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --recipe-path recipe.json

# ============================================================================
# Stage 4: Build the actual binary
# ============================================================================
FROM cook AS builder

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/crates/ ./crates/
COPY backend/migrations/ ./migrations/
COPY backend/.sqlx/ ./.sqlx/

ENV SQLX_OFFLINE=true

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --bin sr-server && \
    cp target/release/sr-server /tmp/sr-server

COPY --from=ui-builder /app/frontend/build /tmp/static

# ============================================================================
# Stage 5: Runtime image with ONNX Runtime GPU
# ============================================================================
# NVIDIA CUDA runtime provides cublas, cudnn, cufft, cudart — required for ORT CUDA EP.
# Host driver 590.48 (CUDA 13.1) is backward-compatible with CUDA 12.x images.
FROM nvidia/cuda:12.6.3-cudnn-runtime-ubuntu24.04 AS runtime

# Install ONNX Runtime GPU shared library (matches ort-sys 2.0.0-rc.11 = ORT 1.23)
ARG ORT_VERSION=1.23.0
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget libgomp1 \
    && mkdir -p /opt/onnxruntime \
    && wget -qO- "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-x64-gpu-${ORT_VERSION}.tgz" \
    | tar xz -C /opt/onnxruntime --strip-components=1 \
    && rm -rf /var/lib/apt/lists/*

# Point ort load-dynamic at the GPU shared library
ENV ORT_DYLIB_PATH=/opt/onnxruntime/lib/libonnxruntime.so
ENV LD_LIBRARY_PATH=/opt/onnxruntime/lib

# Create non-root user and application directories
RUN useradd -ms /bin/bash sitrep && \
    mkdir -p /app/static /app/migrations /app/models /app/data && \
    chown -R sitrep:sitrep /app

WORKDIR /app

COPY --from=builder /tmp/sr-server /app/sr-server
COPY --from=builder /tmp/static /app/static
COPY backend/migrations/ /app/migrations/

RUN chmod +x /app/sr-server && \
    chown -R sitrep:sitrep /app

USER sitrep

ENV RUST_LOG=info
ENV STATIC_DIR=/app/static
ENV BIND_ADDR=0.0.0.0:3000

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=10s --start-period=120s --retries=3 \
    CMD wget -q --spider http://localhost:3000/api/stats || exit 1

CMD ["/app/sr-server"]
