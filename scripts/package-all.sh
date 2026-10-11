#!/usr/bin/env bash
# Package everything in dist/bin/ (see scripts/build-all.sh) into
# installable artifacts under dist/:
#   linux   -> .tar.gz (install.sh) + .deb + .rpm
#   windows -> NSIS -setup.exe + unattended zip (exe + install.ps1)
#   macos   -> pkgbuild .pkg (universal binaries + launchd jobs)
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
BIN=dist/bin
OUT=dist
have() { command -v "$1" >/dev/null 2>&1; }
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
BUILT=()

note() { echo "  -> $1"; BUILT+=("$1"); }

# ---------------------------------------------------------------- linux ----
linux_all() { # $1=triple $2=deb-arch $3=rpm-arch
  local triple=$1 debarch=$2 rpmarch=$3
  local d="$BIN/$triple"
  [ -f "$d/eduguard-server" ] || return 0

  for comp in server agent; do
    local name="eduguard-$comp"

    # --- tarball -----------------------------------------------------------
    local root="$STAGE/tar/eg-$name-$VERSION-$triple"
    mkdir -p "$root"
    cp "$d/$name" "$root/"
    cp "packaging/linux/$name.service" "$root/"
    cp packaging/linux/install.sh "$root/"
    tar -czf "$OUT/$name-$VERSION-$triple.tar.gz" -C "$STAGE/tar" "eg-$name-$VERSION-$triple"
    note "$OUT/$name-$VERSION-$triple.tar.gz"

    # --- deb ---------------------------------------------------------------
    if have dpkg-deb; then
      local debroot="$STAGE/deb/$name"
      rm -rf "$debroot"
      mkdir -p "$debroot/DEBIAN" "$debroot/usr/bin" \
               "$debroot/etc/eduguard" "$debroot/var/lib/eduguard" \
               "$debroot/usr/lib/systemd/system"
      install -m755 "$d/$name" "$debroot/usr/bin/$name"
      install -m644 "packaging/linux/$name.service" \
        "$debroot/usr/lib/systemd/system/$name.service"
      install -m755 "packaging/linux/postinst-$comp" "$debroot/DEBIAN/postinst"
      install -m755 "packaging/linux/prerm-$comp"    "$debroot/DEBIAN/prerm"
      cat > "$debroot/DEBIAN/control" <<EOF
Package: $name
Version: $VERSION
Section: admin
Priority: optional
Architecture: $debarch
Maintainer: EduGuard <dev@eduguard.local>
Description: EduGuard $comp ($triple)
 Classroom safeguarding suite component. See https://github.com/eduguard/eduguard
EOF
      dpkg-deb --build --root-owner-group "$debroot" \
        "$OUT/$name-$VERSION-$debarch.deb" >/dev/null
      note "$OUT/$name-$VERSION-$debarch.deb"
    fi

    # --- rpm ---------------------------------------------------------------
    if have rpmbuild; then
      local top="$STAGE/rpm-$comp"
      local src="$STAGE/rpm-src-$comp"
      rm -rf "$top" "$src"
      mkdir -p "$top"/{SPECS,BUILD,RPMS,SOURCES,SRPMS,BUILDROOT} "$src"
      cp "$d/$name" "$src/"
      cp "packaging/linux/$name.service" "$src/"
      # rpm 6 factory rpmrc limits builds to the host arch ("buildarch_compat:
      # aarch64: noarch"). Override per-build via an rpmrc loaded through
      # XDG_CONFIG_HOME — rpmrc directives are "last entry wins".
      mkdir -p "$STAGE/xdg/rpm"
      cat > "$STAGE/xdg/rpm/rpmrc" <<'RPMRC'
buildarch_compat: aarch64: aarch64 x86_64 arm64 noarch
buildarch_compat: x86_64: aarch64 x86_64 arm64 noarch
RPMRC
      XDG_CONFIG_HOME="$STAGE/xdg" \
      rpmbuild --define "_topdir $top" \
               --define "_eg_version $VERSION" \
               --define "_eg_src $src" \
               --define "_eg_arch $rpmarch" \
               --target "$rpmarch" \
               -bb "packaging/linux/$name.spec" >/dev/null
      local rpm
      rpm=$(find "$top/RPMS" -name "*.rpm" | head -1)
      [ -n "$rpm" ] && cp "$rpm" "$OUT/" && note "$OUT/$(basename "$rpm")"
    fi
  done
}

