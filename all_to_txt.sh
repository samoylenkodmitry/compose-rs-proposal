#!/bin/bash

# all_to_txt.sh - Generate compact project description for LLM agents
# Usage: ./all_to_txt.sh
# Output: single_file_code.md (compact version, excludes tests/benchmarks)

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
    # Use awk to remove #[cfg(test)] blocks and other test-related code
    awk '
    BEGIN { 
        in_test_block = 0
        brace_count = 0
        skip_line = 0
    }
    
    # Skip test modules and cfg(test) blocks
    /^[[:space:]]*#\[cfg\(test\)\]/ {
        in_test_block = 1
        next
    }
    
    # Skip lines starting with #[test]
    /^[[:space:]]*#\[test\]/ {
        skip_line = 1
        next
    }
    
    # If we hit a function after #[test], skip until we close the function
    skip_line == 1 && /^[[:space:]]*fn / {
        in_test_block = 1
        brace_count = 0
    }
    
    # Track braces when in test block
    in_test_block == 1 {
        for (i = 1; i <= length($0); i++) {
            char = substr($0, i, 1)
            if (char == "{") brace_count++
            else if (char == "}") brace_count--
        }
        if (brace_count <= 0) {
            in_test_block = 0
            skip_line = 0
        }
        next
    }
    
    # Process normal lines (remove comments, docs, empty lines)
    {
        # Remove line comments
        gsub(/\/\/.*$/, "")
        
        # Skip doc comments and attributes
        if (/^[[:space:]]*#!\[doc/ || /^[[:space:]]*#\[doc/) next
        
        # Skip empty lines
        if (/^[[:space:]]*$/) next
        
        # Remove std:: prefix
        gsub(/std::/, "")
        
        print
    }
    ' "$file"
}

# Generate the compact markdown file
{
    echo "# Compose-RS Code Structure (Compact)"
    echo ""

    # Generate directory tree showing only .rs files (excluding target, tests, benches)
    echo "## Files:"
    echo '```'
    find . -name "target" -prune -o -name "tests" -prune -o -name "benches" -prune -o -name "*.rs" -type f -print | \
    grep -v -E "(test|bench|example)" | sed 's|^\./||' | sort | while read file; do
        echo "$file"
    done
    echo '```'
    echo ""

    # Output each .rs file with its content (excluding target, tests, benches)
    echo "## Code:"
    echo ""

    find . -name "target" -prune -o -name "tests" -prune -o -name "benches" -prune -o -name "*.rs" -type f -print | \
    grep -v -E "(test|bench|example)" | sed 's|^\./||' | sort | while read file; do
        echo "**$file**"
        echo '```rust'
        optimize_rust_code "$file"
        echo '```'
    done
} > "$OUTPUT_FILE"

echo "Generated $OUTPUT_FILE successfully!"
echo "Mode: Compact (excludes tests/benchmarks/cfg(test) blocks, optimized for LLM tokens)"
echo "File size: $(du -h "$OUTPUT_FILE" | cut -f1)"
echo "Lines: $(wc -l < "$OUTPUT_FILE")"
echo "Words: $(wc -w < "$OUTPUT_FILE")"