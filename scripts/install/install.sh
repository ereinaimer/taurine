#!/usr/bin/env bash
set -euo pipefail

# Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" = "Linux" ]; then
    if [ "$ARCH" = "x86_64" ]; then
        PLATFORM="linux-x86_64"
    else
        echo "Error: Unsupported architecture $ARCH on Linux"
        exit 1
    fi
    INSTALL_DIR="$HOME/.local/share/taurine/bin"
elif [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "x86_64" ]; then
        PLATFORM="macos-x86_64"
    elif [ "$ARCH" = "arm64" ] || [ "$ARCH" = "aarch64" ]; then
        PLATFORM="macos-aarch64"
    else
        echo "Error: Unsupported architecture $ARCH on macOS"
        exit 1
    fi
    INSTALL_DIR="$HOME/Library/Application Support/Taurine/bin"
else
    echo "Error: Unsupported OS $OS"
    exit 1
fi

# Verify required commands
for cmd in curl tar; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: $cmd is required but was not found. Please install it and try again." >&2
        exit 1
    fi
done

# Cleanup handler — always remove temp dir on exit
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

run_with_spinner() {
    local label=$1
    local cmd=$2
    local err_file="${TMP_DIR}/spinner_err.$$"

    eval "$cmd" >/dev/null 2>"$err_file" &
    local pid=$!
    local delay=0.08
    local spinstr='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'

    # Disable set -e temporarily to safely manage spinner loop and wait
    set +e
    while kill -0 $pid 2>/dev/null; do
        local temp=${spinstr#?}
        printf "\r%c %s" "$spinstr" "$label"
        spinstr=$temp${spinstr%"$temp"}
        sleep $delay
    done

    wait $pid
    local exit_code=$?
    set -e

    if [ $exit_code -eq 0 ]; then
        printf "\r\x1b[32m✓\x1b[0m %s\n" "$label"
    else
        printf "\r\x1b[31m✗\x1b[0m %s\n" "$label"
        if [ -s "$err_file" ]; then
            sed 's/^/  /' "$err_file" >&2
        fi
    fi

    rm -f "$err_file"
    return $exit_code
}

verify_checksum() {
    local file=$1
    local expected=$2
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$expected  $file" | sha256sum -c - > /dev/null 2>&1
    elif command -v shasum >/dev/null 2>&1; then
        echo "$expected  $file" | shasum -a 256 -c - > /dev/null 2>&1
    else
        echo "Error: No sha256 checksum tool available (tried sha256sum, shasum)"
        exit 1
    fi
}

# Invoke a command with retry and spinner
invoke_with_retry() {
    local label=$1
    local cmd=$2
    local max_attempts=3
    local attempt=1
    local delay=2

    while [ $attempt -le $max_attempts ]; do
        local current_label="$label"
        if [ $attempt -gt 1 ]; then
            current_label="$label (attempt $attempt/$max_attempts)"
        fi

        if run_with_spinner "$current_label" "$cmd"; then
            return 0
        fi

        if [ $attempt -lt $max_attempts ]; then
            echo "  Retrying in ${delay}s..." >&2
            sleep $delay
            delay=$((delay * 2))
        fi
        attempt=$((attempt + 1))
    done

    echo "Error: Failed after $max_attempts attempts: $label" >&2
    return 1
}

version_gt() {
    # Returns 0 if $1 > $2, 1 otherwise
    local v1="${1%%-*}"
    local v2="${2%%-*}"

    local IFS=.
    set -f
    local parts1=($v1) parts2=($v2)
    set +f

    for i in 0 1 2 3; do
        local a="${parts1[$i]:-0}"
        local b="${parts2[$i]:-0}"
        a="${a%%[!0-9]*}"
        b="${b%%[!0-9]*}"
        a="${a:-0}"
        b="${b:-0}"
        if [ "$a" -gt "$b" ] 2>/dev/null; then return 0; fi
        if [ "$a" -lt "$b" ] 2>/dev/null; then return 1; fi
    done
    return 1
}

trim() {
    echo "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

configure_profile() {
    local profile="$1"
    local shell_type="$2"
    local modified=false

    # Ensure profile directory exists
    mkdir -p "$(dirname "$profile")"

    # 1. Handle PATH (idempotent)
    if [ "$shell_type" = "fish" ]; then
        local path_line="fish_add_path \"$INSTALL_DIR\""
        if [ -f "$profile" ]; then
            if ! grep -Fq "$path_line" "$profile" 2>/dev/null; then
                printf "\n%s\n" "$path_line" >> "$profile"
                modified=true
            fi
        else
            printf "\n%s\n" "$path_line" >> "$profile"
            modified=true
        fi
    elif [ "$shell_type" = "csh" ]; then
        local path_line="set path = ( \$path \"$INSTALL_DIR\" )"
        if [ -f "$profile" ]; then
            if ! grep -Fq "$path_line" "$profile" 2>/dev/null; then
                printf "\n%s\n" "$path_line" >> "$profile"
                modified=true
            fi
        else
            printf "\n%s\n" "$path_line" >> "$profile"
            modified=true
        fi
    else
        local path_line="export PATH=\"$INSTALL_DIR:\$PATH\""
        if [ -f "$profile" ]; then
            if ! grep -Fxq "$path_line" "$profile" 2>/dev/null; then
                printf "\n%s\n" "$path_line" >> "$profile"
                modified=true
            fi
        else
            printf "\n%s\n" "$path_line" >> "$profile"
            modified=true
        fi
    fi

    # 2. Handle Alias (Ensure only ONE entry of the alias exists, and it's correct)
    local alias_line="alias tau='taurine'"
    local alias_prefix="alias tau="
    if [ "$shell_type" = "csh" ]; then
        alias_line="alias tau taurine"
        alias_prefix="alias tau "
    fi

    if [ -f "$profile" ]; then
        local matches
        matches=$(grep -F "$alias_prefix" "$profile" || true)
        local count
        count=$(echo "$matches" | grep -c . || true)
        local trimmed_match
        trimmed_match=$(trim "$matches")

        if [ "$count" -eq 1 ] && [ "$trimmed_match" = "$alias_line" ]; then
            : # Already correct
        else
            local tmp_profile
            tmp_profile=$(mktemp)
            grep -Fv "$alias_prefix" "$profile" > "$tmp_profile" || true
            cat "$tmp_profile" > "$profile"
            rm -f "$tmp_profile"
            printf "\n%s\n" "$alias_line" >> "$profile"
            modified=true
        fi
    else
        printf "\n%s\n" "$alias_line" >> "$profile"
        modified=true
    fi

    if [ "$modified" = true ]; then
        return 0 # Modified
    else
        return 1 # Not modified
    fi
}

# Fetch latest release manifest
invoke_with_retry "Fetching latest release manifest" "curl -fsSL https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json -o \"$TMP_DIR/manifest.json\"" || exit 1

MANIFEST=$(tr -d '\n\r\t ' < "$TMP_DIR/manifest.json")
VERSION=$(echo "$MANIFEST" | grep -o '"version":"[^"]*"' | head -n 1 | cut -d'"' -f4 || true)
URL=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"url":"[^"]*"' | cut -d'"' -f4 || true)
SHA256=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"sha256":"[^"]*"' | cut -d'"' -f4 || true)

