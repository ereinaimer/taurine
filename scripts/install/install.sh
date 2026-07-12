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

# Check if already installed
if [ -x "$INSTALL_DIR/taurine" ]; then
    echo "Taurine is already installed at $INSTALL_DIR/taurine. Skipping installation."
    exit 0
fi

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

# Add to PATH
ADDED_PATH=false
if [ "$OS" = "Darwin" ]; then
    for profile in "$HOME/.zprofile" "$HOME/.bash_profile"; do
        touch "$profile"
        if ! grep -Fxq "export PATH=\"$INSTALL_DIR:\$PATH\"" "$profile"; then
            printf "\nexport PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR" >> "$profile"
            ADDED_PATH=true
        fi
    done
else
    for profile in "$HOME/.bashrc" "$HOME/.zshrc"; do
        if [ -f "$profile" ]; then
            if ! grep -Fxq "export PATH=\"$INSTALL_DIR:\$PATH\"" "$profile"; then
                printf "\nexport PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR" >> "$profile"
                ADDED_PATH=true
            fi
        fi
    done
fi

# Set up tau alias
printf "\nSetting up 'tau' alias...\n"
ALIAS_LINE="alias tau='taurine'"
for profile in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zprofile"; do
    if [ -f "$profile" ] && ! grep -Fq "alias tau=" "$profile" 2>/dev/null; then
        printf "\n%s\n" "$ALIAS_LINE" >> "$profile"
    fi
done

printf "\x1b[32m✓\x1b[0m taurine v%s installed\n" "$VERSION"
if [ "$ADDED_PATH" = true ]; then
    printf "Added to PATH. Please restart your shell or run:\n"
    if [ "$OS" = "Darwin" ]; then
        printf "  source ~/.zprofile\n"
    else
        printf "  source ~/.bashrc\n"
    fi
fi
printf "Added alias tau to your shell profile.\n"
printf "Now you can run 'tau --help' for more details.\n"