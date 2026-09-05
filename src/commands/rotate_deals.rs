use anyhow::{anyhow, Context, Result};
use bridge_encodings::pbn::read_pbn;
use bridge_types::{Board, Direction, Vulnerability};
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

    for line in original_content.lines() {
        let trimmed = line.trim();

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

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_rotate_commentary() {
        assert_eq!(rotate_commentary("North leads", 2), "South leads");
        assert_eq!(rotate_commentary("East and West", 1), "South and North");
    }
}
