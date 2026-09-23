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

#### What rotates with the hands

Moving the hands around the table moves everything that names a seat with them:
`Dealer`, `Vulnerable`, `Deal`, `Auction`, `Play`, `Declarer`, the side in a
`Score` or `OptimumScore`, the direction words in `{...}` commentary, and all
four of a board's double-dummy tags — the `Declarer` column of an
`[OptimumResultTable]`, the positionally encoded `[DoubleDummyTricks]`, and the
seat or side declaring each contract in a `[ParContract]`.

A double-dummy table says what each seat can make, so an unrotated one would
describe seats that no longer hold those cards. Rotating an annotated file and
re-analyzing it give the same answers, and the two encodings of the table stay
in step with each other. The table's rows keep the order the file listed them
in.

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
| `--layout <LAYOUT>` | `-l` | Layout style (see below) | `hand-record` for a file with `%BoardsPerPage 18`, otherwise `analysis` |
| `--board-range <RANGE>` | `-r` | Board range to include | all boards |
| `--circle-sure-winners` | - | Circle sure winners in red (declarer's plan layouts) | off |
| `--circle-promotable-winners` | - | Circle promotable winners in green (declarer's plan layouts) | off |
| `--circle-length-winners` | - | Circle length winners in blue (declarer's plan layouts) | off |
| `--section-commentary` | - | Draw commentary written inside `[Auction]` or `[Play]`, which BridgeComposer leaves out | off |
| `--no-page-furniture` | - | Leave out the event header or headings and the `%PageFooter` lines, for pipelines that add their own | off |

#### Layouts

- `analysis` - Full hand diagram with bidding table and commentary (default)
- `bidding-sheets` - Simplified layout for practice bidding
- `declarers-plan-1up` - Declarer's plan, 1 deal per page (full size)
- `declarers-plan-2up` - Declarer's plan, 2 deals per page (rotated 90°)
- `declarers-plan` - Declarer's plan practice sheets, 4 deals per page
- `dealer-summary` - 6 deals per page summary for the dealer
- `hand-record` - Compact hand record, 18 boards per page with hands and HCP only (chosen automatically for a file with `%BoardsPerPage 18` when `--layout` is omitted)

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

Each board is written the way Bridge Composer exports LIN, using the LIN writer in [bridge-encodings](https://github.com/bridge-craftwork/bridge-encodings):

```
mn|Event - Date|pn|S,W,N,E|qx|o1,BOARD 1|rh||ah|Board 1|md|3<S>,<W>,<N>|sv|n|
sa|0|mb|p|mb|1C|an|!|mb|p|mb|1H|an|<note text>|...|pg||
pc|D2|pc|D3|pc|DJ|pc|DQ|pg||
pc|CA|pc|C5|pc|CK|pc|C3|pg||
mc|9|pg||
```

- `mn` - Title: `Event - Date`
- `pn` - Player names in South, West, North, East order
- `qx`, `ah` - Board id as written in `[Board]`, so ids such as `1-1` are kept
- `md` - Dealer digit (1=S, 2=W, 3=N, 4=E), then the South, West and North hands
- `sv` - Vulnerability (0=none, n=NS, e=EW, b=both)
- `mb` - Calls (`p`, `d`, `r`, `1C` … `7N`). A `!` or `$1` on a call becomes `an|!|`, `$2` becomes `an|?|`, and a `=n=` note reference becomes the note's text
- `pc` - The play, one line per trick, in the order the cards were played
- `mc` - Tricks taken, from `[Result]`; omitted when there is no result

Line endings are LF. No tournament header (`vg`, `rs`, `pw`, `mp`, `bn`) is written. A `|` in any text becomes `/`, and a comma in a player name is dropped, so neither can break the record.

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

### analyze — removed

Double-dummy analysis lives in
[bridge-solver](https://github.com/bridge-craftwork/bridge-solver), whose CLI
reads and writes the same PBN files through the same `PbnDocument`, and does
more with them:

```bash
bridge-solver -i hands.pbn -o hands-analyzed.pbn   # one file to another
bridge-solver -w -i deals/                         # annotate a tree in place
```

It writes `[OptimumResultTable]`, `[DoubleDummyTricks]`, `[OptimumScore]` and
`[ParContract]`, solves across every core (`-j/--threads`, identical bytes at
any thread count), and by default leaves a board that already carries analysis
exactly as found, so annotating a collection fills in only what is missing.

This tool had its own `analyze` until v0.11.0. It solved one board at a time on
one thread and wrote only the table, which on a 500-deal file was 66 seconds
against bridge-solver's 9. Keeping a second, slower implementation of someone
else's job was the whole argument for removing it; the tables the two produced
were identical.

What stays here is what this tool is for: `rotate-deals` moves a board's
double-dummy tags with the hands they describe, so an annotated file can be
rotated without going stale.

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
