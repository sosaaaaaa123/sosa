#!/bin/bash
# Crea l'installer per Mac "SosaTune-Installer-macOS.pkg" a partire dai
# plugin gia' compilati in target/bundled-mhbundle (vedi scripts/macos-bundle.sh).
#
# L'installer mette:
#   - Sosa Tune VST3 in /Library/Audio/Plug-Ins/VST3
#   - Sosa Tune CLAP in /Library/Audio/Plug-Ins/CLAP
# e alla fine toglie il "blocco di sicurezza" (quarantena) dai plugin, cosi'
# i DAW li caricano senza l'avviso "file dannoso".
set -euo pipefail

VERSION="${1:-1.0.0}"
SRC="target/bundled-mhbundle"
WORK="$(mktemp -d)"
OUT="SosaTune-Installer-macOS.pkg"
HERE="installer/macos"

for fmt in vst3 clap; do
  upper=$(echo "$fmt" | tr '[:lower:]' '[:upper:]')
  root="$WORK/root-$fmt"
  mkdir -p "$root"
  cp -R "$SRC/SosaTune.$fmt" "$root/"
  # I plugin non devono essere "spostabili": senza questo, se sul Mac c'e'
  # gia' una copia altrove, l'installer aggiornerebbe quella invece di
  # installare nella cartella giusta.
  pkgbuild --analyze --root "$root" "$WORK/$fmt.plist"
  # (Se pkgbuild non riconosce il plugin come "bundle" l'elenco e' vuoto e
  # i file vengono installati cosi' come sono, che e' gia' quello che serve.)
  i=0
  while /usr/libexec/PlistBuddy -c "Print :$i" "$WORK/$fmt.plist" >/dev/null 2>&1; do
    /usr/libexec/PlistBuddy -c "Set :$i:BundleIsRelocatable false" "$WORK/$fmt.plist" || true
    i=$((i + 1))
  done
  echo "componenti $fmt: $i"
  pkgbuild \
    --root "$root" \
    --component-plist "$WORK/$fmt.plist" \
    --install-location "/Library/Audio/Plug-Ins/$upper" \
    --identifier "com.vladislav.sosatune.$fmt" \
    --version "$VERSION" \
    --scripts "$HERE/scripts" \
    "$WORK/SosaTune-$fmt.pkg"
done

sed "s/@VERSION@/$VERSION/g" "$HERE/distribution.xml" > "$WORK/distribution.xml"
productbuild \
  --distribution "$WORK/distribution.xml" \
  --package-path "$WORK" \
  --resources "$HERE/resources" \
  "$OUT"

pkgutil --check-signature "$OUT" || true
pkgutil --payload-files "$OUT" | head -40
ls -la "$OUT"
