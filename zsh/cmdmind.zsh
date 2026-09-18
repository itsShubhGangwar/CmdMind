# =============================================================================
# CmdMind — Direct Natural-Language Command Execution for macOS zsh
# =============================================================================
#
# This script integrates CmdMind directly into the macOS Zsh Line Editor (ZLE).
# It allows users to type natural-language requests (e.g. `find all pdf files`),
# press Enter once, and have CmdMind generate, validate, and execute the command.
#
# Usage:
#   source /path/to/cmdmind.zsh
#
# To unload:
#   cmdmind-unload
#
# Safety Architecture:
# - Conservative Natural-Language Detection:
#   Normal shell commands (e.g. `git status`, `ls -la`, `cd src`, `cargo test`)
#   are NEVER intercepted and run normally.
# - Escape Mechanism:
#   Prefixing a command with `\` or a leading space explicitly bypasses CmdMind.
# - Recursion Guard:
#   Subprocesses spawned by CmdMind use direct OS execution (`execve`) and do
#   not route back through the ZLE buffer.
# =============================================================================

# Guard against duplicate sourcing
if [[ -n "$CMDMIND_ZSH_LOADED" ]]; then
    return 0
fi
export CMDMIND_ZSH_LOADED=1

# Determine the CmdMind binary location
# Checks PATH first, then standard target locations
_cmdmind_find_binary() {
    if (( $+commands[cmdmind] )); then
        echo "cmdmind"
    elif [[ -x "./target/release/cmdmind" ]]; then
        echo "./target/release/cmdmind"
    elif [[ -x "./target/debug/cmdmind" ]]; then
        echo "./target/debug/cmdmind"
    elif [[ -x "$HOME/.cargo/bin/cmdmind" ]]; then
        echo "$HOME/.cargo/bin/cmdmind"
    elif [[ -x "$HOME/.local/bin/cmdmind" ]]; then
        echo "$HOME/.local/bin/cmdmind"
    else
        echo "cmdmind"
    fi
}

# Conservative Natural-Language Detection
# Returns 0 (true) if the input buffer is recognized as a natural language request.
# Returns 1 (false) if the buffer is a normal shell command, empty, or escaped.
_cmdmind_is_natural_language() {
    local raw_buf="$1"
    
    # 1. Trim leading and trailing whitespace
    local buf="${raw_buf#"${raw_buf%%[![:space:]]*}"}"
    buf="${buf%"${buf##*[![:space:]]}"}"

    # Empty buffer -> normal shell behavior
    if [[ -z "$buf" ]]; then
        return 1
    fi

    # 2. Escape mechanism:
    # A leading backslash (e.g. `\find`) or leading space forces normal shell execution
    if [[ "$raw_buf" =~ '^[[:space:]]' ]] || [[ "$buf" == \\* ]]; then
        return 1
    fi

    # 3. History bypass: `history` is handled normally by zsh unless explicitly `cmdmind history`
    if [[ "$buf" == "history" ]]; then
        return 1
    fi

    # Split into words
    local -a words
    words=(${(z)buf})
    local first_word="${words[1]}"
    local word_count=${#words[@]}

    # Single-word inputs (e.g. `ls`, `pwd`, `clear`, `top`) are standard shell commands
    if (( word_count <= 1 )); then
        return 1
    fi

    # 4. Check if the first word is a known executable, builtin, function, or alias
    local is_cmd=0
    if (( $+commands[$first_word] )) || (( $+builtins[$first_word] )) || \
       (( $+functions[$first_word] )) || (( $+aliases[$first_word] )); then
        is_cmd=1
    fi

    # If the first word is NOT any command/builtin/alias, and there are multiple words:
    # e.g. "show files larger than 500MB", "count lines in all rs files", "delete all tmp files"
    if (( is_cmd == 0 )); then
        return 0
    fi

    # 5. Standard developer tools that should NEVER be intercepted as natural language
    if [[ "$first_word" =~ '^(git|cargo|rustc|python|python3|node|npm|pnpm|yarn|make|docker|kubectl|ssh|scp|curl|wget|vim|nvim|nano|echo|printf|cat|grep|awk|sed|man|chmod|chown|kill|ps|which|where)$' ]]; then
        return 1
    fi

    # 6. For `find`:
    # Normal shell syntax: `find . ...`, `find /path ...`, `find -name ...`
    # Natural language syntax: `find all pdf files`, `find large files`, `find python files modified recently`
    if [[ "$first_word" == "find" ]]; then
        local second_word="${words[2]}"
        # If second word is a natural-language descriptor (not a flag, not an existing path)
        if [[ "$second_word" =~ '^(all|every|any|large|recent|new|old|pdf|python|rust|files|in|me)$' ]]; then
            return 0
        fi
        # If second word is an existing path or flag, it is normal shell find
        if [[ -e "$second_word" ]] || [[ "$second_word" == -* ]]; then
            return 1
        fi
    fi

    # 7. Common English phrasing prefixes
    if [[ "$buf" =~ '^(find all |show me |show files |list all |list files |search for |display all |count all )' ]]; then
        return 0
    fi

    # Default: fail closed, pass to standard zsh
    return 1
}

# ZLE interceptor widget bound to Enter (accept-line)
_cmdmind_accept_line() {
    # Recursion guard: if CmdMind is actively executing, pass directly to zsh
    if [[ -n "$CMDMIND_INTERCEPTING" ]]; then
        zle .accept-line
        return
    fi

    local input="$BUFFER"

    if _cmdmind_is_natural_language "$input"; then
        local binary
        binary=$(_cmdmind_find_binary)

        # Invalidate ZLE display to restore normal terminal mode for live child output
        zle -I

        # Set recursion guard
        export CMDMIND_INTERCEPTING=1

        # Add the original natural language command to zsh history
        print -s "$input"

        # Execute CmdMind directly
        "$binary" "$input"
        local exit_code=$?
        unset CMDMIND_INTERCEPTING

        # Clear buffer and refresh prompt for next input
        BUFFER=""
        zle .reset-prompt
        return $exit_code
    else
        # Normal shell command: pass directly to zsh
        zle .accept-line
    fi
}

# Bind to ZLE accept-line (Enter key)
zle -N accept-line _cmdmind_accept_line

# Clean unloader function
cmdmind-unload() {
    zle -A .accept-line accept-line 2>/dev/null || true
    unset CMDMIND_ZSH_LOADED
    echo "CmdMind zsh integration unloaded."
}
