# Keyform dev container: Rust toolchain plus a software Vulkan driver
# (Mesa lavapipe), so the whole workspace builds and the headless tests run,
# GPU renderer included. The GUI app is run natively, not in here.
#   docker compose up            # build + test
#   docker compose run dev bash  # shell
FROM rust:1.99-bookworm

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      mesa-vulkan-drivers libvulkan1 \
      libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev \
      libxrandr-dev libxi-dev libgl1-mesa-dev pkg-config xz-utils \
 && rm -rf /var/lib/apt/lists/* \
 && rustup component add clippy rustfmt

# wgpu picks lavapipe (a CPU Vulkan device) when no GPU is passed in.
ENV WGPU_BACKEND=vulkan \
    CARGO_TARGET_DIR=/target \
    CARGO_TERM_COLOR=always

WORKDIR /src
CMD ["cargo", "test", "--workspace", "--locked"]
