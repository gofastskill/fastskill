#!/bin/bash
set -euo pipefail

# Constants
REPO="gofastskill/fastskill"
BINARY_NAME="fastskill"
DEFAULT_INSTALL_DIR="/usr/local/bin"
USER_INSTALL_DIR="$HOME/.local/bin"
GITHUB_API="https://api.github.com/repos/${REPO}/releases"
GITHUB_RELEASES="https://github.com/${REPO}/releases"

# Global variables
VERSION=""
INSTALL_DIR=""
FORCE=false
SHOW_VERSION_ONLY=false

# Colors for output (if terminal supports it)
if [[ -t 1 ]]; then
    RED='\033[0;31m'
    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    BLUE='\033[0;34m'
    NC='\033[0m' # No Color
else
    RED=''
    GREEN=''
    YELLOW=''
    BLUE=''
    NC=''
fi

# Print error message and exit
error() {
    echo -e "${RED}Error:${NC} $1" >&2
    exit 1
}

# Print warning message
warn() {
    echo -e "${YELLOW}Warning:${NC} $1" >&2
}

# Print info message
info() {
    echo -e "${BLUE}Info:${NC} $1"
}

# Print success message
success() {
    echo -e "${GREEN}Success:${NC} $1"
}

# Display help message
print_help() {
    cat << EOF
FastSkill Installation Script

Usage: $0 [OPTIONS] [VERSION]

Options:
    -h, --help          Show this help message
    -v, --version       Show version that will be installed (latest if not specified)
    -p, --prefix DIR    Install to custom directory (default: ${DEFAULT_INSTALL_DIR} or ${USER_INSTALL_DIR})
    -f, --force         Overwrite existing installation
    --user              Install to user directory (~/.local/bin) instead of system directory

Arguments:
    VERSION             Specific version to install (e.g., v0.6.8 or 0.6.8)
                        If not specified, installs the latest version

Notes:
    On Linux, the script automatically selects the appropriate binary based on your system:
    - glibc >= 2.38: uses gnu binary (for FIPS/compliance environments)
    - glibc < 2.38 or musl-based: uses musl binary (for maximum compatibility)

Examples:
    $0                                    # Install latest version
    $0 v0.6.8                            # Install specific version
    $0 --user                            # Install to ~/.local/bin
    $0 --prefix /opt/fastskill/bin       # Install to custom directory
    $0 --force v0.6.8                    # Overwrite existing installation

For more information, visit: https://github.com/${REPO}
EOF
}

# Check if required dependencies are available
check_dependencies() {
    local missing_deps=()
    
    if ! command -v curl &> /dev/null && ! command -v wget &> /dev/null; then
        missing_deps+=("curl or wget")
    fi
    
    if ! command -v tar &> /dev/null; then
        missing_deps+=("tar")
    fi
    
    if [ ${#missing_deps[@]} -ne 0 ]; then
        error "Missing required dependencies: ${missing_deps[*]}. Please install them and try again."
    fi
}

# Detect glibc version
detect_glibc_version() {
    local glibc_version=0
    
    # Try to get glibc version from ldd
    if command -v ldd &> /dev/null; then
        local ldd_output
        ldd_output=$(ldd --version 2>&1 | head -1)
        
        # Extract version number (e.g., "2.38" from "ldd (GNU libc) 2.38")
        if [[ "$ldd_output" =~ ([0-9]+\.[0-9]+) ]]; then
            glibc_version="${BASH_REMATCH[1]}"
        fi
    fi
    
    # If no glibc detected, assume musl (returns 0)
    echo "$glibc_version"
}

# Detect platform (OS and architecture)
detect_platform() {
    local os
    local arch
    
    # Detect OS
    case "$(uname -s)" in
        Linux*)
            os="linux"
            ;;
        Darwin*)
            os="macos"
            ;;
        MINGW*|MSYS*|CYGWIN*)
            os="windows"
            ;;
        *)
            error "Unsupported operating system: $(uname -s)"
            ;;
    esac
    
    # Detect architecture
    case "$(uname -m)" in
        x86_64|amd64)
            arch="x86_64"
            ;;
        aarch64|arm64)
            arch="arm64"
            ;;
        *)
            error "Unsupported architecture: $(uname -m)"
            ;;
    esac
    
    # Currently only Linux x86_64 is supported
    if [ "$os" != "linux" ] || [ "$arch" != "x86_64" ]; then
        error "Currently only Linux x86_64 is supported. Detected: ${os} ${arch}"
    fi
    
    # For Linux x86_64, detect glibc version to choose appropriate binary
    if [ "$os" = "linux" ] && [ "$arch" = "x86_64" ]; then
        local glibc_ver
        glibc_ver=$(detect_glibc_version)
        
        # Compare version: glibc >= 2.38 uses gnu binary, otherwise use musl
        # Use awk for floating point comparison
        if [ "$glibc_ver" != "0" ] && awk "BEGIN {exit !($glibc_ver >= 2.38)}" 2>/dev/null; then
            info "Detected glibc ${glibc_ver}, using gnu binary"
            echo "x86_64-unknown-linux-gnu"
        else
            if [ "$glibc_ver" = "0" ]; then
                info "Detected musl-based system, using musl binary"
            else
                info "Detected glibc ${glibc_ver} (< 2.38), using musl binary for compatibility"
            fi
            echo "x86_64-unknown-linux-musl"
        fi
        return
    fi
    
    echo "${arch}-unknown-${os}-gnu"
}

