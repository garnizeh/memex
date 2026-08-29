#!/usr/bin/env bash
# Install Git hooks for automatic background indexing with Memex.
# Hooks: post-commit, post-merge, post-checkout

set -euo pipefail

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1 && ! git rev-parse --git-dir >/dev/null 2>&1; then
    echo "Error: Not inside a Git repository." >&2
    exit 1
fi

HOOKS_DIR="$(git rev-parse --path-format=absolute --git-path hooks 2>/dev/null || true)"
if [ -z "$HOOKS_DIR" ]; then
    GIT_DIR="$(git rev-parse --git-dir)"
    HOOKS_DIR="$GIT_DIR/hooks"
fi

mkdir -p "$HOOKS_DIR"

HOOKS=("post-commit" "post-merge" "post-checkout")

for hook in "${HOOKS[@]}"; do
    HOOK_FILE="$HOOKS_DIR/$hook"
    LEGACY_HOOK_FILE="$HOOKS_DIR/$hook.pre-memex"

    if [ -f "$HOOK_FILE" ]; then
        if grep -q "memex index" "$HOOK_FILE"; then
            echo "Memex hook already present in $hook."
            continue
        fi

        echo "Preserving existing $hook hook as $hook.pre-memex and wrapping..."
        mv "$HOOK_FILE" "$LEGACY_HOOK_FILE"
        chmod +x "$LEGACY_HOOK_FILE"

        cat << EOF > "$HOOK_FILE"
#!/usr/bin/env bash
# Managed by Memex installer
DIR="\$(cd "\$(dirname "\${BASH_SOURCE[0]}")" && pwd)"
LEGACY="\$DIR/${hook}.pre-memex"

# Run previous hook if it exists and preserve its exit status
EXIT_CODE=0
if [ -x "\$LEGACY" ]; then
    "\$LEGACY" "\$@" || EXIT_CODE=\$?
fi

# Memex background index trigger
if command -v memex >/dev/null 2>&1; then
    (memex index --quiet >/dev/null 2>&1 &)
fi

exit \$EXIT_CODE
EOF
    else
        echo "Creating $hook hook..."
        cat << 'EOF' > "$HOOK_FILE"
#!/usr/bin/env bash
# Memex background index trigger
if command -v memex >/dev/null 2>&1; then
    (memex index --quiet >/dev/null 2>&1 &)
fi
EOF
    fi
    chmod +x "$HOOK_FILE"
done

echo "✓ Memex Git hooks installed successfully (post-commit, post-merge, post-checkout)."
