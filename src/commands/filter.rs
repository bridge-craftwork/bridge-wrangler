use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use pbn_to_pdf::{config::Settings, parser::parse_pbn, render::generate_pdf};
use regex::RegexBuilder;
use std::path::{Path, PathBuf};

#[derive(ClapArgs)]
pub struct Args {
    /// Input PBN file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Regex pattern to match against each board
    #[arg(short = 'p', long)]
    pub pattern: String,

    /// Output file for matched boards (defaults to <input>-Matched.pbn if neither -m nor -n specified)
    #[arg(short = 'm', long)]
    pub matched: Option<PathBuf>,

    /// Output file for non-matched boards
    #[arg(short = 'n', long = "not-matched")]
    pub not_matched: Option<PathBuf>,

    /// Case-sensitive matching (default is case-insensitive)
    #[arg(long)]
    pub case_sensitive: bool,

    /// Renumber boards sequentially (1, 2, 3, ...)
    #[arg(long, default_value = "true")]
    pub renumber: bool,

    /// Also generate PDFs of the output files
    #[arg(long)]
    pub pdf: bool,

    /// Leave the page furniture (event header or headings, %PageFooter lines)
    /// out of the PDF. For pipelines that add their own.
    #[arg(long)]
    pub no_page_furniture: bool,
}

pub fn run(args: Args) -> Result<()> {
    // Compile the regex pattern (case-insensitive by default, like the JS version)
    let re = RegexBuilder::new(&args.pattern)
        .case_insensitive(!args.case_sensitive)
        .build()
        .with_context(|| format!("Invalid regex pattern: {}", args.pattern))?;

    // Read input file
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    // Split content into header and board sections
    let (header, board_sections) = split_pbn_content(&content);

    let total_boards = board_sections.len();

    if total_boards == 0 {
        println!("No boards found in input file");
        return Ok(());
    }

    // Separate boards into matched and not-matched
    let mut matched_boards = Vec::new();
    let mut not_matched_boards = Vec::new();

    for section in &board_sections {
        if re.is_match(section) {
            matched_boards.push(section.clone());
        } else {
            not_matched_boards.push(section.clone());
        }
    }

    let matched_count = matched_boards.len();
    let not_matched_count = not_matched_boards.len();
    let match_percent = (matched_count as f64 / total_boards as f64) * 100.0;

    // Determine which outputs to write
    // If neither -m nor -n specified, default to matched output
    let write_matched = args.matched.is_some() || args.not_matched.is_none();
    let write_not_matched = args.not_matched.is_some();

    // Helper to get default output path
    let get_default_path = |suffix: &str| {
        let stem = args.input.file_stem().unwrap_or_default().to_string_lossy();
        let parent = args.input.parent().unwrap_or(std::path::Path::new("."));
        parent.join(format!("{}-{}.pbn", stem, suffix))
    };

    // Write matched boards if requested
    if write_matched {
        let matched_path = args
            .matched
            .clone()
            .unwrap_or_else(|| get_default_path("Matched"));

        let output_boards = if args.renumber {
            renumber_boards(&matched_boards)
        } else {
            matched_boards.clone()
        };

        let output_content = build_output(&header, &output_boards);

        std::fs::write(&matched_path, &output_content)
            .with_context(|| format!("Failed to write matched file: {}", matched_path.display()))?;

        println!(
            "Wrote {} matched boards to {}",
            matched_count,
            matched_path.display()
        );

        // Generate PDF if requested
        if args.pdf && matched_count > 0 {
            generate_pdf_file(&output_content, &matched_path, args.no_page_furniture)?;
        }
    }

    // Write not-matched boards if requested
    if write_not_matched {
        let not_matched_path = args.not_matched.clone().unwrap();

        let output_boards = if args.renumber {
            renumber_boards(&not_matched_boards)
        } else {
            not_matched_boards.clone()
        };

        let output_content = build_output(&header, &output_boards);

        std::fs::write(&not_matched_path, &output_content).with_context(|| {
            format!(
                "Failed to write not-matched file: {}",
                not_matched_path.display()
            )
        })?;

        println!(
            "Wrote {} not-matched boards to {}",
            not_matched_count,
            not_matched_path.display()
        );

        // Generate PDF if requested
        if args.pdf && not_matched_count > 0 {
            generate_pdf_file(&output_content, &not_matched_path, args.no_page_furniture)?;
        }
    }

    // Print summary
    println!();
    println!("Filter results for pattern: {}", args.pattern);
    println!("  Boards scanned:     {}", total_boards);
    println!("  Boards matched:     {}", matched_count);
    println!("  Boards not matched: {}", not_matched_count);
    println!("  Match rate:         {:.1}%", match_percent);

    Ok(())
}

