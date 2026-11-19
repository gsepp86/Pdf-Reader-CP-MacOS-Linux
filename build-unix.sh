#!/usr/bin/env bash
# Compila PDF Reader CP per macOS e Linux e prepara i pacchetti in dist/.
#
#   ./build-unix.sh              compila per il sistema corrente
#   ./build-unix.sh --universal  solo macOS: binario universale (arm64 + x86_64)
#
# La libreria PDFium viene scaricata da bblanchon/pdfium-binaries e messa accanto
# all'eseguibile, perche' il programma la cerca nella propria cartella.
set -euo pipefail
cd "$(dirname "$0")"

UNIVERSAL=0
MODE=all
for a in "$@"; do
  case "$a" in
    --universal) UNIVERSAL=1 ;;
    --app) MODE=app ;;   # solo app e zip (niente installer)
    --dmg) MODE=dmg ;;   # solo installer DMG, usando l'app gia' compilata
    --pkg) MODE=pkg ;;   # solo installer PKG, usando l'app gia' compilata
    -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "Opzione sconosciuta: $a" >&2; exit 1 ;;
  esac
done

VERSION="$(sed -n 's/^version *= *"\(.*\)".*/\1/p' Cargo.toml | head -1)"
# versione usata nei pacchetti senza lo zero finale: 3.1.0 -> 3.1 (Cargo nel manifest
# richiede tutti e tre i numeri, ma il programma e le release si chiamano 3.1)
case "$VERSION" in *.*.0) VERSION="${VERSION%.0}" ;; esac
OS="$(uname -s)"
ARCH="$(uname -m)"
LIB=""; ASSET=""; TAG=""; DEB_ARCH=""

case "$OS" in
  Linux)
    LIB=libpdfium.so
    case "$ARCH" in
      x86_64|amd64) ASSET=pdfium-linux-x64;   TAG=linux-x64;   DEB_ARCH=amd64 ;;
      aarch64|arm64) ASSET=pdfium-linux-arm64; TAG=linux-arm64; DEB_ARCH=arm64 ;;
      *) echo "Architettura non supportata: $ARCH" >&2; exit 1 ;;
    esac ;;
  Darwin)
    LIB=libpdfium.dylib
    if [ "$UNIVERSAL" = 1 ]; then
      ASSET=pdfium-mac-univ; TAG=macos-universal
    else
      case "$ARCH" in
        arm64)  ASSET=pdfium-mac-arm64; TAG=macos-arm64 ;;
        x86_64) ASSET=pdfium-mac-x64;   TAG=macos-x64 ;;
        *) echo "Architettura non supportata: $ARCH" >&2; exit 1 ;;
      esac
    fi ;;
  *) echo "Sistema non supportato: $OS (su Windows usa build.bat)" >&2; exit 1 ;;
esac

mkdir -p dist vendor

# Installer macOS: DMG (si apre, si trascina l'app in Applicazioni) e PKG (doppio clic).
make_dmg() {
  local APP=dist/PDFReaderCP.app DMG="dist/PDFReaderCP-$VERSION-$TAG.dmg" STAGE=dist/dmg-stage
  if ! command -v hdiutil >/dev/null 2>&1; then echo "hdiutil non disponibile: DMG non creato" >&2; return 1; fi
  rm -rf "$STAGE" "$DMG"
  mkdir -p "$STAGE"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  cat > "$STAGE/LEGGIMI.txt" <<EOF
PDF Reader CP $VERSION (macOS 10.13 o successivo, Intel e Apple Silicon)

INSTALLAZIONE
1. Trascina PDFReaderCP nella cartella Applicazioni.
2. Al primo avvio fai clic destro sull'app > Apri (l'app e' firmata solo ad-hoc,
   senza certificato Apple), poi conferma Apri.

In alternativa, da Terminale:
   xattr -dr com.apple.quarantine /Applications/PDFReaderCP.app

