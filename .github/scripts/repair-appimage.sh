#!/usr/bin/env bash

set -euo pipefail

appimage="${1:?usage: repair-appimage.sh <path-to-appimage>}"
if [[ ! -s "$appimage" ]]; then
  echo "AppImage is missing or empty: $appimage" >&2
  exit 1
fi
appimage="$(realpath "$appimage")"

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT
chmod +x "$appimage"
(cd "$temp_dir" && "$appimage" --appimage-extract >/dev/null)

# linuxdeploy bundles Ubuntu's low-level graphics libraries but leaves EGL,
# Mesa and the GPU driver partially host-provided. WebKitGPUProcess then loads
# an ABI-incompatible mixture on rolling distributions and aborts at startup.
lib_dir="$temp_dir/squashfs-root/usr/lib"
test -d "$lib_dir"
shopt -s nullglob
removed=0
for pattern in \
  'libEGL.so*' 'libEGL_mesa.so*' 'libGL.so*' 'libGLX.so*' \
  'libGLdispatch.so*' 'libGLESv2.so*' 'libgbm.so*' 'libdrm.so*' \
  'libwayland-client.so*' 'libwayland-cursor.so*' \
  'libwayland-egl.so*' 'libwayland-server.so*'; do
  matches=("$lib_dir"/$pattern)
  if ((${#matches[@]} > 0)); then
    rm -f -- "${matches[@]}"
    removed=$((removed + ${#matches[@]}))
  fi
done
if ((removed == 0)); then
  echo "AppImage contained no expected host graphics library overrides" >&2
  exit 1
fi
echo "Removed $removed host graphics library overrides from AppImage"

appimagetool="$temp_dir/appimagetool-x86_64.AppImage"
curl --fail --location --retry 3 --output "$appimagetool" \
  "https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
echo "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  $appimagetool" | sha256sum -c -
chmod +x "$appimagetool"

repaired="$temp_dir/repaired.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 \
  "$appimagetool" "$temp_dir/squashfs-root" "$repaired"
install -m 755 "$repaired" "$appimage"
echo "Repacked $appimage"