if [ -z "$VERSION" ] || [ -z "$URL" ]; then
    echo "Error: Could not determine latest version or download URL."
    exit 1
fi

IS_INSTALLED=false
IS_FRESH_INSTALL=false
if [ -x "$INSTALL_DIR/taurine" ]; then
    LOCAL_VERSION=$("$INSTALL_DIR/taurine" --version 2>/dev/null | awk '{print $2}') || true
    if [ -n "$LOCAL_VERSION" ]; then
        if [ "$LOCAL_VERSION" = "$VERSION" ]; then
            printf "\x1b[32m✓\x1b[0m Taurine is already installed, up to date (v%s), and configured properly.\n" "$LOCAL_VERSION"
        elif version_gt "$LOCAL_VERSION" "$VERSION"; then
            printf "\x1b[32m✓\x1b[0m Taurine is already installed, up to date (v%s), and configured properly.\n" "$LOCAL_VERSION"
        else
            printf "Taurine is already installed but a newer version (v%s) is available. Please run 'tau update' to update.\n" "$VERSION"
        fi
    else
        printf "Taurine is already installed. If you want to update to the latest version (v%s), please run 'tau update'.\n" "$VERSION"
    fi
    IS_INSTALLED=true
fi

if [ "$IS_INSTALLED" = false ]; then
    ARCHIVE="$TMP_DIR/taurine.tar.xz"

    # Download archive with retry
    invoke_with_retry "Downloading taurine v$VERSION" "curl -fsSL \"$URL\" -o \"$ARCHIVE\"" || exit 1

    # Verify checksum if available
    if [ -n "$SHA256" ]; then
        verify_checksum "$ARCHIVE" "$SHA256" || {
            echo "Error: Checksum verification failed for downloaded archive."
            exit 1
        }
    fi

    # Extract
    mkdir -p "$INSTALL_DIR"
    run_with_spinner "Extracting" "tar -xf \"$ARCHIVE\" -C \"$TMP_DIR\"" || {
        echo "Error: Extraction failed." >&2
        exit 1
    }

    # Copy binary
    cp "$TMP_DIR/taurine" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/taurine"
    IS_INSTALLED=true
    IS_FRESH_INSTALL=true

    printf "\x1b[32m✓\x1b[0m taurine v%s installed\n" "$VERSION"