Se usi il pacchetto .pkg basta il doppio clic: installa l'app in Applicazioni.
EOF
  hdiutil create -volname "PDF Reader CP $VERSION" -srcfolder "$STAGE" -format UDZO -ov "$DMG"
  rm -rf "$STAGE"
  echo "Creato $DMG"
}

make_pkg() {
  local APP=dist/PDFReaderCP.app PKG="dist/PDFReaderCP-$VERSION-$TAG.pkg"
  rm -f "$PKG"
  if command -v productbuild >/dev/null 2>&1; then
    # il pacchetto installa il bundle direttamente in /Applications
    productbuild --component "$APP" /Applications "$PKG"
    echo "Creato $PKG"
    return 0
  fi
  if ! command -v pkgbuild >/dev/null 2>&1; then echo "ne' productbuild ne' pkgbuild disponibili: PKG non creato" >&2; return 1; fi
  local PKGROOT=dist/pkg-root
  rm -rf "$PKGROOT"
  mkdir -p "$PKGROOT/Applications"
  cp -R "$APP" "$PKGROOT/Applications/"
  # i file installati devono appartenere a root:wheel, non all'utente del runner
  if command -v sudo >/dev/null 2>&1; then sudo -n chown -R root:wheel "$PKGROOT" 2>/dev/null || true; fi
  pkgbuild --root "$PKGROOT" --identifier com.peppecp.pdfreadercp --version "$VERSION" \
    --install-location / "$PKG"
  rm -rf "$PKGROOT"
  echo "Creato $PKG"
}

# Richiesta di un solo installer (usato dal workflow per avere passaggi separati):
# l'app deve essere gia' stata compilata con `--universal --app`.
if [ "$MODE" = dmg ] || [ "$MODE" = pkg ]; then
  if [ "$OS" != Darwin ]; then echo "Gli installer DMG/PKG si creano solo su macOS" >&2; exit 1; fi
  if [ ! -d dist/PDFReaderCP.app ]; then echo "Manca dist/PDFReaderCP.app: esegui prima ./build-unix.sh --universal --app" >&2; exit 1; fi
  if [ "$MODE" = dmg ]; then make_dmg; else make_pkg; fi
  echo; echo "Pacchetti pronti in dist/:"; ls -1 dist | sed 's/^/  /'
  exit 0
fi

echo "== PDF Reader CP $VERSION - $OS $TAG =="

# 1. PDFium accanto all'eseguibile
if [ ! -f "vendor/$LIB" ]; then
  echo "Scarico $ASSET..."
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  curl -L --fail --silent --show-error -o "$TMP/pdfium.tgz" \
    "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/$ASSET.tgz"
  tar -xzf "$TMP/pdfium.tgz" -C "$TMP"
  cp "$TMP/lib/$LIB" "vendor/$LIB"
  rm -rf "$TMP"
  trap - EXIT
fi

