use anyhow::{Context, Result};
use bridge_types::{Direction, Vulnerability};
use clap::Args as ClapArgs;
use pbn_to_pdf::{config::Settings, parser::parse_pbn, render::generate_pdf};
use regex::Regex;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Input PBN file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output PBN file (defaults to input with BxR suffix)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Number of boards per block (defaults to number of boards in input)
    #[arg(short, long)]
    pub block_size: Option<u32>,

    /// Number of blocks to create (defaults to fill 36 boards)
    #[arg(short = 'c', long)]
    pub block_count: Option<u32>,

    /// Also generate a PDF hand record
    #[arg(long)]
    pub pdf: bool,
}

/// Standard vulnerability pattern (repeats every 16 boards)
const STANDARD_VUL: [Vulnerability; 16] = [
    Vulnerability::None,       // 1
    Vulnerability::NorthSouth, // 2
    Vulnerability::EastWest,   // 3
    Vulnerability::Both,       // 4
    Vulnerability::NorthSouth, // 5
    Vulnerability::EastWest,   // 6
    Vulnerability::Both,       // 7
    Vulnerability::None,       // 8
    Vulnerability::EastWest,   // 9
    Vulnerability::Both,       // 10
    Vulnerability::None,       // 11
    Vulnerability::NorthSouth, // 12
    Vulnerability::Both,       // 13
    Vulnerability::None,       // 14
    Vulnerability::NorthSouth, // 15
    Vulnerability::EastWest,   // 16
];

/// Standard dealer pattern (repeats every 4 boards)
const STANDARD_DEALER: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Filler deal where each player gets all cards of one suit
const FILLER_DEAL: &str = "N:AKQJT98765432... .AKQJT98765432.. ..AKQJT98765432. ...AKQJT98765432";

