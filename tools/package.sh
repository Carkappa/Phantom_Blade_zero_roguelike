#!/usr/bin/env bash
# Builds the folder players install: dist/PBZRoguelike/ (and a zip of it when zip or 7z is found).
#
#   tools/package.sh <path to pbz-roguelike.exe>
#
# The folder goes into <game>/PBZ/platform/steam/ue4ss/Mods/. The program must be the Windows build;
# .github/workflows/build.yml makes it and runs this script.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
exe="${1:?usage: tools/package.sh <path to pbz-roguelike.exe>}"
[ -f "$exe" ] || { echo "no such file: $exe" >&2; exit 1; }
case "$exe" in
  *.exe) ;;
  *) echo "warning: $exe is not a Windows program; this package will not run on a player's PC" >&2 ;;
esac

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)"
[ -n "$version" ] || { echo "cannot read the version from Cargo.toml" >&2; exit 1; }
# The bridge and the director must agree on their version.
bridge="$(sed -n "s/^P\.VERSION = '\(.*\)'/\1/p" "$root/mod/PBZRoguelike/Scripts/proto.lua")"
[ "$bridge" = "$version" ] || { echo "version mismatch: Cargo.toml $version, proto.lua $bridge" >&2; exit 1; }

out="$root/dist/PBZRoguelike"
rm -rf "$root/dist"
mkdir -p "$out/Scripts" "$out/Runtime" "$out/data"
cp "$root"/mod/PBZRoguelike/Scripts/*.lua "$out/Scripts/"
cp "$root/mod/PBZRoguelike/Runtime/README.txt" "$out/Runtime/"
cp "$root/mod/PBZRoguelike/enabled.txt" "$out/"
cp "$root"/data/*.json "$out/data/"
cp "$exe" "$out/"
cp "$root/README.md" "$root/LICENSE" "$root/CREDITS.md" "$out/"

# Nothing but what a player needs: no saves, logs or bridge files from a test session.
for stray in inbox.tsv state.tsv run.json profile.json bridge.log director.log; do
  [ ! -e "$out/Runtime/$stray" ] || { echo "stray file in the package: Runtime/$stray" >&2; exit 1; }
done

zipfile="$root/dist/PBZRoguelike-$version.zip"
if command -v zip >/dev/null 2>&1; then
  (cd "$root/dist" && zip -qr "$zipfile" PBZRoguelike)
elif command -v 7z >/dev/null 2>&1; then
  (cd "$root/dist" && 7z a -bso0 "$zipfile" PBZRoguelike)
else
  zipfile=""
fi
echo "packaged: $out"
[ -z "$zipfile" ] || echo "zip: $zipfile"
