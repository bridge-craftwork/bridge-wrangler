use anyhow::{anyhow, Context, Result};
use bridge_encodings::pbn::{dd_table_from_pbn, dd_table_to_pbn, read_pbn};
use bridge_types::{Board, DdTable, Direction, Vulnerability};
use clap::{Args as ClapArgs, ValueEnum};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum RotationBasis {
    /// Standard basis: RotationBasis tag, Student, Declarer, Dealer, Deal (in priority order)
    #[default]
    Standard,
    /// Use the RotationBasis PBN tag
    BasisTag,
    /// Use the Student tag as rotation basis
    Student,
    /// Use the Declarer tag as rotation basis
    Declarer,
    /// Use the Dealer tag as rotation basis
    Dealer,
    /// Use the Deal's first character (starting seat) as rotation basis
    Deal,
    /// Assume all boards are oriented to North
    North,
    /// Assume all boards are oriented to South
    South,
    /// Assume all boards are oriented to East
    East,
    /// Assume all boards are oriented to West
    West,
}

#[derive(ClapArgs)]
pub struct Args {
    /// Input PBN file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output PBN file (defaults to input with pattern appended).
    /// Not used when multiple patterns are specified.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Rotation pattern(s) - sequence of directions for dealer.
    /// Use comma to specify multiple patterns (e.g., "S,NS,NESW") to generate multiple files.
    #[arg(short, long, default_value = "NESW")]
    pub pattern: String,

    /// Basis for determining current board orientation
    #[arg(short, long, value_enum, default_value = "standard")]
    pub basis: RotationBasis,

    /// Use standard vulnerability based on board number instead of rotating
    #[arg(long)]
    pub standard_vul: bool,
}

/// Information about the rotation applied to a board
#[derive(Debug, Clone)]
struct RotationInfo {
    rotation: u8,
    target: Direction,
    basis: Direction,
    basis_kind: String,
    use_standard_vul: bool,
}

impl RotationInfo {
    fn to_rotation_note(&self, board_num: u32) -> String {
        format!(
            "[RotationNote \"Board {}, chOption: {}, chBasis: {}, basisKind:{}, nOption:{}, nBasis: {}, nRot: {}, useStandardVul: {}\"]",
            board_num,
            self.target.to_char(),
            self.basis.to_char(),
            self.basis_kind,
            direction_to_index(self.target),
            direction_to_index(self.basis),
            self.rotation,
            self.use_standard_vul
        )
    }
}

fn direction_to_index(dir: Direction) -> u8 {
    match dir {
        Direction::North => 0,
        Direction::East => 1,
        Direction::South => 2,
        Direction::West => 3,
    }
}

pub fn run(args: Args) -> Result<()> {
    // Split patterns by comma for multi-pattern support
    let patterns: Vec<&str> = args.pattern.split(',').map(|s| s.trim()).collect();

    // Validate that --output is not used with multiple patterns
    if patterns.len() > 1 && args.output.is_some() {
        return Err(anyhow!(
            "Cannot use --output with multiple patterns. Output files will be auto-named."
        ));
    }

    // Read input file once
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    // Parse extra tags that bridge-parsers doesn't handle
    let extra_tags = parse_extra_tags(&content);

    // Validate boards once
    let boards = read_pbn(&content).context("Failed to parse PBN file")?;
    let valid_board_count = boards.iter().filter(|b| board_has_valid_deal(b)).count();

    if valid_board_count == 0 {
        return Err(anyhow!("No valid boards found in input file"));
    }

    println!(
        "Read {} boards from {}",
        valid_board_count,
        args.input.display()
    );

    // Process each pattern
    for pattern_str in &patterns {
        let pattern = parse_pattern(pattern_str)?;

        // Clone boards for this pattern
        let mut rotated_boards = boards.clone();
        rotated_boards.retain(board_has_valid_deal);

        // Track rotation info per board
        let mut rotation_infos: HashMap<u32, RotationInfo> = HashMap::new();

        // Rotate each board
        for (i, board) in rotated_boards.iter_mut().enumerate() {
            // Assign board number if missing
            if board.number.is_none() {
                board.number = Some((i + 1) as u32);
            }

            let board_num = board.number.unwrap();

            // Get target direction from pattern (cycling through)
            let target = pattern[i % pattern.len()];

            // Find current basis direction and kind
            let board_tags = extra_tags.get(&board_num);
            let (basis_dir, basis_kind) = find_basis(board, board_tags, args.basis);

            // Calculate rotation amount (0-3)
            let rotation = rotation_amount(basis_dir, target);

            rotation_infos.insert(
                board_num,
                RotationInfo {
                    rotation,
                    target,
                    basis: basis_dir,
                    basis_kind: basis_kind.to_string(),
                    use_standard_vul: args.standard_vul,
                },
            );

            if rotation != 0 {
                rotate_board(board, rotation, args.standard_vul);
            }
        }

        // Determine output path
        let output_path = if patterns.len() == 1 {
            args.output
                .clone()
                .unwrap_or_else(|| make_output_path(&args.input, pattern_str))
        } else {
            make_output_path(&args.input, pattern_str)
        };

        // Write output using our custom writer that handles extra tags
        let output_content =
            write_rotated_pbn(&content, &extra_tags, &rotated_boards, &rotation_infos)?;
        std::fs::write(&output_path, output_content)
            .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;

        println!(
            "Wrote {} boards to {}",
            rotated_boards.len(),
            output_path.display()
        );
    }

    Ok(())
}

