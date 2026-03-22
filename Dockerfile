# syntax=docker/dockerfile:1.4
# Situation Report Docker Build
#
# Build from repo root with BuildKit:
#   DOCKER_BUILDKIT=1 docker build -t situationreport .
#
# Uses ort-load-dynamic: ONNX Runtime is loaded at runtime (not linked statically).
# The GPU-enabled ORT shared library is installed in the runtime image.
#
# llama-cpp-2 with CUDA: compiled in-process, linked against CUDA toolkit.
# The CUDA dev image is used for build stages; the runtime image has CUDA runtime libs.

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
# Use CUDA devel image for llama-cpp-2 CUDA compilation.
# Install Rust toolchain manually since there's no official rust+cuda image.
FROM nvidia/cuda:12.6.3-devel-ubuntu24.04 AS cook

# Install build essentials, Rust, cmake, clang (for llama.cpp bindgen)
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl build-essential pkg-config libssl-dev cmake clang \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && rm -rf /var/lib/apt/lists/*

ENV PATH="/root/.cargo/bin:${PATH}"
ENV CUDA_PATH=/usr/local/cuda
ENV CUDA_HOME=/usr/local/cuda

RUN cargo install cargo-chef

WORKDIR /app/backend

COPY --from=chef /app/backend/recipe.json recipe.json

# Cook with llm-cuda feature so llama-cpp-sys-2 CUDA build is cached
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    cargo chef cook --release --recipe-path recipe.json --features sr-intel/llm-cuda

# ============================================================================
# Stage 4: Build the actual binary
# ============================================================================
FROM cook AS builder

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/crates/ ./crates/
COPY backend/migrations/ ./migrations/
COPY backend/.sqlx/ ./.sqlx/

ENV SQLX_OFFLINE=true

# Build with llm-cuda feature for in-process GPU inference via llama-cpp-2
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    cargo build --release --bin sr-server --features sr-intel/llm-cuda && \
    cp target/release/sr-server /tmp/sr-server

COPY --from=ui-builder /app/frontend/build /tmp/static

# ============================================================================
# Stage 5: Runtime image with ONNX Runtime GPU + CUDA runtime (for llama-cpp-2)
# ============================================================================
# NVIDIA CUDA runtime provides cublas, cudnn, cufft, cudart — required for both
# ORT CUDA EP and llama-cpp-2 in-process GPU inference.
# Host driver 590.48 (CUDA 13.1) is backward-compatible with CUDA 12.x images.
FROM nvidia/cuda:12.6.3-cudnn-runtime-ubuntu24.04 AS runtime

# Install ONNX Runtime GPU shared library (matches ort-sys 2.0.0-rc.11 = ORT 1.23)
ARG ORT_VERSION=1.23.0
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget libgomp1 libcublas-12-6 \
    && mkdir -p /opt/onnxruntime \
    && wget -qO- "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-x64-gpu-${ORT_VERSION}.tgz" \
    | tar xz -C /opt/onnxruntime --strip-components=1 \
    && rm -rf /var/lib/apt/lists/*

# Point ort load-dynamic at the GPU shared library
ENV ORT_DYLIB_PATH=/opt/onnxruntime/lib/libonnxruntime.so
ENV LD_LIBRARY_PATH=/opt/onnxruntime/lib:/usr/local/cuda/lib64

# Create non-root user and application directories
RUN useradd -ms /bin/bash sitrep && \
    mkdir -p /app/static /app/migrations /app/models /app/data /app/llm-models && \
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