# Fetch latest version from GitHub API
fetch_latest_version() {
    local api_url="${GITHUB_API}/latest"
    local version
    
    info "Fetching latest version from GitHub..."
    
    if command -v curl &> /dev/null; then
        version=$(curl -sL "${api_url}" | grep -oP '"tag_name": "\K[^"]*' | head -1 || echo "")
    elif command -v wget &> /dev/null; then
        version=$(wget -qO- "${api_url}" | grep -oP '"tag_name": "\K[^"]*' | head -1 || echo "")
    fi
    
    if [ -z "$version" ]; then
        error "Failed to fetch latest version. Please check your internet connection or specify a version manually."
    fi
    
    # Remove 'v' prefix if present
    version="${version#v}"
    echo "$version"
}

# Validate that a version exists
validate_version() {
    local version="$1"
    local api_url="${GITHUB_API}/tags/v${version}"
    local exists
    
    # Remove 'v' prefix if present
    version="${version#v}"
    
    info "Validating version v${version}..."
    
    if command -v curl &> /dev/null; then
        exists=$(curl -sL -o /dev/null -w "%{http_code}" "${api_url}" || echo "000")
    elif command -v wget &> /dev/null; then
        exists=$(wget --spider -q -S "${api_url}" 2>&1 | grep -oP 'HTTP/\d\.\d \K\d{3}' | head -1 || echo "000")
    fi
    
    if [ "$exists" != "200" ]; then
        error "Version v${version} not found. Check available versions at: ${GITHUB_RELEASES}"
    fi
    
    echo "$version"
}

# Determine install directory
determine_install_dir() {
    local custom_dir="$1"
    local use_user="$2"
    
    if [ -n "$custom_dir" ]; then
        INSTALL_DIR="$custom_dir"
        return
    fi
    
    if [ "$use_user" = true ]; then
        INSTALL_DIR="$USER_INSTALL_DIR"
        return
    fi
    
    # Try system directory first, fallback to user directory
    if [ -w "$DEFAULT_INSTALL_DIR" ] || command -v sudo &> /dev/null; then
        INSTALL_DIR="$DEFAULT_INSTALL_DIR"
    else
        warn "Cannot write to ${DEFAULT_INSTALL_DIR}. Installing to ${USER_INSTALL_DIR} instead."
        INSTALL_DIR="$USER_INSTALL_DIR"
    fi
}

# Check if binary already exists
check_existing_installation() {
    local binary_path="${INSTALL_DIR}/${BINARY_NAME}"
    
    if [ -f "$binary_path" ]; then
        if [ "$FORCE" = false ]; then
            error "FastSkill is already installed at ${binary_path}. Use --force to overwrite."
        else
            info "Existing installation found at ${binary_path}. Will overwrite with --force."
        fi
    fi
}

# Download and extract binary
download_binary() {
    local version="$1"
    local platform="$2"
    local archive_name="fastskill-${platform}.tar.gz"
    local download_url="${GITHUB_RELEASES}/download/v${version}/${archive_name}"
    local temp_dir
    local archive_path
    
    # Create temporary directory
    temp_dir=$(mktemp -d)
    archive_path="${temp_dir}/${archive_name}"
    
    # Cleanup on exit
    cleanup_temp_dir() {
        rm -rf "$temp_dir"
    }
    trap cleanup_temp_dir EXIT
    
    info "Downloading FastSkill v${version}..."
    
    # Download
    if command -v curl &> /dev/null; then
        if ! curl -fsSL -o "$archive_path" "$download_url"; then
            error "Failed to download binary. Check your internet connection and try again."
        fi
    elif command -v wget &> /dev/null; then
        if ! wget -q -O "$archive_path" "$download_url"; then
            error "Failed to download binary. Check your internet connection and try again."
        fi
    fi
    
    # Verify archive exists and is not empty
    if [ ! -f "$archive_path" ] || [ ! -s "$archive_path" ]; then
        error "Downloaded archive is empty or corrupted."
    fi
    
    info "Extracting binary..."
    
    # Extract
    cd "$temp_dir"
    if ! tar -xzf "$archive_path"; then
        error "Failed to extract archive. The download may be corrupted."
    fi
    
    # Verify binary exists after extraction
    if [ ! -f "${temp_dir}/${BINARY_NAME}" ]; then
        error "Binary not found in archive. The release may be malformed."
    fi
    
    # Make binary executable
    chmod +x "${temp_dir}/${BINARY_NAME}"
    
    echo "$temp_dir"
}

