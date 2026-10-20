#!/bin/sh
# Host side: copies the sample repositories to tests/e2e/work/, applies forge
# (built from this checkout) and commits the result. Run before docker build.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
(cd "$root" && bun run build >/dev/null)
rm -rf "$here/work" && mkdir -p "$here/work"
for app in fastapi-app nextjs-prisma-app; do
  dest="$here/work/$app"
  cp -R "$root/tests/generator/$app" "$dest"
  cp -R "$here/fixtures/$app/." "$dest/" 2>/dev/null || true
  (
    cd "$dest"
    git init -q -b main
    git add -A && git -c user.name=e2e -c user.email=e2e@example.com commit -qm "sample app"
    node "$root/dist/forge.js" apply --yes
    git add -A && git -c user.name=e2e -c user.email=e2e@example.com commit -qm "forge apply"
  )
done
echo "prepared: $here/work"
