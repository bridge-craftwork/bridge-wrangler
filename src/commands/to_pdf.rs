use anyhow::{Context, Result};
use clap::{Args as ClapArgs, ValueEnum};
use pbn_to_pdf::{parse_pbn, render_boards, Layout as PdfLayout, RenderOptions};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum Layout {
    /// Full hand diagram with bidding table and commentary
    #[default]
    Analysis,
    /// Simplified layout for practice bidding
    BiddingSheets,
    /// Declarer's plan - 1 deal per page (full size)
    #[value(name = "declarers-plan-1up")]
    DeclarersPlan1up,
    /// Declarer's plan - 2 deals per page (rotated 90°)
    #[value(name = "declarers-plan-2up")]
    DeclarersPlan2up,
    /// Declarer's plan practice sheets (4 deals per page)
    DeclarersPlan,
    /// 6 deals per page summary for the dealer
    DealerSummary,
}

impl From<Layout> for PdfLayout {
    fn from(layout: Layout) -> Self {
        match layout {
            Layout::Analysis => PdfLayout::Analysis,
            Layout::BiddingSheets => PdfLayout::BiddingSheets,
            Layout::DeclarersPlan1up => PdfLayout::DeclarersPlan1up,
            Layout::DeclarersPlan2up => PdfLayout::DeclarersPlan2up,
            Layout::DeclarersPlan => PdfLayout::DeclarersPlan,
            Layout::DealerSummary => PdfLayout::DealerSummary,
        }
    }
}

#[derive(ClapArgs)]
pub struct Args {
    /// Input PBN file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output PDF file (defaults to input with .pdf extension)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Layout style
    #[arg(short, long, value_enum, default_value = "analysis")]
    pub layout: Layout,

    /// Board range to include (e.g., "1-4" or "1,3,5")
    #[arg(short = 'r', long)]
    pub board_range: Option<String>,

    /// Circle sure winners in red (declarer's plan layouts; priority 1)
    #[arg(long)]
    pub circle_sure_winners: bool,

    /// Circle promotable winners in green (declarer's plan layouts; priority 2)
    #[arg(long)]
    pub circle_promotable_winners: bool,

    /// Circle length winners in blue (declarer's plan layouts; priority 3)
    #[arg(long)]
    pub circle_length_winners: bool,
}

pub fn run(args: Args) -> Result<()> {
    // Read input file
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    // Extract metadata comments (lines starting with %)
    let metadata_comments: Vec<String> = content
        .lines()
        .filter(|line| line.starts_with('%'))
        .map(String::from)
        .collect();

    // Parse PBN
    let pbn_file =
        parse_pbn(&content).map_err(|e| anyhow::anyhow!("Failed to parse PBN: {:?}", e))?;

    println!(
        "Parsed {} boards from {}",
        pbn_file.boards.len(),
        args.input.display()
    );

    // Filter boards if range specified
    let boards = if let Some(ref range) = args.board_range {
        let allowed = parse_board_range(range)?;
        pbn_file
            .boards
            .into_iter()
            .filter(|b| b.number.map(|n| allowed.contains(&n)).unwrap_or(false))
            .collect::<Vec<_>>()
    } else {
        pbn_file.boards
    };

    if boards.is_empty() {
        return Err(anyhow::anyhow!("No boards to render after filtering"));
    }

    let options = RenderOptions {
        circle_sure_winners: args.circle_sure_winners,
        circle_promotable_winners: args.circle_promotable_winners,
        circle_length_winners: args.circle_length_winners,
    };

    // Generate PDF using the high-level API
    let pdf_bytes = render_boards(&boards, &metadata_comments, args.layout.into(), options)
        .map_err(|e| anyhow::anyhow!("Failed to generate PDF: {:?}", e))?;

    // Determine output path
    let output_path = args
        .output
        .unwrap_or_else(|| args.input.with_extension("pdf"));

    // Write output
    std::fs::write(&output_path, &pdf_bytes)
        .with_context(|| format!("Failed to write PDF: {}", output_path.display()))?;

    println!("Wrote {} boards to {}", boards.len(), output_path.display());

    Ok(())
}

/// Parse a board range specification like "1-4" or "1,3,5" or "1-4,7,9-12"
fn parse_board_range(range: &str) -> Result<Vec<u32>> {
    let mut boards = Vec::new();

    for part in range.split(',') {
        let part = part.trim();
        if part.contains('-') {
            let parts: Vec<&str> = part.split('-').collect();
            if parts.len() != 2 {
                return Err(anyhow::anyhow!("Invalid range: {}", part));
            }
            let start: u32 = parts[0]
                .trim()
                .parse()
                .with_context(|| format!("Invalid number in range: {}", parts[0]))?;
            let end: u32 = parts[1]
                .trim()
                .parse()
                .with_context(|| format!("Invalid number in range: {}", parts[1]))?;
            for i in start..=end {
                boards.push(i);
            }
        } else {
            let num: u32 = part
                .parse()
                .with_context(|| format!("Invalid board number: {}", part))?;
            boards.push(num);
        }
    }

    Ok(boards)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_board_range() {
        assert_eq!(parse_board_range("1-4").unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(parse_board_range("1,3,5").unwrap(), vec![1, 3, 5]);
        assert_eq!(parse_board_range("1-3,7").unwrap(), vec![1, 2, 3, 7]);
        assert_eq!(parse_board_range("1").unwrap(), vec![1]);
    }
}
