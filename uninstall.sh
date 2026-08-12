#!/bin/sh
set -eu

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

# Check if Taurine is installed
DATA_DIR_EMPTY=0
if [ ! -d "$DATA_DIR" ] || [ -z "$(ls -A "$DATA_DIR" 2>/dev/null)" ]; then
    DATA_DIR_EMPTY=1
fi

BIN_IN_DATA_DIR=0
if [ -f "$INSTALL_DIR/taurine" ]; then
    BIN_IN_DATA_DIR=1
fi

BIN_IN_PATH=0
if command -v taurine >/dev/null 2>&1; then
    BIN_IN_PATH=1
fi

if [ "$DATA_DIR_EMPTY" -eq 1 ] && [ "$BIN_IN_DATA_DIR" -eq 0 ] && [ "$BIN_IN_PATH" -eq 0 ]; then
    printf "\033[32m✓\033[0m Taurine is not installed on this system.\n"
    exit 0
fi

# Stop service if running via the installed binary
if [ -x "$INSTALL_DIR/taurine" ]; then
    "$INSTALL_DIR/taurine" down >/dev/null 2>&1 || true
fi

# Stop and clean up systemd service on Linux
if [ "$OS" = "Linux" ]; then
    systemctl --user stop ereinaimer-taurine.service >/dev/null 2>&1 || true
    systemctl --user disable ereinaimer-taurine.service >/dev/null 2>&1 || true
    rm -f "$HOME/.config/systemd/user/ereinaimer-taurine.service" || true
    rm -f "$HOME/.config/systemd/user/default.target.wants/ereinaimer-taurine.service" || true
    systemctl --user daemon-reload >/dev/null 2>&1 || true
fi

# Stop and clean up launchd service on macOS
if [ "$OS" = "Darwin" ]; then
    if [ -f "$HOME/Library/LaunchAgents/com.ereinaimer.taurine.plist" ]; then
        launchctl unload "$HOME/Library/LaunchAgents/com.ereinaimer.taurine.plist" >/dev/null 2>&1 || true
        rm -f "$HOME/Library/LaunchAgents/com.ereinaimer.taurine.plist" || true
    fi
fi

# Run completions uninstaller if available
if [ -x "$INSTALL_DIR/taurine" ]; then
    "$INSTALL_DIR/taurine" completions uninstall >/dev/null 2>&1 || true
fi

# Clean shell profiles
clean_profile() {
    local profile="$1"
    if [ -f "$profile" ]; then
        local temp_file
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
    secret-tool clear service taurine account rpc_token >/dev/null 2>&1 || true
fi
if [ "$OS" = "Darwin" ]; then
    security delete-generic-password -s taurine -a rpc_token >/dev/null 2>&1 || true
fi

# Delete all data (config, database, logs, binary)
rm -rf "$DATA_DIR"

echo "Taurine uninstalled successfully."
