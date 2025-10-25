#!/bin/bash

# Define the input markdown file containing the codebase
INPUT_FILE="single_file_code.md"
# Define the output markdown file
OUTPUT_FILE="composer_context.md"

# List of relevant files to include in the context
# Adjust this list if needed
RELEVANT_FILES=(
  "crates/compose-core/src/lib.rs"
  "crates/compose-core/src/composer_context.rs"
  "crates/compose-macros/src/lib.rs"
  "crates/compose-core/src/runtime.rs"
  "crates/compose-core/src/snapshot.rs"
  "crates/compose-core/src/state.rs"
  "crates/compose-app-shell/src/lib.rs"
  "crates/compose-ui/src/widgets/nodes/layout_node.rs"
  "crates/compose-ui/src/layout/mod.rs"
  "crates/compose-ui/src/widgets/layout.rs"
  "crates/compose-core/src/launched_effect.rs"
  "crates/compose-core/src/subcompose.rs"
  "crates/compose-ui/src/subcompose_layout.rs"
  "crates/compose-runtime-std/src/lib.rs"
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
  # Append the file path to the output file
  echo "$file_path" >> "$OUTPUT_FILE"
  echo "" >> "$OUTPUT_FILE" # Add a newline for separation

  # Use awk to find the file section and extract the code block
  # It looks for the bold file path, then prints lines until the closing ```
  awk -v path="**${file_path}**" '
    $0 == path {
      found=1
      # Skip the file path line itself and the ```rust line
      getline
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

  # Add a newline separator between files in the output
  echo "" >> "$OUTPUT_FILE"
done

echo "Finished creating $OUTPUT_FILE."

# Make the script executable (optional, can be done manually)
# chmod +x composer_context.sh
