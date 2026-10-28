FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential git pkg-config libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
    libayatana-appindicator3-dev librsvg2-dev patchelf curl wget file \
    ca-certificates python3 xvfb xauth dbus-x11 webkit2gtk-driver \
    libgl1-mesa-dri libegl1 fonts-dejavu-core && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 1000 builder || true
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg && rm -rf /var/lib/apt/lists/*
ENV HOME=/home/mudabbir
ENV PATH=/home/mudabbir/.cargo/bin:/opt/node/bin:$PATH
WORKDIR /workspace
