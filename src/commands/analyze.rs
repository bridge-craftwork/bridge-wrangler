use anyhow::{Context, Result};
use bridge_encodings::pbn::{optimum_result_table_rows, PbnDocument, OPTIMUM_RESULT_TABLE_HEADER};
use bridge_solver::{
    direction_to_seat, CutoffCache, Hands, PatternCache, Solver, Suit, CLUB, DIAMOND, HEART,
    NOTRUMP, SPADE,
};
use bridge_types::{Board, DdTable, Direction, Strain, Vulnerability};
use clap::Args as ClapArgs;
use std::path::PathBuf;

/// Declarer rows of this tool's console table: partners adjacent, the way a
/// double-dummy table is read at the table.
///
/// This is presentation only. The PBN encoding states its own row order in
/// `bridge_encodings::pbn`, and [`DdTable`] keeps its storage order private so
/// the two cannot be confused.
const DISPLAY_DECLARERS: [Direction; 4] = [
    Direction::North,
    Direction::South,
    Direction::East,
    Direction::West,
];

/// Denomination columns of this tool's console table: notrump first, then
/// spades down to clubs. Presentation only, as [`DISPLAY_DECLARERS`].
const DISPLAY_STRAINS: [Strain; 5] = [
    Strain::NoTrump,
    Strain::Spades,
    Strain::Hearts,
    Strain::Diamonds,
    Strain::Clubs,
];

#[derive(ClapArgs)]
pub struct Args {
    /// Input PBN file
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output PBN file with DD results (optional)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Board range to analyze (e.g., "1-4" or "1,3,5")
    #[arg(short = 'r', long)]
    pub board_range: Option<String>,

    /// Show detailed output for each board
    #[arg(short, long)]
    pub verbose: bool,
}

/// Format a solved table for the console.
///
/// This is the tool's own presentation, not a PBN encoding: rows in
/// [`DISPLAY_DECLARERS`] order, columns in [`DISPLAY_STRAINS`] order.
pub fn display_table(table: &DdTable) -> String {
    let mut output = String::from("       NT   S   H   D   C\n");
    for declarer in DISPLAY_DECLARERS {
        output.push_str(&format!("  {}  ", declarer.to_char()));
        for strain in DISPLAY_STRAINS {
            output.push_str(&format!("  {:2}", table.tricks(declarer, strain)));
        }
        output.push('\n');
    }
    output
}

/// The par contract and score for a solved table.
///
/// A simplified par: the better of the two sides' best makeable contracts,
/// without modelling a sacrifice.
pub fn par_score(table: &DdTable, vul_ns: bool, vul_ew: bool) -> (String, i32) {
    let (ns_contract, ns_score) = best_contract_for_side(table, true, vul_ns);
    let (ew_contract, ew_score) = best_contract_for_side(table, false, vul_ew);

    // The par is the result after competitive bidding: if NS can make game and
    // EW cannot profitably sacrifice, NS plays game.
    if ns_score >= -ew_score {
        (ns_contract, ns_score)
    } else {
        (ew_contract, -ew_score)
    }
}

/// The highest-scoring contract one side can make, and its score.
fn best_contract_for_side(table: &DdTable, is_ns: bool, declarer_vul: bool) -> (String, i32) {
    let declarers: [Direction; 2] = if is_ns {
        [Direction::North, Direction::South]
    } else {
        [Direction::East, Direction::West]
    };

    let mut best_contract = String::new();
    let mut best_score = i32::MIN;

    for declarer in declarers {
        for strain in DISPLAY_STRAINS {
            let tricks = table.tricks(declarer, strain);
            for level in 1..=7 {
                let required = level + 6;
                if tricks >= required {
                    let score = calculate_score(level, strain, tricks, declarer_vul, false);
                    if score > best_score {
                        best_score = score;
                        best_contract =
                            format!("{}{} by {}", level, strain.to_pbn(), declarer.to_char());
                    }
                }
            }
        }
    }

    if best_contract.is_empty() {
        best_contract = "Pass".to_string();
        best_score = 0;
    }

    (best_contract, best_score)
}