linux_all x86_64-unknown-linux-gnu  amd64  x86_64
linux_all aarch64-unknown-linux-gnu arm64  aarch64

# Stage server binaries into the Home Assistant add-on folder — the
# Supervisor builds the container image on-device from that Dockerfile.
addon_stage() {
  local addon="packaging/homeassistant/eduguard-server/bin"
  mkdir -p "$addon/x86_64" "$addon/aarch64"
  if [ -f "$BIN/x86_64-unknown-linux-gnu/eduguard-server" ]; then
    cp "$BIN/x86_64-unknown-linux-gnu/eduguard-server" "$addon/x86_64/"
  fi
  if [ -f "$BIN/aarch64-unknown-linux-gnu/eduguard-server" ]; then
    cp "$BIN/aarch64-unknown-linux-gnu/eduguard-server" "$addon/aarch64/"
  fi
}
addon_stage

# --------------------------------------------------------------- windows ---
windows_all() {
  local d="$BIN/x86_64-pc-windows-gnu"
  [ -f "$d/eduguard-server.exe" ] || return 0

  # unattended zip: exe + PowerShell installer
  for comp in server agent; do
    local name="eduguard-$comp"
    rm -rf "$STAGE/zip"; mkdir -p "$STAGE/zip"
    cp "$d/$name.exe" "$STAGE/zip/"
    cp packaging/windows/install.ps1 "$STAGE/zip/"
    if [ "$comp" = server ]; then
      cp packaging/windows/start-server.cmd "$STAGE/zip/"
    fi
    (cd "$STAGE/zip" && zip -q -r "$STAGE/$name-$VERSION-win64.zip" .)
    mv "$STAGE/$name-$VERSION-win64.zip" "$OUT/"
    note "$OUT/$name-$VERSION-win64.zip"
  done

  # NSIS setup.exe
  if have makensis; then
    for comp in server agent; do
      local name="eduguard-$comp"
      local ns="$STAGE/nsis-$comp"
      rm -rf "$ns"; mkdir -p "$ns"
      cp "$d/$name.exe" "$ns/"
      cp "packaging/windows/$name.nsi" "$ns/"
      [ "$comp" = server ] && cp packaging/windows/start-server.cmd "$ns/"
      ( cd "$ns" && makensis -DPRODUCT_VERSION="$VERSION" "$name.nsi" >/dev/null )
      mv "$ns/$name-$VERSION-setup.exe" "$OUT/"
      note "$OUT/$name-$VERSION-setup.exe"
    done
  fi
}

windows_all

# ----------------------------------------------------------------- macos ---
macos_pkg() {
  local d="$BIN/universal-apple-darwin"
  [ -f "$d/eduguard-server" ] || return 0
  have pkgbuild || return 0

  local root="$STAGE/pkgroot"
  rm -rf "$root"
  mkdir -p "$root/usr/local/bin" \
           "$root/Library/LaunchDaemons" \
           "$root/Library/LaunchAgents"
  install -m755 "$d/eduguard-server" "$root/usr/local/bin/"
  install -m755 "$d/eduguard-agent"  "$root/usr/local/bin/"
  install -m644 packaging/macos/com.eduguard.server.plist "$root/Library/LaunchDaemons/"
  install -m644 packaging/macos/com.eduguard.agent.plist  "$root/Library/LaunchAgents/"

  local scripts="$STAGE/pkgscripts"
  rm -rf "$scripts"; mkdir -p "$scripts"
  cp packaging/macos/postinstall.sh "$scripts/postinstall"
  chmod +x "$scripts/postinstall"

  # macOS materializes xattrs as AppleDouble `._*` entries in the payload;
  # strip them so only real files are installed (and pkgbuild stays quiet).
  xattr -cr "$root" 2>/dev/null || true
  xattr -cr "$scripts" 2>/dev/null || true
  find "$root" "$scripts" -name '._*' -delete 2>/dev/null || true

  pkgbuild --root "$root" \
           --identifier com.eduguard.pkg \
           --version "$VERSION" \
           --scripts "$scripts" \
           --install-location / \
           "$OUT/eduguard-$VERSION-macos-universal.pkg" >/dev/null
  note "$OUT/eduguard-$VERSION-macos-universal.pkg"
}

macos_pkg

echo
echo "EduGuard $VERSION artifacts:"
for f in "${BUILT[@]}"; do
  printf '  %-55s %8s\n' "$(basename "$f")" "$(du -h "$f" | cut -f1)"
done
