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
# musl di default linka staticamente: la libreria CUPS invece e' condivisa (libcups.so),
# quindi il binario resta dinamico contro la musl del sistema.
export RUSTFLAGS="-C target-feature=-crt-static"
# se la compilazione fallisce il messaggio finisce nelle annotazioni del run, cosi' si legge
# subito quale comando ha fallito (i log completi richiedono l'accesso)
set +e
./build-unix.sh > /tmp/build.log 2>&1
rc=$?
set -e
cat /tmp/build.log
if [ "$rc" -ne 0 ]; then
  echo "::error::$(grep -E 'error(\[|:)' /tmp/build.log | head -4 | tr '\n' ' ' | cut -c1-700)"
  echo "::error::$(tail -8 /tmp/build.log | tr '\n' ' ' | cut -c1-700)"
  exit 1
fi

# verifica del pacchetto: apk deve riuscire a risolvere tutte le dipendenze dichiarate
# (servono gli indici dei repository, che --no-cache non lascia su disco)
tar -tzf ./dist/*.apk | head -5
apk update >/dev/null 2>&1 || true
if ! out=$(apk add --allow-untrusted --simulate ./dist/*.apk 2>&1); then
  echo "::error::apk --simulate: $(printf '%s' "$out" | tr '\n' ' ' | cut -c1-700)"
  exit 1
fi
printf '%s\n' "$out" | tail -10