/// Calculate the score for a made contract
fn calculate_score(level: u8, strain: Strain, tricks: u8, vul: bool, doubled: bool) -> i32 {
    let overtricks = tricks as i32 - (level as i32 + 6);
    let trick_value = strain.trick_value();

    let mut score = if strain == Strain::NoTrump {
        40 + (level as i32 - 1) * 30 // NT: 40 for first, 30 for rest
    } else {
        level as i32 * trick_value
    };

    if doubled {
        score *= 2;
    }

    // Game/slam bonuses
    let game_threshold = match strain {
        Strain::NoTrump => 3,                  // 3NT
        Strain::Spades | Strain::Hearts => 4,  // 4M
        Strain::Diamonds | Strain::Clubs => 5, // 5m
    };

    if level >= game_threshold {
        score += if vul { 500 } else { 300 }; // Game bonus
    } else {
        score += 50; // Part score bonus
    }

    if level == 6 {
        score += if vul { 750 } else { 500 }; // Small slam
    } else if level == 7 {
        score += if vul { 1500 } else { 1000 }; // Grand slam
    }

    // Overtricks
    let overtrick_value = if doubled {
        if vul {
            200
        } else {
            100
        }
    } else {
        trick_value
    };
    score += overtricks * overtrick_value;

    score
}

