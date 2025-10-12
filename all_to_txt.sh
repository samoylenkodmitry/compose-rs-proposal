#!/bin/bash

# all_to_txt.sh - Generate compact project description for LLM agents
# Usage: ./all_to_txt.sh
# Output: single_file_code.md (compact version by default)

# Get the project root directory
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# Output file
OUTPUT_FILE="single_file_code.md"

# Always use compact mode
COMPACT_MODE=true

# Function to remove comments and optimize Rust code for token reduction
optimize_rust_code() {
    local file="$1"
    # Compact: remove comments, docs, and empty lines
    sed -e 's|//.*$||g' \
        -e '/^[[:space:]]*$/d' \
        -e '/^[[:space:]]*#!\[doc/d' \
        -e '/^[[:space:]]*#\[doc/d' \
        -e '/^[[:space:]]*\/\*\*/,/\*\//d' \
        -e '/^[[:space:]]*\/\*/,/\*\//d' \
        "$file" | \
    sed -e 's/std:://g' \
        -e '/^[[:space:]]*$/d'
}

# Generate the compact markdown file
{
    echo "# Compose-RS Code Structure (Compact)"
    echo ""

    # Generate directory tree showing only .rs files (excluding target directory)
    echo "## Files:"
    echo '```'
    find . -name "target" -prune -o -name "*.rs" -type f -print | sed 's|^\./||' | sort | while read file; do
        echo "$file"
    done
    echo '```'
    echo ""

    # Output each .rs file with its content (excluding target directory)
    echo "## Code:"
    echo ""

    find . -name "target" -prune -o -name "*.rs" -type f -print | sed 's|^\./||' | sort | while read file; do
        echo "**$file**"
        echo '```rust'
        optimize_rust_code "$file"
        echo '```'
    done
} > "$OUTPUT_FILE"

echo "Generated $OUTPUT_FILE successfully!"
echo "Mode: Compact (optimized for LLM tokens)"
echo "File size: $(du -h "$OUTPUT_FILE" | cut -f1)"
echo "Lines: $(wc -l < "$OUTPUT_FILE")"
echo "Words: $(wc -w < "$OUTPUT_FILE")"