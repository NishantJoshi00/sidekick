#!/usr/bin/env bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
print_error() {
    printf "${RED}✗ %s${NC}\n" "$1"
}

print_success() {
    printf "${GREEN}✓ %s${NC}\n" "$1"
}

print_info() {
    printf "${BLUE}ℹ %s${NC}\n" "$1"
}

print_warning() {
    printf "${YELLOW}⚠ %s${NC}\n" "$1"
}

# Check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Detect Python command (python3 or python)
detect_python() {
    if command_exists python3; then
        echo "python3"
    elif command_exists python; then
        echo "python"
    else
        echo ""
    fi
}

# Detect shell and corresponding RC file
detect_shell_rc() {
    local shell_name
    shell_name=$(basename "$SHELL")

    case "$shell_name" in
        bash)
            if [[ "$OSTYPE" == "darwin"* ]]; then
                # macOS uses .bash_profile
                if [ -f "$HOME/.bash_profile" ]; then
                    echo "$HOME/.bash_profile"
                else
                    echo "$HOME/.bashrc"
                fi
            else
                echo "$HOME/.bashrc"
            fi
            ;;
        zsh)
            echo "$HOME/.zshrc"
            ;;
        fish)
            echo "$HOME/.config/fish/config.fish"
            ;;
        *)
            echo "$HOME/.profile"
            ;;
    esac
}

# Check if sidekick is already installed
check_sidekick_installed() {
    command_exists sidekick
}

# Check if Claude hooks are configured
check_claude_hooks() {
    local settings_file="$HOME/.claude/settings.json"
    if [ ! -f "$settings_file" ]; then
        return 1
    fi

    # Check if sidekick hook exists in the settings
    if grep -q "sidekick hook" "$settings_file" 2>/dev/null; then
        return 0
    fi
    return 1
}

# Check if shell alias is configured
check_shell_alias() {
    local rc_file=$(detect_shell_rc)
    if [ -f "$rc_file" ] && grep -q "alias nvim='sidekick neovim'" "$rc_file" 2>/dev/null; then
        return 0
    fi
    return 1
}

