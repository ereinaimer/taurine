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

# Cleanup handler — always remove temp dir on exit
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

show_spinner() {
    local pid=$1
    local label=$2
    local delay=0.08
    local spinstr='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
    while ps -p $pid > /dev/null 2>&1; do
        local temp=${spinstr#?}
        printf "\r%c %s" "$spinstr" "$label"
        local spinstr=$temp${spinstr%"$temp"}
        sleep $delay
    done
    printf "\r\x1b[32m✓\x1b[0m %s\n" "$label"
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

version_gt() {
    # Returns 0 if $1 > $2, 1 otherwise
    # Strips pre-release suffixes for comparison
    local v1="${1%%-*}"
    local v2="${2%%-*}"

    # Split into components and zero-pad for reliable numeric comparison
    local IFS=.
    set -f
    # shellcheck disable=SC2206
    local parts1=($v1) parts2=($v2)
    set +f

    for i in 0 1 2 3; do
        local a="${parts1[$i]:-0}"
        local b="${parts2[$i]:-0}"
        # Strip any non-numeric suffix
        a="${a%%[!0-9]*}"
        b="${b%%[!0-9]*}"
        a="${a:-0}"
        b="${b:-0}"
        if [ "$a" -gt "$b" ] 2>/dev/null; then return 0; fi
        if [ "$a" -lt "$b" ] 2>/dev/null; then return 1; fi
    done
    return 1
}

# Retry a command up to N times with exponential backoff
retry() {
    local max_attempts=$1
    local cmd="$2"
    local attempt=1
    local delay=2

    while [ $attempt -le $max_attempts ]; do
        if eval "$cmd" 2>/dev/null; then
            return 0
        fi
        if [ $attempt -lt $max_attempts ]; then
            sleep $delay
            delay=$((delay * 2))
        fi
        attempt=$((attempt + 1))
    done
    return 1
}

# Fetch latest release manifest
retry 3 "curl -fsSL https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json -o \"$TMP_DIR/manifest.json\"" &
PID=$!
show_spinner $PID "Fetching latest release manifest"
wait $PID || { echo "Error: Could not fetch release manifest after 3 retries."; exit 1; }

MANIFEST=$(cat "$TMP_DIR/manifest.json")
VERSION=$(echo "$MANIFEST" | grep -o '"version":"[^"]*"' | head -n 1 | cut -d'"' -f4 || true)
URL=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"url":"[^"]*"' | cut -d'"' -f4 || true)
SHA256=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"sha256":"[^"]*"' | cut -d'"' -f4 || true)

if [ -z "$VERSION" ] || [ -z "$URL" ]; then
    echo "Error: Could not determine latest version or download URL."
    exit 1
fi

# Check if already installed — gracefully handle old binaries without --version
LOCAL_VERSION=""
if [ -x "$INSTALL_DIR/taurine" ]; then
    LOCAL_VERSION=$("$INSTALL_DIR/taurine" --version 2>/dev/null | awk '{print $2}') || true
fi

if [ -n "$LOCAL_VERSION" ]; then
    if [ "$LOCAL_VERSION" = "$VERSION" ]; then
        echo "Taurine is already installed and up to date (v$LOCAL_VERSION)."
        exit 0
    fi
    # Prevent downgrade: if local version is newer than manifest, skip
    if version_gt "$LOCAL_VERSION" "$VERSION"; then
        echo "Taurine v$LOCAL_VERSION is newer than the latest release (v$VERSION). Skipping update."
        exit 0
    fi
fi

ARCHIVE="$TMP_DIR/taurine.tar.xz"

# Download archive with retry
retry 3 "curl -fsSL \"$URL\" -o \"$ARCHIVE\"" &
PID=$!
show_spinner $PID "Downloading taurine v$VERSION"
wait $PID || { echo "Error: Could not download taurine v$VERSION after 3 retries."; exit 1; }

# Verify checksum if available
if [ -n "$SHA256" ]; then
    verify_checksum "$ARCHIVE" "$SHA256" || {
        echo "Error: Checksum verification failed for downloaded archive."
        exit 1
    }
fi

# Extract
mkdir -p "$INSTALL_DIR"
tar -xf "$ARCHIVE" -C "$TMP_DIR" &
PID=$!
show_spinner $PID "Extracting"
wait $PID

cp "$TMP_DIR/taurine" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/taurine"

# Add to PATH
ADDED_PATH=false
if [ "$OS" = "Darwin" ]; then
    for profile in "$HOME/.zprofile" "$HOME/.bash_profile"; do
        touch "$profile"
        if ! grep -Fq "$INSTALL_DIR" "$profile"; then
            echo -e "\nexport PATH=\"$INSTALL_DIR:\$PATH\"" >> "$profile"
            ADDED_PATH=true
        fi
    done
else
    for profile in "$HOME/.bashrc" "$HOME/.zshrc"; do
        if [ -f "$profile" ]; then
            if ! grep -Fq "$INSTALL_DIR" "$profile"; then
                echo -e "\nexport PATH=\"$INSTALL_DIR:\$PATH\"" >> "$profile"
                ADDED_PATH=true
            fi
        fi
    done
fi

echo -e "\n\x1b[32m✓\x1b[0m taurine v$VERSION installed"
if [ "$ADDED_PATH" = true ]; then
    echo "Added to PATH. Please restart your shell or run:"
    if [ "$OS" = "Darwin" ]; then
        echo "  source ~/.zprofile"
    else
        echo "  source ~/.bashrc"
    fi
fi