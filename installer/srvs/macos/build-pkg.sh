#!/bin/bash
# Crea l'installer per Mac "SRVocalSuite-Installer-macOS.pkg" a partire dai
# plugin gia' compilati in target/bundled-mhbundle (vedi scripts/macos-bundle.sh).
#
# L'installer mette:
#   - SR Vocal Suite VST3 in /Library/Audio/Plug-Ins/VST3
#   - SR Vocal Suite CLAP in /Library/Audio/Plug-Ins/CLAP
#   - SR Vocal Suite Audio Unit (Logic Pro, GarageBand) in /Library/Audio/Plug-Ins/Components
# e alla fine toglie il "blocco di sicurezza" (quarantena) dai plugin, cosi'
# i DAW li caricano senza l'avviso "file dannoso".
set -euo pipefail

VERSION="${1:-1.0.0}"
SRC="target/bundled-mhbundle"
WORK="$(mktemp -d)"
OUT="SRVocalSuite-Installer-macOS.pkg"
HERE="installer/srvs/macos"

for fmt in vst3 clap component; do
  case "$fmt" in
    vst3) upper="VST3" ;;
    clap) upper="CLAP" ;;
    component) upper="Components" ;;
  esac
  if [ ! -d "$SRC/SRVocalSuite.$fmt" ]; then
    echo "manca $SRC/SRVocalSuite.$fmt" >&2
    exit 1
  fi
  root="$WORK/root-$fmt"
  mkdir -p "$root"
  cp -R "$SRC/SRVocalSuite.$fmt" "$root/"
  # I plugin non devono essere "spostabili": senza questo, se sul Mac c'e'
  # gia' una copia altrove, l'installer aggiornerebbe quella invece di
  # installare nella cartella giusta.
  pkgbuild --analyze --root "$root" "$WORK/$fmt.plist"
  # (Se pkgbuild non riconosce il plugin come "bundle" l'elenco e' vuoto e
  # i file vengono installati cosi' come sono, che e' gia' quello che serve.)
  i=0
  while /usr/libexec/PlistBuddy -c "Print :$i" "$WORK/$fmt.plist" >/dev/null 2>&1; do
    /usr/libexec/PlistBuddy -c "Set :$i:BundleIsRelocatable false" "$WORK/$fmt.plist" 2>/dev/null \
      || /usr/libexec/PlistBuddy -c "Add :$i:BundleIsRelocatable bool false" "$WORK/$fmt.plist"
    i=$((i + 1))
  done
  echo "componenti $fmt: $i"; /usr/libexec/PlistBuddy -c Print "$WORK/$fmt.plist"
  pkgbuild \
    --root "$root" \
    --component-plist "$WORK/$fmt.plist" \
    --install-location "/Library/Audio/Plug-Ins/$upper" \
    --identifier "com.vladislav.srvocalsuite.$fmt" \
    --version "$VERSION" \
    --scripts "$HERE/scripts" \
    "$WORK/SRVocalSuite-$fmt.pkg"
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
