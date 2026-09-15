use anyhow::{Context, Result};
use bridge_encodings::lin::{write_lin_file, LinData};
use bridge_encodings::pbn::read_pbn;
use clap::Args as ClapArgs;
use std::path::PathBuf;

#[derive(ClapArgs)]
pub struct Args {
    /// Input PBN file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output LIN file (defaults to <input>.lin)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Convert a PBN file to LIN, laid out the way Bridge Composer exports it.
pub fn run(args: Args) -> Result<()> {
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    let (board_count, lin_content) = pbn_to_lin(&content)?;

    // Determine output path
    let output_path = args
        .output
        .unwrap_or_else(|| args.input.with_extension("lin"));

    // Write output
    std::fs::write(&output_path, &lin_content)
        .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;

    println!("Converted {} boards to LIN format", board_count);
    println!("Wrote to {}", output_path.display());

    Ok(())
}

/// Encode every board in a PBN text as LIN, returning the board count and the
/// LIN text. bridge-encodings owns both formats, so this only connects its PBN
/// reader to its LIN writer.
fn pbn_to_lin(content: &str) -> Result<(usize, String)> {
    let boards = read_pbn(content).context("Failed to parse PBN file")?;
    let records: Vec<LinData> = boards.iter().map(LinData::from_board).collect();
    Ok((records.len(), write_lin_file(&records)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_encodings::lin::parse_lin_file;

    /// A board with names, `!`, `=n=` and `$2` on calls, and play past the
    /// first trick, from bridge-encodings' LIN probe fixtures.
    const PBN: &str = include_str!("../../tests/fixtures/input/alerts-notes-play.pbn");

    /// Bridge Composer 5.118.2's LIN export of the same board.
    const BRIDGE_COMPOSER_LIN: &str =
        include_str!("../../tests/fixtures/expected/alerts-notes-play.lin");

    #[test]
    fn matches_bridge_composer_export() {
        let (count, lin) = pbn_to_lin(PBN).unwrap();
        assert_eq!(count, 1);
        // Bridge Composer writes CRLF line endings; the writer writes LF.
        assert_eq!(lin, BRIDGE_COMPOSER_LIN.replace("\r\n", "\n"));
    }

    #[test]
    fn play_after_the_first_trick_is_in_play_order() {
        let (_, lin) = pbn_to_lin(PBN).unwrap();
        let boards = parse_lin_file(&lin).unwrap();
        assert_eq!(
            boards[0].format_cardplay_by_trick(),
            "D2 D3 DJ DQ|CA C5 CK C3|C4 C7 CQ C6"
        );
    }
}