/// Generate PDF from PBN content and write to file
fn generate_pdf_file(pbn_content: &str, pbn_path: &Path, no_page_furniture: bool) -> Result<()> {
    let pdf_path = pbn_path.with_extension("pdf");

    let pbn_file = parse_pbn(pbn_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse PBN for PDF: {:?}", e))?;

    let mut settings = Settings::default().with_metadata(&pbn_file.metadata);
    settings.page_furniture = !no_page_furniture;

    let pdf_bytes = generate_pdf(&pbn_file.boards, &settings)
        .map_err(|e| anyhow::anyhow!("Failed to generate PDF: {:?}", e))?;

    std::fs::write(&pdf_path, &pdf_bytes)
        .with_context(|| format!("Failed to write PDF: {}", pdf_path.display()))?;

    println!("Wrote PDF to {}", pdf_path.display());
    Ok(())
}

/// Renumber boards sequentially starting from 1
fn renumber_boards(boards: &[String]) -> Vec<String> {
    let board_re = regex::Regex::new(r#"\[Board\s+"[^"]*"\]"#).unwrap();

    boards
        .iter()
        .enumerate()
        .map(|(i, section)| {
            let new_board_tag = format!("[Board \"{}\"]", i + 1);
            board_re
                .replace(section, new_board_tag.as_str())
                .to_string()
        })
        .collect()
}

/// Build output content from header and board sections
fn build_output(header: &str, boards: &[String]) -> String {
    let mut output = String::new();
    output.push_str(header);
    for board in boards {
        output.push_str(board);
        if !board.ends_with('\n') {
            output.push('\n');
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regex_matching() {
        let re = RegexBuilder::new(r#"\[Contract "3NT"\]"#)
            .case_insensitive(true)
            .build()
            .unwrap();
        let board = r#"[Board "1"]
[Contract "3NT"]
[Deal "N:..."]
"#;
        assert!(re.is_match(board));
    }

    #[test]
    fn test_case_insensitive_matching() {
        let re = RegexBuilder::new("3nt")
            .case_insensitive(true)
            .build()
            .unwrap();
        let board = r#"[Contract "3NT"]"#;
        assert!(re.is_match(board));
    }

    #[test]
    fn test_renumber_boards() {
        let boards = vec![
            r#"[Board "5"]
[Deal "N:..."]
"#
            .to_string(),
            r#"[Board "8"]
[Deal "N:..."]
"#
            .to_string(),
        ];
        let renumbered = renumber_boards(&boards);
        assert!(renumbered[0].contains("[Board \"1\"]"));
        assert!(renumbered[1].contains("[Board \"2\"]"));
    }

    #[test]
    fn test_split_pbn_content() {
        let content = r#"% PBN 2.1
%Creator: Test
[Event "Test"]
[Board "1"]
[Deal "N:..."]

[Event "Test"]
[Board "2"]
[Deal "N:..."]
"#;
        let (header, boards) = split_pbn_content(content);
        assert!(header.contains("% PBN 2.1"));
        assert_eq!(boards.len(), 2);
    }
}
