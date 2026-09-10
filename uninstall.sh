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

# Cleanup handler — always remove temp dir on exit
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

run_with_spinner() {
    label=$1
    cmd=$2
    success_label=${3:-$label}
    err_file="${TMP_DIR}/spinner_err.$$"

    eval "$cmd" >/dev/null 2>"$err_file" &
    pid=$!
    delay=0.08
    spinstr='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'

    # Disable set -e temporarily to safely manage spinner loop and wait
    set +e
    while kill -0 $pid 2>/dev/null; do
        temp=${spinstr#?}
        printf "\r%c %s" "$spinstr" "$label"
        spinstr=$temp${spinstr%"$temp"}
        sleep $delay
    done

    wait $pid
    exit_code=$?
    set -e

    if [ $exit_code -eq 0 ]; then
        printf "\r\x1b[32m✓\x1b[0m %s\x1b[K\n" "$success_label"
    else
        printf "\r\x1b[31m✗\x1b[0m %s\x1b[K\n" "$label"
        if [ -s "$err_file" ]; then
            sed 's/^/  /' "$err_file" >&2
        fi
    fi

    rm -f "$err_file"
    return $exit_code
}

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

stop_service() {
    if [ -x "$INSTALL_DIR/taurine" ]; then
        "$INSTALL_DIR/taurine" down >/dev/null 2>&1 || true
    fi
}

remove_background_service() {
    if [ "$OS" = "Linux" ] && command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop ereinaimer-taurine.service >/dev/null 2>&1 || true
        systemctl --user disable ereinaimer-taurine.service >/dev/null 2>&1 || true
        rm -f "$HOME/.config/systemd/user/ereinaimer-taurine.service" || true
        rm -f "$HOME/.config/systemd/user/default.target.wants/ereinaimer-taurine.service" || true
        systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    if [ "$OS" = "Darwin" ] && command -v launchctl >/dev/null 2>&1; then
        LAUNCH_AGENT="$HOME/Library/LaunchAgents/com.ereinaimer.taurine.plist"
        if [ -f "$LAUNCH_AGENT" ]; then
            USER_ID=$(id -u)
            launchctl bootout "gui/$USER_ID" "$LAUNCH_AGENT" >/dev/null 2>&1 || \
                launchctl unload "$LAUNCH_AGENT" >/dev/null 2>&1 || true
            rm -f "$LAUNCH_AGENT" || true
        fi
    fi
}

remove_completions() {
    if [ -x "$INSTALL_DIR/taurine" ]; then
        "$INSTALL_DIR/taurine" completions uninstall >/dev/null 2>&1 || true
    fi
}

clean_all_profiles() {
    for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.zprofile" "$HOME/.profile"; do
        clean_profile "$profile"
    done
    clean_profile "$HOME/.config/fish/config.fish"
    for profile in "$HOME/.tcshrc" "$HOME/.cshrc"; do
        clean_profile "$profile"
    done
}

remove_credentials() {
    if [ -x "$INSTALL_DIR/taurine" ]; then
        "$INSTALL_DIR/taurine" ai remove --all --yes --json >/dev/null 2>&1 || true
    fi
    if command -v secret-tool >/dev/null 2>&1; then
        secret-tool clear service taurine >/dev/null 2>&1 || true
    fi
    if [ "$OS" = "Darwin" ]; then
        security delete-generic-password -s taurine >/dev/null 2>&1 || true
    fi
}

remove_files() {
    if [ -n "${DATA_DIR:-}" ] && [ "$DATA_DIR" != "/" ]; then
        rm -rf "$DATA_DIR"
    fi
}

run_with_spinner "Stopping Taurine" "stop_service" || true
run_with_spinner "Removing background service" "remove_background_service" || true
run_with_spinner "Removing shell completions" "remove_completions" || true
run_with_spinner "Cleaning shell profiles" "clean_all_profiles" || true
run_with_spinner "Removing credentials" "remove_credentials" || true
run_with_spinner "Removing data files" "remove_files" || true

printf "\x1b[32m✓\x1b[0m Taurine has been uninstalled successfully.\n"