fn make_output_path(input: &Path, pattern: &str) -> PathBuf {
    let stem = input.file_stem().unwrap_or_default().to_string_lossy();
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "pbn".to_string());
    input.with_file_name(format!("{} - {}.{}", stem, pattern.to_uppercase(), ext))
}

/// Check if a board has a valid deal (not empty or placeholder)
fn board_has_valid_deal(board: &Board) -> bool {
    board.deal.has_cards()
}

/// Parse extra tags from PBN content that bridge-parsers doesn't handle
/// Returns a map of board_number -> tag_name -> tag_value
fn parse_extra_tags(content: &str) -> HashMap<u32, HashMap<String, String>> {
    let mut result: HashMap<u32, HashMap<String, String>> = HashMap::new();
    let mut current_board: Option<u32> = None;

    for line in content.lines() {
        let line = line.trim();
        if !line.starts_with('[') || !line.ends_with(']') {
            continue;
        }

        if let Some(tag) = parse_tag_line(line) {
            if tag.0 == "Board" {
                if let Ok(num) = tag.1.parse::<u32>() {
                    current_board = Some(num);
                }
            } else if let Some(board_num) = current_board {
                result
                    .entry(board_num)
                    .or_default()
                    .insert(tag.0.to_string(), tag.1.to_string());
            }
        }
    }

    result
}

fn parse_tag_line(line: &str) -> Option<(&str, &str)> {
    let line = line.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts = line.splitn(2, ' ');
    let name = parts.next()?;
    let value = parts.next()?.trim();
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    Some((name, value))
}

/// Parse a pattern string like "NESW" or "NS" into a vec of Directions
fn parse_pattern(pattern: &str) -> Result<Vec<Direction>> {
    let pattern = pattern.to_uppercase();
    let mut directions = Vec::new();

    for c in pattern.chars() {
        let dir = Direction::from_char(c)
            .ok_or_else(|| anyhow!("Invalid direction '{}' in pattern", c))?;
        directions.push(dir);
    }

    if directions.is_empty() {
        return Err(anyhow!("Pattern cannot be empty"));
    }

    Ok(directions)
}

/// Find the basis direction for a board based on the rotation basis setting
fn find_basis(
    board: &Board,
    tags: Option<&HashMap<String, String>>,
    basis: RotationBasis,
) -> (Direction, &'static str) {
    let get_tag_direction = |tag_name: &str| -> Option<Direction> {
        tags.and_then(|t| t.get(tag_name))
            .and_then(|s| s.chars().next())
            .and_then(Direction::from_char)
    };

    match basis {
        RotationBasis::Standard => {
            // Priority: RotationBasis > Student > Declarer > Dealer > Deal > North
            if let Some(dir) = get_tag_direction("RotationBasis") {
                (dir, "RotationBasis")
            } else if let Some(dir) = get_tag_direction("Student") {
                (dir, "Student")
            } else if let Some(dir) = get_tag_direction("Declarer") {
                (dir, "Declarer")
            } else if let Some(dir) = board.dealer {
                (dir, "Dealer")
            } else {
                (Direction::North, "North")
            }
        }
        RotationBasis::BasisTag => (
            get_tag_direction("RotationBasis").unwrap_or(Direction::North),
            "RotationBasis",
        ),
        RotationBasis::Student => (
            get_tag_direction("Student").unwrap_or(Direction::North),
            "Student",
        ),
        RotationBasis::Declarer => (
            get_tag_direction("Declarer").unwrap_or(Direction::North),
            "Declarer",
        ),
        RotationBasis::Dealer => (board.dealer.unwrap_or(Direction::North), "Dealer"),
        RotationBasis::Deal => {
            // The deal's first character indicates which hand is listed first
            // For now, use dealer as fallback
            (board.dealer.unwrap_or(Direction::North), "Deal")
        }
        RotationBasis::North => (Direction::North, "North"),
        RotationBasis::South => (Direction::South, "South"),
        RotationBasis::East => (Direction::East, "East"),
        RotationBasis::West => (Direction::West, "West"),
    }
}