fi

# Configure shell profiles if installation succeeded (either fresh or pre-existing)
if [ "$IS_INSTALLED" = true ]; then
    # Determine active shell to ensure we have at least one profile file
    ACTIVE_SHELL=$(basename "${SHELL:-bash}")
    case "$ACTIVE_SHELL" in
        zsh)   touch "$HOME/.zshrc" ;;
        fish)  mkdir -p "$HOME/.config/fish" && touch "$HOME/.config/fish/config.fish" ;;
        csh|tcsh) touch "$HOME/.cshrc" ;;
        bash)  touch "$HOME/.bashrc" ;;
        *)     touch "$HOME/.profile" ;;
    esac

    ADDED_PATH=false
    MODIFIED_PROFILES=()

    # POSIX profiles
    for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.zprofile" "$HOME/.profile"; do
        if [ -f "$profile" ]; then
            if configure_profile "$profile" "posix"; then
                ADDED_PATH=true
                MODIFIED_PROFILES+=("$profile")
            fi
        fi
    done

    # Fish profile
    FISH_PROFILE="$HOME/.config/fish/config.fish"
    if [ -f "$FISH_PROFILE" ]; then
        if configure_profile "$FISH_PROFILE" "fish"; then
            ADDED_PATH=true
            MODIFIED_PROFILES+=("$FISH_PROFILE")
        fi
    fi

    # Csh/Tcsh profiles
    for profile in "$HOME/.tcshrc" "$HOME/.cshrc"; do
        if [ -f "$profile" ]; then
            if configure_profile "$profile" "csh"; then
                ADDED_PATH=true
                MODIFIED_PROFILES+=("$profile")
            fi
        fi
    done

    if [ "$IS_FRESH_INSTALL" = true ]; then
        if [ "$ADDED_PATH" = true ]; then
            printf "Added to PATH and set up alias tau in your shell profiles:\n"
            for p in "${MODIFIED_PROFILES[@]}"; do
                printf "  %s\n" "$p"
            done
            printf "Please restart your shell or source your profile to apply the changes.\n"
        else
            ON_PATH=false
            if command -v taurine >/dev/null 2>&1; then
                ON_PATH=true
            fi
            case ":$PATH:" in
                *:"$INSTALL_DIR":*) ON_PATH=true ;;
            esac

            if [ "$ON_PATH" = true ]; then
                printf "\x1b[32m✓\x1b[0m Taurine binary is already on your PATH.\n"
            else
                printf "Taurine binary is configured in your profile, but not in your current shell session.\n"
                printf "Please restart your shell or source your profile.\n"
            fi
            printf "\x1b[32m✓\x1b[0m alias 'tau' is already set up.\n"
        fi
    fi
fi