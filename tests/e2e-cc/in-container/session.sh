#!/bin/sh
# Runs inside the container, in /app. Usage: session.sh <name> <model> <max-turns>
# The prompt is /e2e/prompts/<name>.txt. Writes /e2e/out/<name>.jsonl (stream-json with hook
# events), <name>.err and <name>.debug (Claude Code's debug log, hook timings). The token comes
# from the environment (docker exec -e CLAUDE_CODE_OAUTH_TOKEN), never from argv.
set -u
name=$1 model=$2 turns=$3
export PATH="/root/.local/bin:$PATH"
cd /app || exit 1
start=$(date +%s)
claude -p "$(cat "/e2e/prompts/$name.txt")" \
  --model "$model" --max-turns "$turns" \
  --permission-mode bypassPermissions \
  --output-format stream-json --verbose --include-hook-events \
  --debug-file "/e2e/out/$name.debug" \
  >"/e2e/out/$name.jsonl" 2>"/e2e/out/$name.err" </dev/null
code=$?
echo "session $name: exit=$code, $(($(date +%s) - start))s"
# The project's state after the session, for the assertions (what the agent left in the files).
git -C /app status --short >"/e2e/out/$name.status"
git -C /app diff HEAD >"/e2e/out/$name.diff"
git -C /app log --format='%h %s' -5 >"/e2e/out/$name.log"