pub fn run(args: Args) -> Result<()> {
    let content = std::fs::read_to_string(&args.input)
        .with_context(|| format!("Failed to read input file: {}", args.input.display()))?;

    // One parse serves both jobs: `boards()` is what `read_pbn` would return,
    // and the same document edits itself in place later without disturbing the
    // bytes it did not write.
    let mut doc = PbnDocument::parse(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse PBN: {:?}", e))?;

    println!(
        "Read {} boards from {}",
        doc.boards().len(),
        args.input.display()
    );

    let allowed = match args.board_range {
        Some(ref range) => Some(parse_board_range(range)?),
        None => None,
    };

    let selected: Vec<usize> = doc
        .boards()
        .iter()
        .enumerate()
        .filter(|(_, board)| match allowed {
            Some(ref allowed) => board.number.is_some_and(|n| allowed.contains(&n)),
            None => true,
        })
        .map(|(index, _)| index)
        .collect();

    if selected.is_empty() {
        return Err(anyhow::anyhow!("No boards to analyze after filtering"));
    }

    // Solve first, edit afterwards: `boards()` borrows the document, and the
    // section writes need it mutably.
    let mut results: Vec<(usize, DdTable)> = Vec::new();

    for index in selected {
        let board = &doc.boards()[index];
        let board_num = board.number.unwrap_or(0);

        let hands = match board_to_hands(board) {
            Some(h) => h,
            None => {
                println!("Board {}: No deal found, skipping", board_num);
                continue;
            }
        };

        println!("Analyzing board {}...", board_num);

        let table = analyze_deal(&hands);

        if args.verbose {
            println!("Board {}:", board_num);
            println!("{}", display_table(&table));

            let (vul_ns, vul_ew) = match board.vulnerable {
                Vulnerability::None => (false, false),
                Vulnerability::NorthSouth => (true, false),
                Vulnerability::EastWest => (false, true),
                Vulnerability::Both => (true, true),
            };
            let (par_contract, par) = par_score(&table, vul_ns, vul_ew);
            println!("  Par: {} ({})\n", par_contract, par);
        }

        results.push((index, table));
    }

    println!("Analyzed {} boards", results.len());

    if let Some(output_path) = args.output {
        for (index, table) in &results {
            set_optimum_result_table(&mut doc, *index, table)?;
        }
        doc.write_file(&output_path)
            .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;
        println!("\nWrote PBN with DD results to {}", output_path.display());
    }

    Ok(())
}

/// Write one board's `OptimumResultTable` section, replacing any it already
/// carries.
///
/// The header and rows come from `bridge_encodings`, which is the one place
/// that says how a double-dummy table is written down: PBN 2.1 §5.7 defines the
/// tag as a table section, a header naming its three columns followed by one
/// line per cell.
fn set_optimum_result_table(doc: &mut PbnDocument, board: usize, table: &DdTable) -> Result<()> {
    let rows = optimum_result_table_rows(table);
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    doc.set_section(
        board,
        "OptimumResultTable",
        OPTIMUM_RESULT_TABLE_HEADER,
        &rows,
    )
    .map_err(|e| anyhow::anyhow!("Failed to write OptimumResultTable: {:?}", e))
}

/// Convert a Board's deal to solver Hands format
fn board_to_hands(board: &Board) -> Option<Hands> {
    let deal = &board.deal;

    // Check if deal has cards (at least one hand has cards)
    if deal.hand(Direction::North).is_empty()
        && deal.hand(Direction::East).is_empty()
        && deal.hand(Direction::South).is_empty()
        && deal.hand(Direction::West).is_empty()
    {
        return None;
    }

    // Build PBN deal string: "N:spades.hearts.diamonds.clubs spades.hearts.diamonds.clubs ..."
    // Order is N E S W
    let pbn_deal = format!(
        "N:{} {} {} {}",
        deal.hand(Direction::North).to_pbn(),
        deal.hand(Direction::East).to_pbn(),
        deal.hand(Direction::South).to_pbn(),
        deal.hand(Direction::West).to_pbn()
    );

    Hands::from_pbn(&pbn_deal)
}

/// The solver's trump constant for a strain.
fn solver_trump(strain: Strain) -> Suit {
    match strain {
        Strain::NoTrump => NOTRUMP,
        Strain::Spades => SPADE,
        Strain::Hearts => HEART,
        Strain::Diamonds => DIAMOND,
        Strain::Clubs => CLUB,
    }
}

/// Perform DD analysis on a deal
fn analyze_deal(hands: &Hands) -> DdTable {
    let mut table = DdTable::new();

    for strain in DISPLAY_STRAINS {
        // Create caches once per denomination for efficiency
        let mut cutoff_cache = CutoffCache::new(16);
        let mut pattern_cache = PatternCache::new(16);

        for declarer in DISPLAY_DECLARERS {
            let declarer_seat = direction_to_seat(declarer);
            // Leader is to the left of declarer
            let leader = (declarer_seat + 1) % 4;

            let solver = Solver::new(*hands, solver_trump(strain), leader);
            let ns_tricks = solver.solve_with_caches(&mut cutoff_cache, &mut pattern_cache);

            // Convert NS tricks to declarer's tricks
            let declarer_tricks = match declarer {
                Direction::North | Direction::South => ns_tricks,
                Direction::East | Direction::West => hands.num_tricks() as u8 - ns_tricks,
            };

            table.set(declarer, strain, declarer_tricks);
        }
    }

    table
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

    /// A two-board file carrying every kind of byte `PbnDocument` promises to
    /// leave alone: a `%` directive block, `;` comments and `{...}` commentary.
    const ANNOTATED_PBN: &str = concat!(
        "% PBN 2.1\n",
        "% EXPORT\n",
        ";\n",
        "; Hand-authored header comment - must survive the rewrite.\n",
        ";\n",
        "[Event \"Test Session\"]\n",
        "[Board \"1\"]\n",
        "[Dealer \"N\"]\n",
        "[Vulnerable \"None\"]\n",
        "[Deal \"N:K843.T542.J6.863 AQJ7.K.Q75.AT942 962.AJ7.KT82.J75 T5.Q9863.A943.KQ\"]\n",
        "{ Board one commentary, written by hand. }\n",
        "\n",
        "[Event \"Test Session\"]\n",
        "[Board \"2\"]\n",
        "[Dealer \"E\"]\n",
        "[Vulnerable \"NS\"]\n",
        "[Deal \"N:AQJ7.K.Q75.AT942 962.AJ7.KT82.J75 T5.Q9863.A943.KQ K843.T542.J6.863\"]\n",
        "; a trailing comment on board two\n",
    );

    fn sample_table() -> DdTable {
        let mut table = DdTable::new();
        let mut n = 0u8;
        for declarer in DISPLAY_DECLARERS {
            for strain in DISPLAY_STRAINS {
                table.set(declarer, strain, n % 14);
                n += 1;
            }
        }
        table
    }

    #[test]
    fn test_parse_board_range() {
        assert_eq!(parse_board_range("1-4").unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(parse_board_range("1,3,5").unwrap(), vec![1, 3, 5]);
        assert_eq!(parse_board_range("1-3,7").unwrap(), vec![1, 2, 3, 7]);
        assert_eq!(parse_board_range("1").unwrap(), vec![1]);
    }

    #[test]
    fn test_calculate_score() {
        // 3NT making exactly, not vul
        assert_eq!(calculate_score(3, Strain::NoTrump, 9, false, false), 400); // 100 + 300 game

        // 4S making exactly, vul
        assert_eq!(calculate_score(4, Strain::Spades, 10, true, false), 620); // 120 + 500 game

        // 3NT with 2 overtricks, not vul
        assert_eq!(calculate_score(3, Strain::NoTrump, 11, false, false), 460); // 100 + 300 + 60
    }

    #[test]
    fn display_table_is_notrump_first_and_partners_adjacent() {
        let table = sample_table();
        let shown = display_table(&table);
        let lines: Vec<&str> = shown.lines().collect();
        assert_eq!(lines[0], "       NT   S   H   D   C");
        assert_eq!(lines.len(), 5);
        for (line, declarer) in lines[1..].iter().zip(DISPLAY_DECLARERS) {
            assert!(line.starts_with(&format!("  {}  ", declarer.to_char())));
        }
    }

    /// The tag is a PBN 2.1 §5.7 table section, not one quoted value with
    /// embedded separators.
    #[test]
    fn optimum_result_table_is_written_as_a_section() {
        let mut doc = PbnDocument::parse(ANNOTATED_PBN).unwrap();
        set_optimum_result_table(&mut doc, 0, &sample_table()).unwrap();
        let out = doc.to_pbn();

        assert!(out.contains(&format!(
            "[OptimumResultTable \"{}\"]",
            OPTIMUM_RESULT_TABLE_HEADER
        )));
        // Twenty data rows, one cell each, and nothing tab-separated.
        let rows = doc.tag_rows(0, "OptimumResultTable");
        assert_eq!(rows.len(), 20);
        assert!(!out.contains('\t'));
        assert!(!out.contains("\\n"));
        for row in rows {
            assert_eq!(row.split_whitespace().count(), 3);
        }
    }

    #[test]
    fn rewrite_preserves_directives_comments_and_commentary() {
        let mut doc = PbnDocument::parse(ANNOTATED_PBN).unwrap();
        assert_eq!(doc.boards().len(), 2);
        for board in 0..doc.boards().len() {
            set_optimum_result_table(&mut doc, board, &sample_table()).unwrap();
        }
        let out = doc.to_pbn();

        for preserved in [
            "% PBN 2.1",
            "% EXPORT",
            "; Hand-authored header comment - must survive the rewrite.",
            "{ Board one commentary, written by hand. }",
            "; a trailing comment on board two",
            "[Event \"Test Session\"]",
            "[Deal \"N:K843.T542.J6.863 AQJ7.K.Q75.AT942 962.AJ7.KT82.J75 T5.Q9863.A943.KQ\"]",
        ] {
            assert!(out.contains(preserved), "lost {preserved:?}");
        }
    }

    /// An unedited document is byte-for-byte the file it was given, and a board
    /// that is not selected keeps every byte it had.
    #[test]
    fn untouched_boards_are_untouched() {
        let doc = PbnDocument::parse(ANNOTATED_PBN).unwrap();
        assert!(!doc.is_modified());
        assert_eq!(doc.to_pbn(), ANNOTATED_PBN);

        let mut doc = doc;
        set_optimum_result_table(&mut doc, 1, &sample_table()).unwrap();
        assert!(doc.is_modified());
        assert!(doc.tag_rows(0, "OptimumResultTable").is_empty());
        assert_eq!(doc.tag_rows(1, "OptimumResultTable").len(), 20);
    }

    /// Replacing an existing table leaves one section behind, not two.
    #[test]
    fn existing_table_is_replaced_not_duplicated() {
        let mut doc = PbnDocument::parse(ANNOTATED_PBN).unwrap();
        set_optimum_result_table(&mut doc, 0, &sample_table()).unwrap();
        set_optimum_result_table(&mut doc, 0, &sample_table()).unwrap();
        let out = doc.to_pbn();
        assert_eq!(out.matches("[OptimumResultTable ").count(), 1);
        assert_eq!(doc.tag_rows(0, "OptimumResultTable").len(), 20);
    }
}
