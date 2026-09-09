#!/bin/sh
set -eu
umask 022

if [ -z "${HOME:-}" ]; then
    echo "Error: HOME is not set. Please set HOME and try again." >&2
    exit 1
fi

# Detect OS and setup paths
OS="$(uname -s)"
if [ "$OS" = "Linux" ]; then
    INSTALL_DIR="$HOME/.local/share/taurine/bin"
    DATA_DIR="$HOME/.local/share/taurine"
elif [ "$OS" = "Darwin" ]; then
    INSTALL_DIR="$HOME/Library/Application Support/Taurine/bin"
    DATA_DIR="$HOME/Library/Application Support/Taurine"
else
    echo "Error: Unsupported OS $OS"
    exit 1
fi

# Stop service if the installed binary is available.
if [ -x "$INSTALL_DIR/taurine" ]; then
    "$INSTALL_DIR/taurine" down >/dev/null 2>&1 || true
fi

# Stop and clean up systemd service on Linux
if [ "$OS" = "Linux" ] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop ereinaimer-taurine.service >/dev/null 2>&1 || true
    systemctl --user disable ereinaimer-taurine.service >/dev/null 2>&1 || true
    rm -f "$HOME/.config/systemd/user/ereinaimer-taurine.service" || true
    rm -f "$HOME/.config/systemd/user/default.target.wants/ereinaimer-taurine.service" || true
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi

# Stop and clean up launchd service on macOS
if [ "$OS" = "Darwin" ] && command -v launchctl >/dev/null 2>&1; then
    LAUNCH_AGENT="$HOME/Library/LaunchAgents/com.ereinaimer.taurine.plist"
    if [ -f "$LAUNCH_AGENT" ]; then
        USER_ID=$(id -u)
        launchctl bootout "gui/$USER_ID" "$LAUNCH_AGENT" >/dev/null 2>&1 || \
            launchctl unload "$LAUNCH_AGENT" >/dev/null 2>&1 || true
        rm -f "$LAUNCH_AGENT" || true
    fi
fi

# Run completions uninstaller if available
if [ -x "$INSTALL_DIR/taurine" ]; then
    "$INSTALL_DIR/taurine" completions uninstall >/dev/null 2>&1 || true
fi

# Clean shell profiles
clean_profile() {
    profile="$1"
    if [ -f "$profile" ]; then
        temp_file=$(mktemp)
        grep -v -F "export PATH=\"$INSTALL_DIR:\$PATH\"" "$profile" | \
        grep -v -F "alias tau='taurine'" | \
        grep -v -F "fish_add_path \"$INSTALL_DIR\"" | \
        grep -v -F "set path = ( \$path \"$INSTALL_DIR\" )" | \
        grep -v -F "alias tau taurine" > "$temp_file" || true
        cat "$temp_file" > "$profile"
        rm -f "$temp_file"
    fi
}

for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.zprofile" "$HOME/.profile"; do
    clean_profile "$profile"
done
clean_profile "$HOME/.config/fish/config.fish"
for profile in "$HOME/.tcshrc" "$HOME/.cshrc"; do
    clean_profile "$profile"
done

# Remove all configured API keys and RPC token from OS keyring
if [ -x "$INSTALL_DIR/taurine" ]; then
    "$INSTALL_DIR/taurine" ai remove --all --yes --json >/dev/null 2>&1 || true
fi
if command -v secret-tool >/dev/null 2>&1; then
    secret-tool clear service taurine >/dev/null 2>&1 || true
fi
if [ "$OS" = "Darwin" ]; then
    security delete-generic-password -s taurine >/dev/null 2>&1 || true
fi

# Delete all data (config, database, logs, binary)
if [ -n "${DATA_DIR:-}" ] && [ "$DATA_DIR" != "/" ]; then
    rm -rf "$DATA_DIR"
fi

echo "Taurine uninstalled successfully."
