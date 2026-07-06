#!/usr/bin/env bash
set -e

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

TMP_DIR=$(mktemp -d)

# Fetch latest release manifest
curl -fsSL https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json -o "$TMP_DIR/manifest.json" &
PID=$!
show_spinner $PID "Fetching latest release manifest"
wait $PID

MANIFEST=$(cat "$TMP_DIR/manifest.json")
VERSION=$(echo "$MANIFEST" | grep -o '"version":"[^"]*"' | head -n 1 | cut -d'"' -f4)
URL=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"url":"[^"]*"' | cut -d'"' -f4)

if [ -z "$VERSION" ] || [ -z "$URL" ]; then
    echo "Error: Could not determine latest version or download URL."
    rm -rf "$TMP_DIR"
    exit 1
fi

# Check if already installed
if [ -x "$INSTALL_DIR/taurine" ]; then
    LOCAL_VERSION=$("$INSTALL_DIR/taurine" --version 2>/dev/null | awk '{print $2}')
    if [ "$LOCAL_VERSION" = "$VERSION" ]; then
        echo "Taurine is already installed and up to date (v$LOCAL_VERSION)."
        rm -rf "$TMP_DIR"
        exit 0
    fi
fi

ARCHIVE="$TMP_DIR/taurine.tar.xz"


# Download archive
curl -fsSL "$URL" -o "$ARCHIVE" &
PID=$!
show_spinner $PID "Downloading taurine v$VERSION"
wait $PID

# Extract
mkdir -p "$INSTALL_DIR"
tar -xf "$ARCHIVE" -C "$TMP_DIR" &
PID=$!
show_spinner $PID "Extracting"
wait $PID

cp "$TMP_DIR/taurine" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/taurine"
rm -rf "$TMP_DIR"

# Add to PATH
ADDED_PATH=false
if [ "$OS" = "Darwin" ]; then
    for profile in "$HOME/.zprofile" "$HOME/.bash_profile"; do
        touch "$profile"
        if ! grep -q "$INSTALL_DIR" "$profile"; then
            echo -e "\nexport PATH=\"$INSTALL_DIR:\$PATH\"" >> "$profile"
            ADDED_PATH=true
        fi
    done
else
    for profile in "$HOME/.bashrc" "$HOME/.zshrc"; do
        if [ -f "$profile" ]; then
            if ! grep -q "$INSTALL_DIR" "$profile"; then
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

