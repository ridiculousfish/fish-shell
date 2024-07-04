#!/usr/bin/env bash

# Script to produce an OS X installer .pkg and .app(.zip)

usage() {
  echo "Usage: $0 [-s] -f <p12 file> -p <p12 password> [-e <entitlements file>]"
  exit 1
}

set -x
set -e

SIGN=

while getopts "sf:p:e:" opt; do
  case $opt in
    s) SIGN=1;;
    f) P12_FILE=$(realpath "$OPTARG");;
    p) P12_PASSWORD="$OPTARG";;
    e) ENTITLEMENTS_FILE=$(realpath "$OPTARG");;
    \?) usage;;
  esac
done

if [ -n "$SIGN" ] && ([ -z "$P12_FILE" ] || [ -z "$P12_PASSWORD" ]); then
  usage
fi


VERSION=$(git describe --always --dirty 2>/dev/null)
if test -z "$VERSION" ; then
  echo "Could not get version from git"
  if test -f version; then
    VERSION=$(cat version)
  fi
fi

echo "Version is $VERSION"


PKGDIR=$(mktemp -d)
echo "$PKGDIR"

SRC_DIR=$PWD
OUTPUT_PATH=${FISH_ARTEFACT_PATH:-~/fish_built}

mkdir -p "$PKGDIR/build" "$PKGDIR/root" "$PKGDIR/intermediates" "$PKGDIR/dst"

# Pass FISH_USE_SYSTEM_PCRE2=OFF because a system PCRE2 on macOS will not be signed by fish,
# and will probably not be built universal, so the package will fail to validate/run on other systems.
{ cd "$PKGDIR/build" && cmake -DMAC_INJECT_GET_TASK_ALLOW=OFF -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_EXE_LINKER_FLAGS="-Wl,-ld_classic" -DWITH_GETTEXT=OFF -DFISH_USE_SYSTEM_PCRE2=OFF -DCMAKE_OSX_ARCHITECTURES='arm64;x86_64' "$SRC_DIR" && make VERBOSE=1 -j 12 && env DESTDIR="$PKGDIR/root/" make install; }

if test -n "$SIGN"; then
    echo "Signing"
    ARGS=(
        --p12-file "$P12_FILE"
        --p12-password "$P12_PASSWORD"
        --code-signature-flags runtime
    )
    if [ -n "$ENTITLEMENTS_FILE" ]; then
        ARGS+=(--entitlements-xml-file "$ENTITLEMENTS_FILE")
    fi
    for FILE in "$PKGDIR"/root/usr/local/bin/*; do
        rcodesign sign "${ARGS[@]}" "$FILE"
    done
fi

pkgbuild --scripts "$SRC_DIR/build_tools/osx_package_scripts" --root "$PKGDIR/root/" --identifier 'com.ridiculousfish.fish-shell-pkg' --version "$VERSION" "$PKGDIR/intermediates/fish.pkg"
productbuild  --package-path "$PKGDIR/intermediates" --distribution "$SRC_DIR/build_tools/osx_distribution.xml" --resources "$SRC_DIR/build_tools/osx_package_resources/" "$OUTPUT_PATH/fish-$VERSION.pkg"

# MAC_PRODUCTSIGN_ID=${MAC_PRODUCTSIGN_ID:--}
# productsign --sign "${MAC_PRODUCTSIGN_ID}" "$OUTPUT_PATH/fish-$VERSION.pkg" "$OUTPUT_PATH/fish-$VERSION-signed.pkg" && mv "$OUTPUT_PATH/fish-$VERSION-signed.pkg" "$OUTPUT_PATH/fish-$VERSION.pkg"

# # Make the app
# { cd "$PKGDIR/build" && make -j 12 signed_fish_macapp && zip -r "$OUTPUT_PATH/fish-$VERSION.app.zip" fish.app; }

# rm -rf "$PKGDIR"
