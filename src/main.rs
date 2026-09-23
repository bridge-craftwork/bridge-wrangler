use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;

#[derive(Parser)]
#[command(name = "bridge-wrangler")]
#[command(about = "CLI tool for operations on bridge PBN files", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Rotate deals to set dealer/declarer according to a pattern
    RotateDeals(commands::rotate_deals::Args),
    /// Convert PBN file to PDF
    ToPdf(commands::to_pdf::Args),
    /// Convert PBN file to LIN format
    ToLin(commands::to_lin::Args),
    /// Replicate boards into blocks for multi-table play
    BlockReplicate(commands::block_replicate::Args),
    /// Filter boards by regex pattern
    Filter(commands::filter::Args),
    /// Update the Event tag for all boards
    Event(commands::event::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::RotateDeals(args) => commands::rotate_deals::run(args),
        Commands::ToPdf(args) => commands::to_pdf::run(args),
        Commands::ToLin(args) => commands::to_lin::run(args),
        Commands::BlockReplicate(args) => commands::block_replicate::run(args),
        Commands::Filter(args) => commands::filter::run(args),
        Commands::Event(args) => commands::event::run(args),
    }
}
