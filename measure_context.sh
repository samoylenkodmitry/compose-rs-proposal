#!/bin/bash

# Define the input markdown file containing the codebase
INPUT_FILE="single_file_code.md"
# Define the output markdown file for the measurement context
OUTPUT_FILE="measure_context.md"

# List of relevant files to investigate the slow measure pass.
# This list focuses on layout, measurement policies, and core composition logic.
RELEVANT_FILES=(
  "crates/compose-ui-layout/src/core.rs"
  "crates/compose-ui-layout/src/constraints.rs"
  "crates/compose-ui-layout/src/lib.rs"
  "crates/compose-ui/src/layout/core.rs"
  "crates/compose-ui/src/layout/mod.rs"
  "crates/compose-ui/src/layout/policies.rs"
  "crates/compose-ui/src/subcompose_layout.rs"
  "crates/compose-ui/src/widgets/layout.rs"
  "crates/compose-foundation/src/modifier.rs"
  "crates/compose-ui/src/modifier/mod.rs"
  "crates/compose-ui/src/modifier/padding.rs"
  "crates/compose-ui/src/modifier_nodes.rs"
  "crates/compose-ui/src/widgets/row.rs"
  "crates/compose-ui/src/widgets/column.rs"
  "crates/compose-ui/src/widgets/box_widget.rs"
  "crates/compose-ui/src/widgets/text.rs"
  "crates/compose-ui/src/widgets/nodes/layout_node.rs"
  "crates/compose-app-shell/src/lib.rs"
  "crates/compose-core/src/lib.rs"
  "apps/desktop-demo/src/main.rs"
)

# Check if input file exists
if [[ ! -f "$INPUT_FILE" ]]; then
  echo "Error: Input file '$INPUT_FILE' not found."
  exit 1
fi

# Clear or create the output file
> "$OUTPUT_FILE"

echo "Processing files and creating $OUTPUT_FILE..."

# Loop through the relevant files
for file_path in "${RELEVANT_FILES[@]}"; do
  echo "Extracting: $file_path"
  # Append the file path header to the output file
  echo "**$file_path**" >> "$OUTPUT_FILE"
  echo '```rust' >> "$OUTPUT_FILE"

  # Use awk to find the file section and extract the code block
  # It looks for the bold file path, then prints lines until the closing ```
  awk -v path="**${file_path}**" '
    BEGIN { found=0 }
    $0 == path {
      found=1
      # Skip the file path line itself and the opening ```rust line
      getline
      next
    }
    found && /^```$/ {
      found=0
      next
    }
    found {
      print
    }
  ' "$INPUT_FILE" >> "$OUTPUT_FILE"

  # Add the closing code block and a newline separator
  echo '```' >> "$OUTPUT_FILE"
  echo "" >> "$OUTPUT_FILE"
done

echo "Finished creating $OUTPUT_FILE."

# You can make the script executable by running:
# chmod +x measure_context.sh