# Install binary to target directory
install_binary() {
    local source_dir="$1"
    local source_binary="${source_dir}/${BINARY_NAME}"
    local target_binary="${INSTALL_DIR}/${BINARY_NAME}"
    
    # Create install directory if it doesn't exist
    if [ ! -d "$INSTALL_DIR" ]; then
        info "Creating directory ${INSTALL_DIR}..."
        mkdir -p "$INSTALL_DIR"
    fi
    
    # Check if we need sudo
    if [ ! -w "$INSTALL_DIR" ]; then
        if ! command -v sudo &> /dev/null; then
            error "Cannot write to ${INSTALL_DIR} and sudo is not available. Try --user flag or --prefix with a writable directory."
        fi
        
        info "Installing to ${INSTALL_DIR} (requires sudo)..."
        sudo mv "$source_binary" "$target_binary"
        sudo chmod +x "$target_binary"
    else
        info "Installing to ${INSTALL_DIR}..."
        mv "$source_binary" "$target_binary"
        chmod +x "$target_binary"
    fi
    
    # Verify installation
    if [ ! -f "$target_binary" ]; then
        error "Installation failed. Binary not found at ${target_binary}."
    fi
    
    # Check if directory is in PATH
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        warn "${INSTALL_DIR} is not in your PATH. Add it to your shell profile:"
        echo "  export PATH=\"\${PATH}:${INSTALL_DIR}\""
    fi
}

# Verify installation
verify_installation() {
    local binary_path="${INSTALL_DIR}/${BINARY_NAME}"
    
    info "Verifying installation..."
    
    if [ ! -f "$binary_path" ]; then
        error "Installation verification failed. Binary not found."
    fi
    
    if ! "$binary_path" --version &> /dev/null; then
        error "Installation verification failed. Binary is not executable or corrupted."
    fi
    
    local installed_version
    installed_version=$("$binary_path" --version 2>&1 | head -1 || echo "unknown")
    
    success "FastSkill installed successfully!"
    echo ""
    echo "  Binary location: ${binary_path}"
    echo "  Version: ${installed_version}"
    echo ""
    echo "Run '${BINARY_NAME} --help' to get started."
}

# Parse command line arguments
parse_args() {
    local custom_prefix=""
    local use_user=false
    
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                print_help
                exit 0
                ;;
            -v|--version)
                SHOW_VERSION_ONLY=true
                shift
                ;;
            -p|--prefix)
                if [ -z "${2:-}" ]; then
                    error "--prefix requires a directory path"
                fi
                custom_prefix="$2"
                shift 2
                ;;
            -f|--force)
                FORCE=true
                shift
                ;;
            --user)
                use_user=true
                shift
                ;;
            -*)
                error "Unknown option: $1. Use --help for usage information."
                ;;
            *)
                if [ -z "$VERSION" ]; then
                    VERSION="$1"
                else
                    error "Multiple versions specified. Use --help for usage information."
                fi
                shift
                ;;
        esac
    done
    
    determine_install_dir "$custom_prefix" "$use_user"
}

# Main function
main() {
    local platform
    local temp_dir
    
    # Parse arguments
    parse_args "$@"
    
    # Check dependencies
    check_dependencies
    
    # Detect platform
    platform=$(detect_platform)
    
    # Get version
    if [ -z "$VERSION" ]; then
        VERSION=$(fetch_latest_version)
    else
        VERSION=$(validate_version "$VERSION")
    fi
    
    # Show version only if requested
    if [ "$SHOW_VERSION_ONLY" = true ]; then
        echo "FastSkill v${VERSION}"
        exit 0
    fi
    
    info "Installing FastSkill v${VERSION} for ${platform}..."
    
    # Check for existing installation
    check_existing_installation
    
    # Download and extract
    temp_dir=$(download_binary "$VERSION" "$platform")
    
    # Install
    install_binary "$temp_dir"
    
    # Verify
    verify_installation
}

# Run main function
main "$@"