pub fn run(args: Args) -> Result<()> {
    // Read input file
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    // Split content into header and board sections
    let (header, board_sections) = split_pbn_content(&content);

    // Extract deal strings and BCFlags from each board section
    let deal_strings: Vec<String> = board_sections
        .iter()
        .map(|section| extract_tag_value(section, "Deal"))
        .collect();

    let bcflags_strings: Vec<String> = board_sections
        .iter()
        .map(|section| extract_tag_value(section, "BCFlags"))
        .collect();

    let input_board_count = board_sections.len() as u32;
    println!(
        "Read {} boards from {}",
        input_board_count,
        args.input.display()
    );

    if input_board_count == 0 {
        return Err(anyhow::anyhow!("No boards found in input file"));
    }

    // Determine block size (default: number of input boards)
    let block_size = args.block_size.unwrap_or(input_board_count);

    // Determine block count (default: fill 36 boards)
    let block_count = args.block_count.unwrap_or(36 / block_size);

    if block_count == 0 {
        return Err(anyhow::anyhow!("Block count must be at least 1"));
    }

    let total_boards = block_size * block_count;
    println!(
        "Creating {} blocks of {} boards = {} total boards",
        block_count, block_size, total_boards
    );

    // Generate the output content
    let output_content = generate_replicated_pbn(
        &header,
        &board_sections,
        &deal_strings,
        &bcflags_strings,
        block_size,
        block_count,
    );

    // Determine output path
    let output_path = args.output.unwrap_or_else(|| {
        let stem = args.input.file_stem().unwrap_or_default().to_string_lossy();
        let parent = args.input.parent().unwrap_or(std::path::Path::new("."));
        parent.join(format!("{} - {}x{}.pbn", stem, block_size, block_count))
    });

    // Write output
    std::fs::write(&output_path, &output_content)
        .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;

    println!("Wrote {} boards to {}", total_boards, output_path.display());

    // Generate PDF if requested
    if args.pdf {
        let pdf_path = output_path.with_extension("pdf");

        // Parse the generated PBN for PDF rendering
        let pbn_file = parse_pbn(&output_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse PBN for PDF: {:?}", e))?;

        // Use hand-record style settings for dealing machine output
        let settings = Settings::default().with_metadata(&pbn_file.metadata);

        let pdf_bytes = generate_pdf(&pbn_file.boards, &settings)
            .map_err(|e| anyhow::anyhow!("Failed to generate PDF: {:?}", e))?;

        std::fs::write(&pdf_path, &pdf_bytes)
            .with_context(|| format!("Failed to write PDF: {}", pdf_path.display()))?;

        println!("Wrote PDF to {}", pdf_path.display());
    }

    Ok(())
}

/// Generate the replicated PBN content
fn generate_replicated_pbn(
    header: &str,
    board_sections: &[String],
    deal_strings: &[String],
    bcflags_strings: &[String],
    block_size: u32,
    block_count: u32,
) -> String {
    let mut output = String::new();

    // Copy header
    output.push_str(header);

    let input_board_count = board_sections.len() as u32;

    // Generate each board
    for bd in 0..(block_size * block_count) {
        let block_num = bd / block_size;
        let board_in_block = bd % block_size;

        // First block: preserve original boards with commentary (no virtual tags)
        if block_num == 0 && board_in_block < input_board_count {
            // Copy original board content verbatim
            if let Some(section) = board_sections.get(board_in_block as usize) {
                output.push_str(section);
                if !section.ends_with('\n') {
                    output.push('\n');
                }
            }
            continue;
        }

        // Replicated blocks or filler boards: generate with virtual tags
        let board_num = bd + 1; // 1-indexed

        // Standard dealer and vulnerability based on board number
        let dealer = STANDARD_DEALER[(bd % 4) as usize];
        let vulnerable = STANDARD_VUL[(bd % 16) as usize];

        // Virtual board info (original board this replicates)
        let virtual_board = board_in_block + 1;
        let virtual_dealer = STANDARD_DEALER[(board_in_block % 4) as usize];
        let virtual_vul = STANDARD_VUL[(board_in_block % 16) as usize];

        // Get deal from source board section or use filler
        let deal_str = if (board_in_block as usize) < deal_strings.len() {
            deal_strings[board_in_block as usize].clone()
        } else {
            FILLER_DEAL.to_string()
        };

        // Get BCFlags from source board if available
        let bcflags = if (board_in_block as usize) < bcflags_strings.len() {
            bcflags_strings[board_in_block as usize].clone()
        } else {
            String::new()
        };

        // Write board tags (PBN standard order)
        output.push_str("[Event \"\"]\n");
        output.push_str("[Site \"\"]\n");
        output.push_str("[Date \"\"]\n");
        output.push_str(&format!("[Board \"{}\"]\n", board_num));
        output.push_str("[West \"\"]\n");
        output.push_str("[North \"\"]\n");
        output.push_str("[East \"\"]\n");
        output.push_str("[South \"\"]\n");
        output.push_str(&format!("[Dealer \"{}\"]\n", dealer.to_char()));
        output.push_str(&format!("[Vulnerable \"{}\"]\n", vulnerable.to_pbn()));
        output.push_str(&format!("[Deal \"{}\"]\n", deal_str));
        output.push_str("[Scoring \"\"]\n");
        output.push_str("[Declarer \"\"]\n");
        output.push_str("[Contract \"\"]\n");
        output.push_str("[Result \"\"]\n");

        // Add BCFlags if present in original
        if !bcflags.is_empty() {
            output.push_str(&format!("[BCFlags \"{}\"]\n", bcflags));
        }

        // Add virtual board tags for tracking (only for replicated boards)
        output.push_str(&format!("[VirtualBoard \"{}\"]\n", virtual_board));
        output.push_str(&format!(
            "[VirtualDealer \"{}\"]\n",
            virtual_dealer.to_char()
        ));
        output.push_str(&format!(
            "[VirtualVulnerable \"{}\"]\n",
            virtual_vul.to_pbn()
        ));
        output.push_str(&format!("[BlockNumber \"{}\"]\n", block_num + 1));

        output.push('\n');
    }

    output
}

/// Split PBN content into header and individual board sections
fn split_pbn_content(content: &str) -> (String, Vec<String>) {
    let mut header = String::new();
    let mut board_sections: Vec<String> = Vec::new();
    let mut current_board = String::new();
    let mut in_header = true;

    for line in content.lines() {
        let trimmed = line.trim();

        if in_header {
            if trimmed.starts_with('%') || trimmed.is_empty() {
                header.push_str(line);
                header.push('\n');
            } else if trimmed.starts_with('[') {
                in_header = false;
                current_board.push_str(line);
                current_board.push('\n');
            }
        } else {
            // Check if this is the start of a new board (Event tag typically starts a board)
            if trimmed.starts_with("[Event ") && !current_board.is_empty() {
                board_sections.push(std::mem::take(&mut current_board));
            }
            current_board.push_str(line);
            current_board.push('\n');
        }
    }

    // Don't forget the last board
    if !current_board.is_empty() {
        board_sections.push(current_board);
    }

    (header, board_sections)
}

/// Extract a tag value from a board section
fn extract_tag_value(section: &str, tag_name: &str) -> String {
    let pattern = format!(r#"\[{}\s+"([^"]+)"\]"#, tag_name);
    let re = Regex::new(&pattern).unwrap();
    if let Some(caps) = re.captures(section) {
        caps.get(1)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_vul_pattern() {
        // Board 1: None, Board 2: NS, Board 5: NS, Board 8: None
        assert_eq!(STANDARD_VUL[0], Vulnerability::None);
        assert_eq!(STANDARD_VUL[1], Vulnerability::NorthSouth);
        assert_eq!(STANDARD_VUL[4], Vulnerability::NorthSouth);
        assert_eq!(STANDARD_VUL[7], Vulnerability::None);
    }

    #[test]
    fn test_standard_dealer_pattern() {
        assert_eq!(STANDARD_DEALER[0], Direction::North);
        assert_eq!(STANDARD_DEALER[1], Direction::East);
        assert_eq!(STANDARD_DEALER[2], Direction::South);
        assert_eq!(STANDARD_DEALER[3], Direction::West);
    }
}
