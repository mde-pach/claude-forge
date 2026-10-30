#!/bin/sh
# Runs inside the container, in /app, as root. Usage: setup.sh <claude-code-version>
# Installs Claude Code <version> when the image's is older than Forge's minimum, applies Forge
# (user level, then the project), commits, and marks /app trusted so `claude -p` runs unattended.
set -eu
want=$1
min=2.1.295
export PATH="/root/.local/bin:$PATH"

older() { # older A B: A < B as dotted versions
  [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -1)" = "$1" ]
}
have=$(claude --version 2>/dev/null | cut -d' ' -f1 || true)
if [ -z "$have" ] || older "$have" "$min" || { [ "$want" != "keep" ] && [ "$have" != "$want" ]; }; then
  v=$want; [ "$v" = keep ] && v=stable
  curl -fsSL https://downloads.claude.ai/claude-code-releases/bootstrap.sh | bash -s -- "$v" >/tmp/cc-install.log 2>&1 \
    || { tail -5 /tmp/cc-install.log; exit 1; }
  hash -r 2>/dev/null || true
fi
echo "claude: $(claude --version)"
echo "forge: $(claude-forge --version 2>&1 | head -1)"

cd /app
git config --global user.name e2e
git config --global user.email e2e@example.invalid
git config --global init.defaultBranch main

claude-forge apply --user --yes >/e2e/out/apply-user.log 2>&1 || { cat /e2e/out/apply-user.log; exit 1; }
claude-forge apply --yes >/e2e/out/apply.log 2>&1 || { cat /e2e/out/apply.log; exit 1; }
git add -A
git commit -qm "e2e baseline: quality tooling applied"
git show --stat --format='applied: %h %s' HEAD | head -20

# Onboarding and trust, as an interactive first run would record them.
python3 - <<'EOF'
import json, os
p = "/root/.claude.json"
d = json.load(open(p)) if os.path.exists(p) else {}
d["hasCompletedOnboarding"] = True
d.setdefault("projects", {}).setdefault("/app", {}).update(
    hasTrustDialogAccepted=True, hasCompletedProjectOnboarding=True
)
json.dump(d, open(p, "w"))
EOF