/// Calculate how many positions to rotate clockwise (0-3)
fn rotation_amount(from: Direction, to: Direction) -> u8 {
    let from_idx = direction_index(from);
    let to_idx = direction_index(to);
    ((to_idx + 4 - from_idx) % 4) as u8
}

fn direction_index(dir: Direction) -> usize {
    match dir {
        Direction::North => 0,
        Direction::East => 1,
        Direction::South => 2,
        Direction::West => 3,
    }
}

fn rotate_direction(dir: Direction, rotation: u8) -> Direction {
    let idx = direction_index(dir);
    let new_idx = (idx + rotation as usize) % 4;
    match new_idx {
        0 => Direction::North,
        1 => Direction::East,
        2 => Direction::South,
        3 => Direction::West,
        _ => unreachable!(),
    }
}

/// Rotate a board by the given amount (0-3 positions clockwise)
fn rotate_board(board: &mut Board, rotation: u8, use_standard_vul: bool) {
    // Rotate dealer
    if let Some(dealer) = board.dealer {
        board.dealer = Some(rotate_direction(dealer, rotation));
    }

    // Rotate vulnerability
    if use_standard_vul {
        if let Some(num) = board.number {
            board.vulnerable = Vulnerability::from_board_number(num);
        }
    } else {
        // For odd rotations, swap NS and EW vulnerability
        if rotation % 2 == 1 {
            board.vulnerable = match board.vulnerable {
                Vulnerability::NorthSouth => Vulnerability::EastWest,
                Vulnerability::EastWest => Vulnerability::NorthSouth,
                other => other,
            };
        }
    }

    // Rotate the deal (swap hands around the table)
    let old_deal = board.deal.clone();
    for dir in Direction::ALL {
        let source_dir = rotate_direction(dir, 4 - rotation);
        let hand = old_deal.hand(source_dir).clone();
        board.deal.set_hand(dir, hand);
    }
}

/// Rotate a direction character (N, E, S, W)
fn rotate_direction_char(c: char, rotation: u8) -> char {
    if let Some(dir) = Direction::from_char(c) {
        let rotated = rotate_direction(dir, rotation);
        if c.is_uppercase() {
            rotated.to_char()
        } else {
            rotated.to_char().to_ascii_lowercase()
        }
    } else {
        c
    }
}

/// Rotate a direction string value (single char like "N" or "E")
fn rotate_direction_value(value: &str, rotation: u8) -> String {
    if value.len() == 1 {
        let c = value.chars().next().unwrap();
        rotate_direction_char(c, rotation).to_string()
    } else {
        value.to_string()
    }
}

/// Rotate a Score tag value (e.g., "NS 420" -> "EW 420" for odd rotations)
fn rotate_score_value(value: &str, rotation: u8) -> String {
    if rotation.is_multiple_of(2) {
        return value.to_string();
    }

    if let Some(rest) = value.strip_prefix("NS") {
        format!("EW{}", rest)
    } else if let Some(rest) = value.strip_prefix("EW") {
        format!("NS{}", rest)
    } else {
        value.to_string()
    }
}

/// Rotate a `DoubleDummyTricks` value: the same table an `OptimumResultTable`
/// holds, written as twenty characters in a fixed declarer-and-strain order.
///
/// Because the seats are positions rather than labels here, rotating means
/// moving each cell to the seat the hand went to, then writing the table out
/// again. Decoding and re-encoding through `bridge_encodings` keeps this one
/// definition of the order; reshuffling the characters by hand would make a
/// second.
///
/// A value that does not decode is left as it stands: a rotation is not the
/// place to reject a file's other contents.
fn rotate_double_dummy_tricks(value: &str, rotation: u8) -> String {
    if rotation == 0 {
        return value.to_string();
    }

    let Ok(table) = dd_table_from_pbn(value) else {
        return value.to_string();
    };

    let mut rotated = DdTable::new();
    for (declarer, strain, tricks) in table.cells() {
        rotated.set(rotate_direction(declarer, rotation), strain, tricks);
    }

    dd_table_to_pbn(&rotated)
}