# Main installation
main() {
    echo ""
    echo "=========================================="
    echo "  Sidekick Installer"
    echo "  Your AI Should Be a Sidekick, Not Pilot"
    echo "=========================================="
    echo ""

    # Pre-installation verification
    print_info "Checking existing Sidekick configuration..."
    echo ""

    local sidekick_installed=false
    local hooks_configured=false
    local alias_configured=false
    local needs_installation=false

    if check_sidekick_installed; then
        print_success "Sidekick is already installed"
        sidekick_installed=true
    else
        print_info "Sidekick not found - will install"
        needs_installation=true
    fi

    if check_claude_hooks; then
        print_success "Claude Code hooks already configured"
        hooks_configured=true
    else
        print_info "Claude Code hooks not configured - will configure"
        needs_installation=true
    fi

    if check_shell_alias; then
        print_success "Shell alias already configured"
        alias_configured=true
    else
        print_info "Shell alias not configured - will configure"
        needs_installation=true
    fi

    echo ""

    # If everything is already configured, exit
    if [ "$sidekick_installed" = true ] && [ "$hooks_configured" = true ] && [ "$alias_configured" = true ]; then
        echo "=========================================="
        print_success "Sidekick is fully configured!"
        echo "=========================================="
        echo ""
        print_info "You're all set. Just use: nvim <file>"
        echo ""
        exit 0
    fi

    # Ask for confirmation if partial configuration exists (only in interactive mode)
    if [ "$sidekick_installed" = true ] || [ "$hooks_configured" = true ] || [ "$alias_configured" = true ]; then
        echo ""
        print_warning "Some components are already configured."
        print_info "The installer will only configure missing components."
        echo ""

        # Check if running in interactive mode (not piped)
        if [ -t 0 ]; then
            read -p "Continue with installation? (y/N) " -n 1 -r
            echo ""
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                print_info "Installation cancelled"
                exit 0
            fi
            echo ""
        else
            print_info "Running in non-interactive mode, proceeding automatically..."
            echo ""
        fi
    fi

    # Step 1: Check dependencies
    print_info "Checking dependencies..."

    local missing_deps=()

    # Check for Rust/Cargo
    if ! command_exists cargo; then
        missing_deps+=("cargo (Rust)")
    fi

    # Check for Python
    PYTHON_CMD=$(detect_python)
    if [ -z "$PYTHON_CMD" ]; then
        missing_deps+=("python3 or python")
    fi

    # Check for jq (optional but recommended)
    if ! command_exists jq; then
        print_warning "jq is not installed (optional, but recommended for JSON manipulation)"
    fi

    # If missing dependencies, show error and exit
    if [ ${#missing_deps[@]} -gt 0 ]; then
        print_error "Missing required dependencies:"
        for dep in "${missing_deps[@]}"; do
            echo "  - $dep"
        done
        echo ""

        if [[ " ${missing_deps[@]} " =~ " cargo (Rust) " ]]; then
            print_info "Install Rust with:"
            echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        fi

        if [[ " ${missing_deps[@]} " =~ " python3 or python " ]]; then
            print_info "Install Python from: https://www.python.org/downloads/"
        fi

        exit 1
    fi

    print_success "All dependencies found"
    echo ""

    # Step 2: Install Sidekick (only if not already installed)
    if [ "$sidekick_installed" = false ]; then
        print_info "Installing Sidekick..."

        if cargo install sidekick; then
            print_success "Sidekick installed successfully"
        else
            print_error "Failed to install Sidekick"
            exit 1
        fi
        echo ""
    fi

    # Step 3: Configure Claude Code hooks (only if not already configured)
    if [ "$hooks_configured" = false ]; then
        print_info "Configuring Claude Code hooks..."

        CLAUDE_SETTINGS_DIR="$HOME/.claude"
        CLAUDE_SETTINGS_FILE="$CLAUDE_SETTINGS_DIR/settings.json"

        # Create .claude directory if it doesn't exist
        if [ ! -d "$CLAUDE_SETTINGS_DIR" ]; then
            mkdir -p "$CLAUDE_SETTINGS_DIR"
            print_info "Created $CLAUDE_SETTINGS_DIR directory"
        fi

        # Create or update settings.json with Python
        $PYTHON_CMD << 'EOF'
import json
import os
import sys
from pathlib import Path

settings_file = Path.home() / ".claude" / "settings.json"

# Hook configuration
sidekick_hook = {
    "type": "command",
    "command": "sidekick hook"
}

hook_config = {
    "matcher": "MultiEdit|Edit|Write",
    "hooks": [sidekick_hook]
}

# Load existing settings or create new
if settings_file.exists():
    with open(settings_file, 'r') as f:
        try:
            settings = json.load(f)
        except json.JSONDecodeError:
            settings = {}
else:
    settings = {}

# Ensure hooks structure exists
if "hooks" not in settings:
    settings["hooks"] = {}

# Add PreToolUse hooks
if "PreToolUse" not in settings["hooks"]:
    settings["hooks"]["PreToolUse"] = []

# Add PostToolUse hooks
if "PostToolUse" not in settings["hooks"]:
    settings["hooks"]["PostToolUse"] = []

# Check if sidekick hook already exists
def has_sidekick_hook(hooks_list):
    for hook_entry in hooks_list:
        if isinstance(hook_entry, dict) and hook_entry.get("matcher") == "MultiEdit|Edit|Write":
            for hook in hook_entry.get("hooks", []):
                if hook.get("command") == "sidekick hook":
                    return True
    return False

# Add hooks if not present
pre_exists = has_sidekick_hook(settings["hooks"]["PreToolUse"])
post_exists = has_sidekick_hook(settings["hooks"]["PostToolUse"])

if not pre_exists:
    settings["hooks"]["PreToolUse"].append(hook_config)

if not post_exists:
    settings["hooks"]["PostToolUse"].append(hook_config)

# Write back to file
with open(settings_file, 'w') as f:
    json.dump(settings, f, indent=2)

if pre_exists and post_exists:
    print("Hooks already configured")
else:
    print("Hooks configured successfully")
EOF

        if [ $? -eq 0 ]; then
            print_success "Claude Code hooks configured"
        else
            print_error "Failed to configure Claude Code hooks"
            exit 1
        fi
        echo ""
    fi

    # Step 4: Add shell alias (only if not already configured)
    if [ "$alias_configured" = false ]; then
        print_info "Configuring shell alias..."

        RC_FILE=$(detect_shell_rc)
        ALIAS_LINE="alias nvim='sidekick neovim'"

        # Add alias to RC file
        echo "" >> "$RC_FILE"
        echo "# Sidekick - AI assistant integration" >> "$RC_FILE"
        echo "$ALIAS_LINE" >> "$RC_FILE"
        print_success "Alias added to $RC_FILE"
        echo ""
    fi

    echo "=========================================="
    print_success "Installation complete!"
    echo "=========================================="
    echo ""
    print_info "Next steps:"
    if [ "$alias_configured" = false ]; then
        RC_FILE=$(detect_shell_rc)
        echo "  1. Restart your shell or run: source $RC_FILE"
        echo "  2. Start using: nvim <file>"
    else
        echo "  1. Start using: nvim <file>"
    fi
    echo ""
    print_info "The alias 'nvim' now launches Neovim with Sidekick integration"
    print_info "Claude Code will automatically respect your unsaved changes"
    echo ""
}

# Run main installation
main
