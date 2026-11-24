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
    INSTALL_DIR="$HOME/.local/share/nite/bin"
    DATA_DIR="$HOME/.local/share/nite"
elif [ "$OS" = "Darwin" ]; then
    INSTALL_DIR="$HOME/Library/Application Support/Nite/bin"
    DATA_DIR="$HOME/Library/Application Support/Nite"
else
    echo "Error: Unsupported OS $OS"
    exit 1
fi

if [ ! -x "$INSTALL_DIR/nite" ] && ! command -v nite >/dev/null 2>&1 && [ ! -e "$DATA_DIR" ]; then
    printf "\x1b[32m✓\x1b[0m Nite is not installed on this system.\n"
    exit 0
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
        grep -v -E "alias[[:space:]]+tau(=|[[:space:]])" | \
        grep -v -F "fish_add_path \"$INSTALL_DIR\"" | \
        grep -v -F "set path = ( \$path \"$INSTALL_DIR\" )" > "$temp_file" || true
        cat "$temp_file" > "$profile"
        rm -f "$temp_file"
    fi
}

stop_service() {
    if [ -x "$INSTALL_DIR/nite" ]; then
        "$INSTALL_DIR/nite" down >/dev/null 2>&1 || true
    fi
    if command -v pkill >/dev/null 2>&1; then
        pkill -x nite >/dev/null 2>&1 || true
        pkill -x nite-startup >/dev/null 2>&1 || true
    fi
}

remove_background_service() {
    if [ "$OS" = "Linux" ] && command -v systemctl >/dev/null 2>&1; then
        systemctl --user stop ereinaimer-nite.service >/dev/null 2>&1 || true
        systemctl --user disable ereinaimer-nite.service >/dev/null 2>&1 || true
        rm -f "$HOME/.config/systemd/user/ereinaimer-nite.service" || true
        rm -f "$HOME/.config/systemd/user/default.target.wants/ereinaimer-nite.service" || true
        systemctl --user daemon-reload >/dev/null 2>&1 || true
    fi
    if [ "$OS" = "Darwin" ] && command -v launchctl >/dev/null 2>&1; then
        LAUNCH_AGENT="$HOME/Library/LaunchAgents/com.ereinaimer.nite.plist"
        if [ -f "$LAUNCH_AGENT" ]; then
            USER_ID=$(id -u)
            launchctl bootout "gui/$USER_ID" "$LAUNCH_AGENT" >/dev/null 2>&1 || \
                launchctl unload "$LAUNCH_AGENT" >/dev/null 2>&1 || true
            rm -f "$LAUNCH_AGENT" || true
        fi
    fi
}

remove_completions() {
    if [ -x "$INSTALL_DIR/nite" ]; then
        "$INSTALL_DIR/nite" completions uninstall >/dev/null 2>&1 || true
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
    if [ -x "$INSTALL_DIR/nite" ]; then
        "$INSTALL_DIR/nite" ai remove --all --yes --json >/dev/null 2>&1 || true
    fi
    if command -v secret-tool >/dev/null 2>&1; then
        secret-tool clear service nite >/dev/null 2>&1 || true
        secret-tool clear service nite username db-key >/dev/null 2>&1 || true
        secret-tool clear service nite-cloud >/dev/null 2>&1 || true
        secret-tool clear service nite.inline_ai >/dev/null 2>&1 || true
    fi
    if [ "$OS" = "Darwin" ]; then
        security delete-generic-password -s nite >/dev/null 2>&1 || true
        security delete-generic-password -s nite -a db-key >/dev/null 2>&1 || true
        security delete-generic-password -s nite-cloud >/dev/null 2>&1 || true
        security delete-generic-password -s nite.inline_ai >/dev/null 2>&1 || true
    fi
}

remove_files() {
    if [ -n "${DATA_DIR:-}" ] && [ "$DATA_DIR" != "/" ]; then
        rm -rf "$DATA_DIR"
    fi
}

remove_binary() {
    if [ -n "${INSTALL_DIR:-}" ] && [ "$INSTALL_DIR" != "/" ]; then
        rm -f "$INSTALL_DIR/nite" "$INSTALL_DIR/uninstall.sh" || true
        rmdir "$INSTALL_DIR" >/dev/null 2>&1 || true
    fi
}

PURGE_DATA=false
if [ -t 0 ]; then
    printf "Remove configuration and data files? [y/N] "
    read -r answer || answer=""
    case "$answer" in
        [Yy]*) PURGE_DATA=true ;;
    esac
fi

run_with_spinner "Stopping Nite" "stop_service" || true
run_with_spinner "Removing background service" "remove_background_service" || true
run_with_spinner "Removing shell completions" "remove_completions" || true
run_with_spinner "Cleaning shell profiles" "clean_all_profiles" || true
if [ "$PURGE_DATA" = true ]; then
    run_with_spinner "Removing credentials" "remove_credentials" || true
fi
run_with_spinner "Removing binary" "remove_binary" || true
if [ "$PURGE_DATA" = true ]; then
    run_with_spinner "Removing data files" "remove_files" || true
else
    printf "\x1b[32m✓\x1b[0m Kept configuration and data files.\n"
fi

printf "\x1b[32m✓\x1b[0m Nite has been uninstalled successfully.\n"
