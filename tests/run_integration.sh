#!/bin/bash
# Integration test: Generate all rotations, PDFs and LIN from ABS2-2.pbn

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
INPUT_FILE="$SCRIPT_DIR/fixtures/input/ABS2-2.pbn"
OUTPUT_DIR="$SCRIPT_DIR/fixtures/output"

# Build the project first
echo "Building bridge-wrangler..."
cd "$PROJECT_DIR"
# CI-parity build: never bare cargo, which would apply local [patch] overrides
# and rewrite Cargo.lock (see CLAUDE.md).
./dev-build.sh --ci build --release

BINARY="$PROJECT_DIR/target/release/bridge-wrangler"

# Clean output directory
echo "Cleaning output directory..."
rm -f "$OUTPUT_DIR"/*.pbn "$OUTPUT_DIR"/*.pdf "$OUTPUT_DIR"/*.lin

# Run rotate-deals with all patterns
echo ""
echo "=== Running rotate-deals with patterns: S, NS, NES, NESW ==="
"$BINARY" rotate-deals -i "$INPUT_FILE" -p "S,NS,NES,NESW" --standard-vul

# Move the generated PBN files to output directory
echo ""
echo "Moving PBN files to output directory..."
INPUT_DIR="$(dirname "$INPUT_FILE")"
mv "$INPUT_DIR/ABS2-2 - S.pbn" "$OUTPUT_DIR/"
mv "$INPUT_DIR/ABS2-2 - NS.pbn" "$OUTPUT_DIR/"
mv "$INPUT_DIR/ABS2-2 - NES.pbn" "$OUTPUT_DIR/"
mv "$INPUT_DIR/ABS2-2 - NESW.pbn" "$OUTPUT_DIR/"

# Generate PDFs for each rotation
echo ""
echo "=== Generating PDFs ==="

for pattern in S NS NES NESW; do
    pbn_file="$OUTPUT_DIR/ABS2-2 - $pattern.pbn"
    pdf_file="$OUTPUT_DIR/ABS2-2 - $pattern.pdf"
    echo "Generating PDF for $pattern pattern..."
    "$BINARY" to-pdf -i "$pbn_file" -o "$pdf_file"
done

# Also generate a PDF from the original (unrotated) file
echo "Generating PDF for original (unrotated)..."
"$BINARY" to-pdf -i "$INPUT_FILE" -o "$OUTPUT_DIR/ABS2-2 - Original.pdf"

# Generate LIN for each rotation, to set beside Bridge Composer's exports in
# fixtures/expected (those carry a tournament header this writer omits)
echo ""
echo "=== Generating LIN ==="

for pattern in S NS NES NESW; do
    echo "Generating LIN for $pattern pattern..."
    "$BINARY" to-lin -i "$OUTPUT_DIR/ABS2-2 - $pattern.pbn" -o "$OUTPUT_DIR/ABS2-2 - $pattern.lin"
done

# Summary
echo ""
echo "=== Integration Test Complete ==="
echo "Output directory: $OUTPUT_DIR"
echo ""
echo "Generated files:"
ls -la "$OUTPUT_DIR"