# 2. compilazione
if [ "$UNIVERSAL" = 1 ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  BIN=target/PDFReaderCP-universale
  lipo -create -output "$BIN" \
    target/aarch64-apple-darwin/release/PDFReaderCP \
    target/x86_64-apple-darwin/release/PDFReaderCP
else
  cargo build --release
  BIN=target/release/PDFReaderCP
fi

# 3. pacchetto
if [ "$OS" = Linux ]; then
  PKG="dist/PDFReaderCP-$VERSION-$TAG"
  rm -rf "$PKG"
  mkdir -p "$PKG"
  cp "$BIN" "$PKG/PDFReaderCP"
  cp "vendor/$LIB" "$PKG/"
  chmod +x "$PKG/PDFReaderCP"
  cat > "$PKG/LEGGIMI.txt" <<EOF
PDF Reader CP $VERSION ($TAG)

Avvio:  ./PDFReaderCP [file.pdf]

La libreria $LIB deve restare nella stessa cartella dell'eseguibile.
La stampa usa CUPS (pacchetto cups-client). Le opzioni della stampante
(fronte/retro, vassoio, qualita') si impostano nella coda CUPS, per esempio:
    lpoptions -d NOME_STAMPANTE -l
EOF
  tar -C dist -czf "$PKG.tar.gz" "$(basename "$PKG")"
  echo "Creato $PKG.tar.gz"

  if command -v dpkg-deb >/dev/null 2>&1; then
    ROOT=dist/deb-root
    rm -rf "$ROOT"
    mkdir -p "$ROOT/DEBIAN" "$ROOT/usr/lib/pdfreadercp" "$ROOT/usr/bin" \
      "$ROOT/usr/share/applications" "$ROOT/usr/share/icons/hicolor/256x256/apps" \
      "$ROOT/usr/share/doc/pdf-reader-cp"
    cp "$BIN" "$ROOT/usr/lib/pdfreadercp/PDFReaderCP"
    cp "vendor/$LIB" "$ROOT/usr/lib/pdfreadercp/"
    chmod +x "$ROOT/usr/lib/pdfreadercp/PDFReaderCP"
    printf '#!/bin/sh\nexec /usr/lib/pdfreadercp/PDFReaderCP "$@"\n' > "$ROOT/usr/bin/pdfreadercp"
    chmod +x "$ROOT/usr/bin/pdfreadercp"
    cp unix/pdfreader-cp.desktop "$ROOT/usr/share/applications/"
    cp assets/icon.png "$ROOT/usr/share/icons/hicolor/256x256/apps/pdf-reader-cp.png"
    cp README-UNIX.md "$ROOT/usr/share/doc/pdf-reader-cp/README.md"
    cat > "$ROOT/DEBIAN/control" <<EOF
Package: pdf-reader-cp
Version: $VERSION
Section: office
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Peppe CP <https://github.com/gsepp86/Pdf-Reader-CP>
Depends: libc6, libstdc++6, cups-client | cups, libgl1, libxkbcommon0, libx11-6, libxcursor1, libxrandr2, libxi6, fonts-dejavu-core | fonts-liberation
Description: Lettore PDF a schede con stampa rapida
 Lettore PDF con schede, stampa immediata di tutte le schede aperte e
 anteprima di stampa. La stampa passa da CUPS.
EOF
    DEB="dist/PDFReaderCP_${VERSION}_${DEB_ARCH}.deb"
    if ! dpkg-deb --root-owner-group --build "$ROOT" "$DEB" >/dev/null 2>&1; then
      dpkg-deb --build "$ROOT" "$DEB" >/dev/null
    fi
    rm -rf "$ROOT"
    echo "Creato $DEB"
  fi
fi

if [ "$OS" = Darwin ]; then
  APP=dist/PDFReaderCP.app
  rm -rf "$APP"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp "$BIN" "$APP/Contents/MacOS/PDFReaderCP"
  cp "vendor/$LIB" "$APP/Contents/MacOS/"
  chmod +x "$APP/Contents/MacOS/PDFReaderCP"
  sed "s/@VERSION@/$VERSION/g" unix/Info.plist > "$APP/Contents/Info.plist"

  if command -v iconutil >/dev/null 2>&1 && command -v sips >/dev/null 2>&1; then
    ICONSET=dist/icon.iconset
    rm -rf "$ICONSET"
    mkdir -p "$ICONSET"
    for s in 16 32 128 256 512; do
      sips -z "$s" "$s" assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null 2>&1
      d=$((s * 2))
      sips -z "$d" "$d" assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null 2>&1
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
    rm -rf "$ICONSET"
  fi

  # firma ad-hoc: senza certificato Apple il primo avvio va sbloccato (vedi README-UNIX.md)
  codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

  ZIP="dist/PDFReaderCP-$VERSION-$TAG.zip"
  rm -f "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP"
  echo "Creato $ZIP"

  # Installer (solo nella build completa; con --app si ottiene solo app e zip)
  if [ "$MODE" = all ]; then
    make_dmg
    make_pkg
  fi
fi

echo
echo "Pacchetti pronti in dist/:"
ls -1 dist | sed 's/^/  /'
