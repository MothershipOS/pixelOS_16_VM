# Mesa for the Android guest (cargo xtask android-mesa): meson >= 1.4 (pip) and Mesa's build tools; the NDK is
# mounted with the sources.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq \
 && apt-get install -y -qq --no-install-recommends python3-pip python3-mako python3-yaml python3-packaging glslang-tools \
	bison flex pkg-config git ninja-build ca-certificates \
 && pip install -q --break-system-packages meson==1.12.1 \
 && rm -rf /var/lib/apt/lists/*
