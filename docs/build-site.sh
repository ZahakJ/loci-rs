#!/bin/sh
# Builds the GitHub Pages site into OUT (default .scratch/site): the landing page from
# docs/site/ at the root, the captures under captures/, the manual (mdBook) under manual/, and the
# browser app under app/ when it has been built (apps/loci: pnpm build:wasm && pnpm build:web).
# The same script runs locally and in .github/workflows/pages.yml.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-$here/../.scratch/site}
case $out in /*) ;; *) out=$PWD/$out ;; esac
rm -rf "$out"
mkdir -p "$out/captures"
cp -R "$here/site/." "$out/"
cp -R "$here/captures/." "$out/captures/"
mkdir -p "$out/media" && cp "$here/media/banner.png" "$out/media/"
mdbook build "$here/manual" -d "$out/manual"
if [ -d "$here/../apps/loci/dist-web" ]; then
  mkdir -p "$out/app" && cp -R "$here/../apps/loci/dist-web/." "$out/app/"
fi
echo "site built in $out"
