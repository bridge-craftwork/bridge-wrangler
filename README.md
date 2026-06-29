# bridge-wrangler

CLI tool for operations on bridge PBN (Portable Bridge Notation) files.

## Installation

```bash
cargo build --release
```

The binary will be at `target/release/bridge-wrangler`.

## Commands

| Command | Description |
|---------|-------------|
| [rotate-deals](#rotate-deals) | Rotate deals to set dealer/declarer according to a pattern |
| [to-pdf](#to-pdf) | Convert PBN file to PDF with various layouts |
| [to-lin](#to-lin) | Convert PBN file to LIN format (Bridge Base Online) |
| [analyze](#analyze) | Perform double-dummy analysis on deals |
| [block-replicate](#block-replicate) | Replicate boards into blocks for multi-table play |
| [filter](#filter) | Filter boards by regex pattern |
| [event](#event) | Update the Event tag for all boards |

---

### rotate-deals

Rotate deals to set the dealer (or declarer) according to a repeating pattern. This is useful for creating practice sets where a specific player should be dealer for each board.

```bash
bridge-wrangler rotate-deals --input <FILE> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--output <FILE>` | `-o` | Output PBN file (not used with multi-pattern) | `<input> - <PATTERN>.pbn` |
| `--pattern <PATTERN>` | `-p` | Rotation pattern(s), comma-separated (see below) | `NESW` |
| `--basis <BASIS>` | `-b` | How to determine current orientation | `standard` |
| `--standard-vul` | - | Use standard vulnerability by board number | off |

#### Patterns

The pattern specifies the target dealer for each board, cycling through the pattern as needed:

- `N` - All boards dealer is North
- `S` - All boards dealer is South
- `NS` - Board 1 North, Board 2 South, Board 3 North, etc.
- `NESW` - Standard rotation: Board 1 North, Board 2 East, Board 3 South, Board 4 West, then repeats

**Multiple patterns**: Use commas to generate multiple output files in one run:
```bash
bridge-wrangler rotate-deals -i deals.pbn -p "S,NS,NESW"
# Creates: deals - S.pbn, deals - NS.pbn, deals - NESW.pbn
```

#### Basis Options

The basis determines how the tool identifies the current orientation of each board:

- `standard` - Priority: RotationBasis tag > Student tag > Declarer > Dealer (default, matches Bridge Composer)
- `basis-tag` - Use the RotationBasis PBN tag
- `student` - Use the Student tag
- `declarer` - Use the Declarer tag
- `dealer` - Use the Dealer tag
- `deal` - Use the Deal tag's first character (starting seat)
- `north` - Assume all boards are oriented to North
- `south` - Assume all boards are oriented to South
- `east` - Assume all boards are oriented to East
- `west` - Assume all boards are oriented to West

#### Examples

Rotate all boards so South is dealer:
```bash
bridge-wrangler rotate-deals -i practice.pbn -p S
```

Create a set where boards alternate between North and South dealer:
```bash
bridge-wrangler rotate-deals -i hands.pbn -p NS -o hands-ns.pbn
```

Generate multiple rotations at once:
```bash
bridge-wrangler rotate-deals -i lesson.pbn -p "S,NS,NES,NESW"
```

Rotate boards assuming they're all currently oriented to North:
```bash
bridge-wrangler rotate-deals -i deals.pbn -p NESW -b north
```

#### What Gets Rotated

- **Dealer** - Rotated to match the target direction
- **Vulnerable** - Swapped between NS/EW for odd rotations (or set to standard if `--standard-vul`)
- **Deal** - Hands are moved around the table to match the new orientation
- **Declarer** - Rotated to match the new orientation
- **Auction** - Starting seat rotated
- **Play** - Opening leader rotated
- **Score** - NS/EW prefix swapped for odd rotations
- **Commentary** - Direction words (North, South, East, West) rotated in text

### to-pdf

Convert PBN files to PDF format with various layout options.

```bash
bridge-wrangler to-pdf --input <FILE> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--output <FILE>` | `-o` | Output PDF file | `<input>.pdf` |
| `--layout <LAYOUT>` | `-l` | Layout style (see below) | `analysis` |
| `--boards-per-page <N>` | `-b` | Boards per page (1, 2, or 4) | varies by layout |
| `--board-range <RANGE>` | `-r` | Board range to include | all boards |
| `--hide-bidding` | - | Hide bidding information | off |
| `--hide-play` | - | Hide play sequence | off |
| `--hide-commentary` | - | Hide commentary | off |
| `--show-hcp` | - | Show high card points | off |

#### Layouts

- `analysis` - Full hand diagram with bidding table and commentary (default)
- `bidding-sheets` - Simplified layout for practice bidding
- `declarers-plan` - 4 deals per page for declarer's planning practice

#### Board Range

Specify which boards to include using ranges or lists:
- `1-4` - Boards 1 through 4
- `1,3,5` - Boards 1, 3, and 5
- `1-4,7,9-12` - Combination of ranges and individual boards

#### Examples

Convert a PBN file to PDF:
```bash
bridge-wrangler to-pdf -i lesson.pbn
# Creates: lesson.pdf
```

Create PDF with only boards 1-4:
```bash
bridge-wrangler to-pdf -i hands.pbn -r "1-4" -o first-four.pdf
```

Create a declarer's plan practice sheet:
```bash
bridge-wrangler to-pdf -i deals.pbn -l declarers-plan
```

### to-lin

Convert PBN files to LIN format (Bridge Base Online's linear format). LIN is a pipe-delimited format used by BBO for hand records that can encode deals, auctions, and cardplay.

```bash
bridge-wrangler to-lin --input <FILE> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--output <FILE>` | `-o` | Output LIN file | `<input>.lin` |

#### Output Format

Each board is encoded on a single line containing:
- `pn|S,W,N,E|` - Player names (South, West, North, East order)
- `md|dealer+hands|` - Dealer digit (1=S, 2=W, 3=N, 4=E) + hands in S,W,N order
- `sv|v|` - Vulnerability (o=none, n=NS, e=EW, b=both)
- `ah|Board N|` - Board header
- `mb|bid|` - Each bid in the auction
- `pc|card|` - Each card played

#### Examples

Convert a PBN file to LIN:
```bash
bridge-wrangler to-lin -i hands.pbn
# Creates: hands.lin
```

Specify output file:
```bash
bridge-wrangler to-lin -i session.pbn -o bbo-upload.lin
```

### analyze

Perform double-dummy analysis on deals and optionally add results to PBN files.

```bash
bridge-wrangler analyze --input <FILE> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--output <FILE>` | `-o` | Output PBN file with DD results | - |
| `--board-range <RANGE>` | `-r` | Board range to analyze | all boards |
| `--verbose` | `-v` | Show DD results table and par scores | off |

#### Output

By default, the command runs quietly and only reports progress. Use `-v` to display the DD results table showing tricks for each declarer (N, S, E, W) in each denomination (NT, S, H, D, C):

```
       NT   S   H   D   C
  N     7   6   7   6   6
  S     7   6   7   6   6
  E     6   6   6   6   6
  W     6   6   6   6   6
```

When using `--output`, the results are added to the PBN file as `[OptimumResultTable]` tags.

#### Examples

Analyze all boards (quiet mode):
```bash
bridge-wrangler analyze -i hands.pbn
```

Analyze and display DD results with par scores:
```bash
bridge-wrangler analyze -i hands.pbn -v
```

Analyze and save results to a new PBN file:
```bash
bridge-wrangler analyze -i hands.pbn -o hands-analyzed.pbn
```

Analyze only boards 1-4:
```bash
bridge-wrangler analyze -i hands.pbn -r "1-4" -v
```

### block-replicate

Replicate boards into blocks for multi-table play. This creates copies of the input boards with correct dealer and vulnerability for each board position, adding tracking tags for the original ("virtual") board information.

```bash
bridge-wrangler block-replicate --input <FILE> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--output <FILE>` | `-o` | Output PBN file | `<input> - <B>x<C>.pbn` |
| `--block-size <N>` | `-b` | Number of boards per block | number of input boards |
| `--block-count <N>` | `-c` | Number of blocks to create | fills to 36 boards |
| `--pdf` | | Also generate a PDF hand record | off |

#### How It Works

The command replicates input boards into multiple blocks. Each block contains the same deals as the original, but with:

- **Board numbers** assigned sequentially (1, 2, 3, ...)
- **Dealer** set according to standard pattern (N, E, S, W, repeating)
- **Vulnerability** set according to standard 16-board pattern

The first block preserves the original boards completely (including all commentary). Replicated blocks (2+) contain minimal board data with tracking tags:
- `[VirtualBoard]` - Original board number within the block
- `[VirtualDealer]` - Original dealer for that board position
- `[VirtualVulnerable]` - Original vulnerability for that board position
- `[BlockNumber]` - Which block this board belongs to (1-indexed)

If block_size exceeds the number of input boards, filler deals are used (each player gets all 13 cards of one suit).

#### Examples

Replicate 8 boards into 4 blocks (32 total boards):
```bash
bridge-wrangler block-replicate -i session.pbn
# With 8 input boards: creates 4 blocks of 8 = 32 boards
# Output: session - 8x4.pbn
```

Create a specific number of blocks:
```bash
bridge-wrangler block-replicate -i hands.pbn -c 6
# Creates 6 blocks
```

Create blocks with a specific size:
```bash
bridge-wrangler block-replicate -i hands.pbn -b 9 -c 4
# Creates 4 blocks of 9 boards = 36 total
```

Specify output file:
```bash
bridge-wrangler block-replicate -i deals.pbn -o tournament.pbn
```

Generate PBN and PDF for dealing machines:
```bash
bridge-wrangler block-replicate -i lesson.pbn --pdf
# Creates: lesson - 4x9.pbn and lesson - 4x9.pdf
```

### filter

Filter boards by regex pattern. Separates boards into matched and/or not-matched output files. Boards are renumbered sequentially by default. Based on the Bridge Composer Filter.js plugin.

```bash
bridge-wrangler filter --input <FILE> --pattern <REGEX> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--pattern <REGEX>` | `-p` | Regex pattern to match (required) | - |
| `--matched <FILE>` | `-m` | Output file for matched boards | `<input>-Matched.pbn` |
| `--not-matched <FILE>` | `-n` | Output file for non-matched boards | - |
| `--case-sensitive` | | Use case-sensitive matching | off (case-insensitive) |
| `--renumber` | | Renumber boards sequentially (1, 2, 3, ...) | on |
| `--pdf` | | Also generate PDFs of the output files | off |

If neither `-m` nor `-n` is specified, matched boards are written to the default file. You can specify both to get separate files for matched and not-matched boards.

The pattern is matched against each board's entire content (all tags and commentary). Matching is case-insensitive by default. Output files receive the original file's header comments.

#### Examples

Filter boards with notrump contracts:
```bash
bridge-wrangler filter -i hands.pbn -p "NT"
# Creates: hands-Matched.pbn with only NT contract boards
```

Separate matched and not-matched boards:
```bash
bridge-wrangler filter -i hands.pbn -p "3NT" -m with-3nt.pbn -n without-3nt.pbn
# Creates both output files
```

Filter by vulnerability and generate PDFs:
```bash
bridge-wrangler filter -i deals.pbn -p '\[Vulnerable "None"\]' --pdf -m not-vul.pbn
# Creates: not-vul.pbn and not-vul.pdf
```

Case-sensitive search for specific contract:
```bash
bridge-wrangler filter -i hands.pbn -p '\[Contract "3NT"\]' --case-sensitive
```

### event

Update the Event tag for all boards in a PBN file.

```bash
bridge-wrangler event --input <FILE> --event <NAME> [OPTIONS]
```

#### Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--input <FILE>` | `-i` | Input PBN file (required) | - |
| `--event <NAME>` | `-e` | Event name to set (required) | - |
| `--output <FILE>` | `-o` | Output PBN file | `<input>-Updated.pbn` |
| `--in-place` | | Update the input file directly | off |

#### Examples

Update event name and write to new file:
```bash
bridge-wrangler event -i hands.pbn -e "Club Championship 2024"
# Creates: hands-Updated.pbn
```

Update event name in place:
```bash
bridge-wrangler event -i hands.pbn -e "Weekly Duplicate" --in-place
# Modifies hands.pbn directly
```

Specify output file:
```bash
bridge-wrangler event -i raw.pbn -e "Spring Sectional" -o tournament.pbn
```

## Dependencies

This tool uses:
- [bridge-parsers](https://github.com/bridge-craftwork/Bridge-Parsers) - PBN parsing
- [pbn-to-pdf](https://github.com/bridge-craftwork/pbn-to-pdf) - PDF generation
- [bridge-solver](https://github.com/bridge-craftwork/Dealer3) - Double-dummy analysis

## License

Unlicense
