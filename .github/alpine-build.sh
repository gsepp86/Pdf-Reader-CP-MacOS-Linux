#!/bin/sh
# Compila PDF Reader CP e crea il pacchetto .apk dentro un container Alpine (musl).
# Usato dal workflow: docker run --rm -v "$PWD:/src" -w /src alpine:3.20 sh /src/.github/alpine-build.sh
set -eu

# bash serve a build-unix.sh (Alpine ha solo la shell di busybox)
apk add --no-cache bash build-base pkgconf cups-dev libxkbcommon-dev wayland-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev mesa-dev \
  zstd tar gzip curl ca-certificates

# Rust aggiornato (quello dei repository di Alpine puo' essere piu' vecchio del minimo di egui)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"

chmod +x build-unix.sh
./build-unix.sh

# le dipendenze dichiarate nel .apk devono risolversi nei repository di Alpine
apk add --allow-untrusted --simulate ./dist/*.apk
