#!/usr/bin/env bash
# Crea SosaTune.vst3 e SosaTune.clap per macOS con l'eseguibile in formato
# "bundle" (Mach-O MH_BUNDLE), lo stesso usato da JUCE e dalla maggior parte
# dei plugin commerciali. Rust normalmente produce una libreria dinamica
# (MH_DYLIB): Ableton e Reaper la accettano, ma alcuni DAW (FL Studio) no.
#
# Come funziona: compiliamo il plugin come libreria statica (.a) per Apple
# Silicon e per Intel, la colleghiamo con `clang -bundle` esportando solo i
# punti d'ingresso VST3/CLAP, uniamo le due architetture con `lipo` e firmiamo.
#
# Uso (sul Mac, dalla cartella del progetto, dopo `cargo xtask bundle-universal sosatune --release`):
#   scripts/macos-bundle.sh
set -euo pipefail

NAME=SosaTune
CRATE=sosatune
OUT=target/bundled-mhbundle
WORK=target/mhbundle-work
rm -rf "$OUT" "$WORK"
mkdir -p "$OUT" "$WORK"

# I simboli che il DAW cerca nel plugin.
cat > "$WORK/exports.txt" <<'SYMS'
_GetPluginFactory
_bundleEntry
_bundleExit
_clap_entry
SYMS

link_arch() {
    local target=$1 arch=$2 min=$3
    echo "== $target"
    # La libreria statica, e l'elenco delle librerie di sistema che le servono.
    cargo rustc -p "$CRATE" --release --lib --target "$target" --crate-type staticlib \
        -- --print native-static-libs 2> "$WORK/rustc-$arch.txt" || { cat "$WORK/rustc-$arch.txt"; exit 1; }
    local libs
    libs=$(sed -n 's/.*native-static-libs: //p' "$WORK/rustc-$arch.txt" | tail -1)
    if [ -z "$libs" ]; then
        libs="-framework AppKit -framework Foundation -framework QuartzCore -framework CoreGraphics -framework CoreFoundation -lobjc -liconv -lSystem -lc -lm"
    fi
    echo "librerie: $libs"
    # shellcheck disable=SC2086
    clang -bundle -arch "$arch" -mmacosx-version-min="$min" \
        -o "$WORK/$NAME-$arch" \
        -Wl,-u,_GetPluginFactory -Wl,-u,_bundleEntry -Wl,-u,_bundleExit -Wl,-u,_clap_entry \
        -Wl,-exported_symbols_list,"$WORK/exports.txt" \
        -Wl,-dead_strip \
        "target/$target/release/lib$CRATE.a" $libs
}

link_arch aarch64-apple-darwin arm64 11.0
link_arch x86_64-apple-darwin x86_64 10.13
lipo -create "$WORK/$NAME-arm64" "$WORK/$NAME-x86_64" -output "$WORK/$NAME"
file "$WORK/$NAME"

# Stessa struttura dei pacchetti creati da xtask, ma con il nuovo eseguibile.
for ext in vst3 clap; do
    src="target/bundled/$NAME.$ext"
    dst="$OUT/$NAME.$ext"
    cp -R "$src" "$dst"
    rm -rf "$dst/Contents/_CodeSignature"
    cp "$WORK/$NAME" "$dst/Contents/MacOS/$NAME"
    codesign --force --sign - --timestamp=none "$dst"
    codesign --verify --strict --verbose=2 "$dst"
done
echo "Pacchetti pronti in $OUT"
