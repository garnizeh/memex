#!/usr/bin/env bash
# Install Git hooks for automatic background indexing with Memex.
# Hooks: post-commit, post-merge, post-checkout

set -euo pipefail

GIT_DIR="$(git rev-parse --git-dir 2>/dev/null || true)"

if [ -z "$GIT_DIR" ]; then
    echo "Error: Not inside a Git repository." >&2
    exit 1
fi

HOOKS_DIR="$GIT_DIR/hooks"
mkdir -p "$HOOKS_DIR"

HOOK_CONTENT='#!/usr/bin/env bash
# Trigger Memex background incremental re-indexing
if command -v memex >/dev/null 2>&1; then
    (memex index --quiet >/dev/null 2>&1 &)
fi
'

HOOKS=("post-commit" "post-merge" "post-checkout")

for hook in "${HOOKS[@]}"; do
    HOOK_FILE="$HOOKS_DIR/$hook"
    
    if [ -f "$HOOK_FILE" ]; then
        if grep -q "memex index" "$HOOK_FILE"; then
            echo "Memex hook already present in $hook."
            continue
        fi
        echo "Appending Memex trigger to existing $hook hook..."
        printf "\n# Memex background index trigger\nif command -v memex >/dev/null 2>&1; then\n    (memex index --quiet >/dev/null 2>&1 &)\nfi\n" >> "$HOOK_FILE"
    else
        echo "Creating $hook hook..."
        printf "%s" "$HOOK_CONTENT" > "$HOOK_FILE"
    fi
    chmod +x "$HOOK_FILE"
done

echo "✓ Memex Git hooks installed successfully (post-commit, post-merge, post-checkout)."
