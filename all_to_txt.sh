#!/bin/bash

# all_to_txt.sh - Generate concise project description for LLM agents
# Usage: ./all_to_txt.sh
# Output: single_file_code.md

# Get the project root directory
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# Output file
OUTPUT_FILE="single_file_code.md"

# Generate the markdown file
{
    echo "# Compose-RS Project Structure and Source Code"
    echo ""

    # Generate directory tree showing only .rs files (excluding target directory)
    echo "## Directory Structure (.rs files only):"
    echo '```'
    find . -name "target" -prune -o -name "*.rs" -type f -print | sed 's|^\./||' | sort | while read file; do
        echo "$file"
    done
    echo '```'
    echo ""

    # Generate table of contents with links
    echo "## Table of Contents"
    echo ""
    find . -name "target" -prune -o -name "*.rs" -type f -print | sed 's|^\./||' | sort | while read file; do
        # Convert file path to markdown anchor (replace special chars with hyphens, lowercase)
        anchor=$(echo "$file" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]/-/g' | sed 's/--*/-/g' | sed 's/^-\|-$//g')
        echo "- [$file](#$anchor)"
    done
    echo ""

    # Output each .rs file with its content (excluding target directory)
    echo "## Source Code Files:"
    echo ""

    find . -name "target" -prune -o -name "*.rs" -type f -print | sed 's|^\./||' | sort | while read file; do
        echo "### $file"
        echo '```rust'
        cat "$file"
        echo '```'
        echo ""
    done
} > "$OUTPUT_FILE"

echo "Generated $OUTPUT_FILE successfully!"
echo "File size: $(du -h "$OUTPUT_FILE" | cut -f1)"
echo "Lines: $(wc -l < "$OUTPUT_FILE")"