/// Rotate a `ParContract` value, such as `NS 4H+1`, `N 3N=` or
/// `EW 2SX-1; EW 3CX-1`.
///
/// Each contract names who declares it before the contract itself: a side when
/// either partner can take the tricks, a single seat when only one can. Sides
/// swap on an odd rotation, as a `Score` does; a seat moves with its hand. The
/// contract itself does not change, only who plays it.
fn rotate_par_contract(value: &str, rotation: u8) -> String {
    if rotation == 0 {
        return value.to_string();
    }

    value
        .split("; ")
        .map(|contract| {
            let Some((declarer, rest)) = contract.split_once(' ') else {
                return contract.to_string();
            };
            let moved = match declarer {
                "NS" | "EW" => rotate_score_value(declarer, rotation),
                seat if seat.len() == 1 => rotate_direction_value(seat, rotation),
                _ => return contract.to_string(),
            };
            format!("{} {}", moved, rest)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The index of the `Declarer` column in a table section's header value, or
/// `None` if the section does not name one.
///
/// The header lists its columns separated by `;`, each optionally carrying a
/// format specifier after a `\` — `Declarer;Denomination\2R;Result\1R` (PBN 2.1
/// §3.7). Only the column name matters here.
fn declarer_column(header: &str) -> Option<usize> {
    header
        .split(';')
        .position(|column| column.split('\\').next().unwrap_or("").trim() == "Declarer")
}

/// Rotate the `Declarer` column of a table section's data rows.
///
/// The rows of an `[OptimumResultTable]` say what each seat can make. Rotating
/// the board moves the hands around the table, so a row that described North
/// now describes whichever seat those cards landed in, and the labels have to
/// move with them.
///
/// The file's own row order is kept: rows are relabelled and then regrouped so
/// the groups still appear in the order the source listed them, which for an
/// `analyze` table is N, S, E, W. Relabelling in place would leave a file whose
/// groups run in a rotated order instead.
///
/// Rows are returned unchanged if any of them is missing the declarer column or
/// has something other than a direction in it, since a partial rotation would
/// be worse than none.
fn rotate_dd_rows(rows: &[String], declarer_col: usize, rotation: u8) -> Vec<String> {
    if rotation == 0 {
        return rows.to_vec();
    }

    // Relabel each row, keeping its text and spacing otherwise intact.
    let mut relabelled: Vec<(char, String)> = Vec::with_capacity(rows.len());
    for row in rows {
        let Some((offset, field)) = row_field(row, declarer_col) else {
            return rows.to_vec();
        };
        let Some(dir) = (field.len() == 1)
            .then(|| field.chars().next())
            .flatten()
            .and_then(Direction::from_char)
        else {
            return rows.to_vec();
        };
        let new_label = rotate_direction(dir, rotation).to_char();
        let mut rotated = row.clone();
        rotated.replace_range(offset..offset + field.len(), &new_label.to_string());
        relabelled.push((new_label, rotated));
    }

    // The order the source listed its declarers in, to be kept.
    let mut label_order: Vec<char> = Vec::new();
    for row in rows {
        if let Some((_, field)) = row_field(row, declarer_col) {
            if let Some(c) = field.chars().next() {
                if !label_order.contains(&c) {
                    label_order.push(c);
                }
            }
        }
    }

    let mut out = Vec::with_capacity(relabelled.len());
    for label in &label_order {
        for (row_label, text) in &relabelled {
            if row_label == label {
                out.push(text.clone());
            }
        }
    }

    // Any row whose new label was not among the originals (a table listing only
    // some seats) still has to be written, after the groups that were ordered.
    for (row_label, text) in &relabelled {
        if !label_order.contains(row_label) {
            out.push(text.clone());
        }
    }

    out
}

/// One whitespace-separated field of a table row, with its byte offset, so a
/// field can be replaced without disturbing the row's spacing.
fn row_field(row: &str, index: usize) -> Option<(usize, &str)> {
    let mut fields = Vec::new();
    let mut start = None;
    for (offset, c) in row.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(offset),
            (true, Some(from)) => {
                fields.push((from, &row[from..offset]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        fields.push((from, &row[from..]));
    }
    fields.get(index).copied()
}

/// Rotate direction words in commentary text
fn rotate_commentary(text: &str, rotation: u8) -> String {
    if rotation == 0 {
        return text.to_string();
    }

    let directions = ["North", "East", "South", "West"];

    // Create regex patterns with word boundaries for each direction (case-insensitive)
    let patterns: Vec<Regex> = directions
        .iter()
        .map(|d| Regex::new(&format!(r"(?i)\b{}\b", d)).unwrap())
        .collect();

    // Create temporary placeholders to avoid double-replacement
    let mut result = text.to_string();

    // First pass: replace with placeholders, preserving case
    for (i, pattern) in patterns.iter().enumerate() {
        result = pattern
            .replace_all(&result, |caps: &regex::Captures| {
                let matched = &caps[0];
                if matched.chars().next().unwrap().is_uppercase() {
                    if matched.chars().all(|c| c.is_uppercase()) {
                        format!("__DIR_UPPER_{}__", i)
                    } else {
                        format!("__DIR_TITLE_{}__", i)
                    }
                } else {
                    format!("__DIR_LOWER_{}__", i)
                }
            })
            .to_string();
    }

    // Second pass: replace placeholders with rotated directions
    for (i, _) in directions.iter().enumerate() {
        let new_idx = (i + rotation as usize) % 4;
        let new_title = directions[new_idx];
        let new_lower = new_title.to_lowercase();
        let new_upper = new_title.to_uppercase();
        result = result
            .replace(&format!("__DIR_TITLE_{}__", i), new_title)
            .replace(&format!("__DIR_LOWER_{}__", i), &new_lower)
            .replace(&format!("__DIR_UPPER_{}__", i), &new_upper);
    }

    result
}

/// Write rotated PBN content, preserving original structure and rotating additional tags
fn write_rotated_pbn(
    original_content: &str,
    _extra_tags: &HashMap<u32, HashMap<String, String>>,
    rotated_boards: &[Board],
    rotation_infos: &HashMap<u32, RotationInfo>,
) -> Result<String> {
    let mut output = String::new();
    let mut current_board_num: Option<u32> = None;
    let mut board_index = 0;
    let mut in_commentary = false;
    let mut commentary_buffer = String::new();
    let mut current_rotation: u8 = 0;
    let mut is_first_board = true;
    let mut in_header = true;

    // Extract Event title from the first [Event] tag in the file
    let event_title = original_content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if let Some((tag, value)) = parse_tag_line(trimmed) {
                if tag == "Event" && !value.is_empty() {
                    return Some(value.to_string());
                }
            }
            None
        })
        .unwrap_or_default();

    // The data rows of an [OptimumResultTable], held until the section ends so
    // the whole table can be rotated at once: declarer column, the board's
    // rotation, and the rows as read.
    let mut dd_section: Option<(usize, u8, Vec<String>)> = None;

    for line in original_content.lines() {
        let trimmed = line.trim();

        // A table section's data rows run until the next tag, commentary,
        // directive or blank line.
        if dd_section.is_some() {
            let is_row = !trimmed.is_empty()
                && !trimmed.starts_with('[')
                && !trimmed.starts_with('{')
                && !trimmed.starts_with('%')
                && !trimmed.starts_with(';');
            if is_row {
                if let Some((_, _, rows)) = dd_section.as_mut() {
                    rows.push(line.to_string());
                }
                continue;
            }
            flush_dd_section(&mut output, &mut dd_section);
        }

        // Track commentary blocks
        if trimmed.starts_with('{') && !trimmed.ends_with('}') {
            in_commentary = true;
            commentary_buffer.clear();
            commentary_buffer.push_str(line);
            commentary_buffer.push('\n');
            continue;
        }

        if in_commentary {
            commentary_buffer.push_str(line);
            commentary_buffer.push('\n');
            if trimmed.ends_with('}') {
                in_commentary = false;
                // Rotate commentary and output
                if current_board_num.is_some() && current_rotation != 0 {
                    let rotated = rotate_commentary(&commentary_buffer, current_rotation);
                    output.push_str(&rotated);
                } else {
                    output.push_str(&commentary_buffer);
                }
            }
            continue;
        }

        // Handle single-line commentary
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            if current_board_num.is_some() && current_rotation != 0 {
                let rotated = rotate_commentary(line, current_rotation);
                output.push_str(&rotated);
                output.push('\n');
            } else {
                output.push_str(line);
                output.push('\n');
            }
            continue;
        }

        // Skip directives and comments (preserve them)
        if trimmed.starts_with('%') || trimmed.starts_with(';') {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        // Handle empty lines
        if trimmed.is_empty() {
            output.push_str(line);
            output.push('\n');
            continue;
        }

        // Handle tag lines
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if let Some((tag_name, tag_value)) = parse_tag_line(trimmed) {
                // Check if this is a Board tag
                if tag_name == "Board" {
                    in_header = false;
                    if let Ok(num) = tag_value.parse::<u32>() {
                        // Check if this board is in our rotated set
                        if rotated_boards.iter().any(|b| b.number == Some(num)) {
                            current_board_num = Some(num);
                            let info = rotation_infos.get(&num);
                            current_rotation = info.map(|i| i.rotation).unwrap_or(0);
                            board_index = rotated_boards
                                .iter()
                                .position(|b| b.number == Some(num))
                                .unwrap_or(0);

                            // Output Event/Site/Date before Board
                            if is_first_board {
                                output.push_str(&format!("[Event \"{}\"]\n", event_title));
                                is_first_board = false;
                            } else {
                                output.push_str("[Event \"\"]\n");
                            }
                            output.push_str("[Site \"\"]\n");
                            output.push_str("[Date \"\"]\n");
                        } else {
                            // Skip this board entirely
                            current_board_num = None;
                            current_rotation = 0;
                            continue;
                        }
                    }
                    output.push_str(line);
                    output.push('\n');
                    continue;
                }

                // Skip Event/Site/Date tags from original (we output them ourselves)
                if matches!(tag_name, "Event" | "Site" | "Date") {
                    if in_header {
                        // Keep header tags (before first board)
                        // Actually, we skip these too since we'll output them before Board
                    }
                    continue;
                }

                // If no current board, skip
                if current_board_num.is_none() {
                    continue;
                }

                let board = &rotated_boards[board_index];
                let rotation = current_rotation;

                // Handle tags that need rotation
                let new_line = match tag_name {
                    "Dealer" => {
                        format!(
                            "[Dealer \"{}\"]",
                            board.dealer.map(|d| d.to_char()).unwrap_or('N')
                        )
                    }
                    "Vulnerable" => {
                        format!("[Vulnerable \"{}\"]", board.vulnerable.to_pbn())
                    }
                    "Deal" => {
                        let first_dir = board.dealer.unwrap_or(Direction::North);
                        format!("[Deal \"{}\"]", board.deal.to_pbn(first_dir))
                    }
                    "Auction" | "Play" => {
                        // Rotate the direction value
                        let rotated_value = rotate_direction_value(tag_value, rotation);
                        format!("[{} \"{}\"]", tag_name, rotated_value)
                    }
                    "Declarer" => {
                        let rotated_value = rotate_direction_value(tag_value, rotation);
                        format!("[Declarer \"{}\"]", rotated_value)
                    }
                    "Score" => {
                        let rotated_value = rotate_score_value(tag_value, rotation);
                        format!("[Score \"{}\"]", rotated_value)
                    }
                    "DoubleDummyTricks" => {
                        let rotated_value = rotate_double_dummy_tricks(tag_value, rotation);
                        format!("[DoubleDummyTricks \"{}\"]", rotated_value)
                    }
                    "ParContract" => {
                        let rotated_value = rotate_par_contract(tag_value, rotation);
                        format!("[ParContract \"{}\"]", rotated_value)
                    }
                    "OptimumScore" => {
                        // An NS/EW value, like Score.
                        let rotated_value = rotate_score_value(tag_value, rotation);
                        format!("[OptimumScore \"{}\"]", rotated_value)
                    }
                    "OptimumResultTable" => {
                        // The header is unchanged; its rows are gathered and
                        // rotated together once the section ends.
                        if let Some(column) = declarer_column(tag_value) {
                            dd_section = Some((column, rotation, Vec::new()));
                        }
                        line.to_string()
                    }
                    "BCFlags" => {
                        // Output BCFlags, then add RotationNote
                        output.push_str(line);
                        output.push('\n');
                        if let Some(info) = rotation_infos.get(&current_board_num.unwrap()) {
                            output.push_str(&info.to_rotation_note(current_board_num.unwrap()));
                            output.push('\n');
                        }
                        continue;
                    }
                    _ => line.to_string(),
                };

                output.push_str(&new_line);
                output.push('\n');
            } else {
                output.push_str(line);
                output.push('\n');
            }
        } else {
            // Non-tag lines (auction data, play data, etc.)
            output.push_str(line);
            output.push('\n');
        }
    }

    // A table that ran to the end of the file without a blank line after it.
    flush_dd_section(&mut output, &mut dd_section);

    Ok(output)
}

/// Write out a gathered `[OptimumResultTable]`, rotated, and clear it.
fn flush_dd_section(output: &mut String, section: &mut Option<(usize, u8, Vec<String>)>) {
    if let Some((column, rotation, rows)) = section.take() {
        for row in rotate_dd_rows(&rows, column, rotation) {
            output.push_str(&row);
            output.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_types::Strain;

    #[test]
    fn test_parse_pattern() {
        let p = parse_pattern("NESW").unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], Direction::North);
        assert_eq!(p[1], Direction::East);
        assert_eq!(p[2], Direction::South);
        assert_eq!(p[3], Direction::West);

        let p = parse_pattern("ns").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], Direction::North);
        assert_eq!(p[1], Direction::South);
    }

    #[test]
    fn test_rotation_amount() {
        assert_eq!(rotation_amount(Direction::North, Direction::North), 0);
        assert_eq!(rotation_amount(Direction::North, Direction::East), 1);
        assert_eq!(rotation_amount(Direction::North, Direction::South), 2);
        assert_eq!(rotation_amount(Direction::North, Direction::West), 3);

        assert_eq!(rotation_amount(Direction::East, Direction::North), 3);
        assert_eq!(rotation_amount(Direction::South, Direction::North), 2);
    }

    #[test]
    fn test_rotate_direction() {
        assert_eq!(rotate_direction(Direction::North, 0), Direction::North);
        assert_eq!(rotate_direction(Direction::North, 1), Direction::East);
        assert_eq!(rotate_direction(Direction::North, 2), Direction::South);
        assert_eq!(rotate_direction(Direction::North, 3), Direction::West);
        assert_eq!(rotate_direction(Direction::East, 1), Direction::South);
    }

    #[test]
    fn test_rotate_score_value() {
        assert_eq!(rotate_score_value("NS 420", 0), "NS 420");
        assert_eq!(rotate_score_value("NS 420", 1), "EW 420");
        assert_eq!(rotate_score_value("NS 420", 2), "NS 420");
        assert_eq!(rotate_score_value("EW -100", 1), "NS -100");
    }

    #[test]
    fn a_double_dummy_tricks_tag_follows_the_hands_too() {
        // The same table as an OptimumResultTable, positionally encoded, so it
        // has to move exactly as the section's rows do.
        let table = DdTable::from_fn(|declarer, strain| match (declarer, strain) {
            (Direction::North, Strain::NoTrump) => 10,
            (Direction::North, _) => 9,
            _ => 4,
        });
        let encoded = dd_table_to_pbn(&table);

        let rotated = dd_table_from_pbn(&rotate_double_dummy_tricks(&encoded, 1)).unwrap();
        assert_eq!(rotated.tricks(Direction::East, Strain::NoTrump), 10);
        assert_eq!(rotated.tricks(Direction::North, Strain::NoTrump), 4);

        // A full turn is the identity, and the two encodings agree.
        assert_eq!(rotate_double_dummy_tricks(&encoded, 0), encoded);
        let twice = rotate_double_dummy_tricks(&rotate_double_dummy_tricks(&encoded, 1), 3);
        assert_eq!(twice, encoded);
    }

    #[test]
    fn a_double_dummy_tricks_tag_and_its_table_stay_in_step() {
        // The invariant that matters: whatever the section says a seat makes,
        // the packed tag says the same after the same rotation.
        let table = DdTable::from_fn(|declarer, strain| match (declarer, strain) {
            (Direction::North, Strain::NoTrump) => 11,
            (Direction::South, Strain::NoTrump) => 11,
            (Direction::West, Strain::Spades) => 7,
            _ => 5,
        });
        let rows: Vec<String> = table
            .cells()
            .map(|(declarer, strain, tricks)| {
                format!("{} {} {}", declarer.to_char(), strain.to_char(), tricks)
            })
            .collect();

        for rotation in 1..=3 {
            let packed = dd_table_from_pbn(&rotate_double_dummy_tricks(
                &dd_table_to_pbn(&table),
                rotation,
            ))
            .unwrap();
            for row in rotate_dd_rows(&rows, 0, rotation) {
                let fields: Vec<&str> = row.split_whitespace().collect();
                let seat = Direction::from_char(fields[0].chars().next().unwrap()).unwrap();
                let strain = Strain::from_str(fields[1]).unwrap();
                assert_eq!(
                    packed.tricks(seat, strain),
                    fields[2].parse::<u8>().unwrap(),
                    "rotation {rotation}, row {row}"
                );
            }
        }
    }

    #[test]
    fn a_par_contract_names_who_plays_it_after_the_rotation() {
        // A side swaps on an odd rotation, as a Score does.
        assert_eq!(rotate_par_contract("NS 4H+1", 1), "EW 4H+1");
        assert_eq!(rotate_par_contract("NS 4H+1", 2), "NS 4H+1");
        assert_eq!(rotate_par_contract("EW 2SX-1", 3), "NS 2SX-1");

        // A single seat moves with its hand.
        assert_eq!(rotate_par_contract("N 3N=", 1), "E 3N=");
        assert_eq!(rotate_par_contract("W 3N=", 2), "E 3N=");

        // Every contract of a tie is rotated, and a passed-out par still reads.
        assert_eq!(
            rotate_par_contract("EW 2SX-1; EW 3CX-1", 1),
            "NS 2SX-1; NS 3CX-1"
        );
        assert_eq!(rotate_par_contract("NS Pass", 1), "EW Pass");
        assert_eq!(rotate_par_contract("NS 4H+1", 0), "NS 4H+1");
    }

    #[test]
    fn declarer_column_reads_the_header() {
        assert_eq!(
            declarer_column("Declarer;Denomination\\2R;Result\\1R"),
            Some(0)
        );
        assert_eq!(
            declarer_column("Denomination\\2R;Declarer;Result\\2R"),
            Some(1)
        );
        assert_eq!(declarer_column("Denomination;Result"), None);
    }

    /// A table as `analyze` writes it: five denominations per declarer, in the
    /// order N, S, E, W.
    fn dd_table(n: u8, s: u8, e: u8, w: u8) -> Vec<String> {
        ['N', 'S', 'E', 'W']
            .iter()
            .zip([n, s, e, w])
            .flat_map(|(seat, tricks)| {
                ["NT", " S", " H", " D", " C"]
                    .iter()
                    .map(move |denom| format!("{} {} {:2}", seat, denom, tricks))
            })
            .collect()
    }

    #[test]
    fn a_dd_table_follows_the_hands_around_the_table() {
        // North's cards move to East on a rotation of 1, so the tricks North
        // could take are now East's.
        let rotated = rotate_dd_rows(&dd_table(7, 7, 6, 6), 0, 1);
        assert_eq!(rotated, dd_table(6, 6, 7, 7));

        // And the row order is the file's own, still N, S, E, W.
        let seats: Vec<&str> = rotated.iter().map(|row| &row[0..1]).collect();
        assert_eq!(seats[0], "N");
        assert_eq!(seats[5], "S");
        assert_eq!(seats[10], "E");
        assert_eq!(seats[15], "W");
    }

    #[test]
    fn a_half_turn_swaps_the_partners_and_a_full_turn_changes_nothing() {
        assert_eq!(
            rotate_dd_rows(&dd_table(7, 5, 6, 4), 0, 2),
            dd_table(5, 7, 4, 6)
        );
        assert_eq!(
            rotate_dd_rows(&dd_table(7, 5, 6, 4), 0, 0),
            dd_table(7, 5, 6, 4)
        );
    }

    #[test]
    fn a_dd_row_keeps_its_spacing_and_its_other_columns() {
        let rows = vec!["N NT 10".to_string(), "N  S  9".to_string()];
        let rotated = rotate_dd_rows(&rows, 0, 1);
        assert_eq!(rotated, vec!["E NT 10".to_string(), "E  S  9".to_string()]);
    }

    #[test]
    fn rows_that_do_not_name_a_seat_are_left_alone() {
        let rows = vec!["N NT 10".to_string(), "? NT 10".to_string()];
        assert_eq!(rotate_dd_rows(&rows, 0, 1), rows);
    }

    #[test]
    fn test_rotate_commentary() {
        assert_eq!(rotate_commentary("North leads", 2), "South leads");
        assert_eq!(rotate_commentary("East and West", 1), "South and North");
    }
}
