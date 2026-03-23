mod cmd;

use clap::{Parser, Subcommand};
use tcg_scanner::api::HistoryRange;

#[derive(Parser)]
#[command(name = "tcg-scanner", about = "TCGPlayer price scanner and analytics")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Collect prices for all cards in a profile's set
    Collect {
        /// Profile name from profiles.toml
        profile: String,
    },

    /// Scan for volume spikes across a profile's set
    Alerts {
        /// Profile name from profiles.toml
        profile: String,

        /// Spike threshold multiplier (default: 2.0)
        #[arg(short, long, default_value_t = 2.0)]
        threshold: f64,

        /// History range: month, quarter, semi, annual (default: month)
        #[arg(short, long, default_value = "month")]
        range: String,
    },

    /// Fetch and display a single product by ID
    Fetch {
        /// TCGPlayer product ID
        product_id: u64,
    },

    /// List available profiles from profiles.toml
    Profiles,
}

fn parse_range(s: &str) -> HistoryRange {
    match s {
        "quarter" | "3m" => HistoryRange::Quarter,
        "semi" | "6m" => HistoryRange::SemiAnnual,
        "annual" | "1y" => HistoryRange::Annual,
        _ => HistoryRange::Month,
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Collect { profile } => {
            cmd::collect::run(&profile).await?;
        }
        Commands::Alerts {
            profile,
            threshold,
            range,
        } => {
            cmd::alerts::run(&profile, threshold, parse_range(&range)).await?;
        }
        Commands::Fetch { product_id } => {
            cmd::fetch::run(product_id).await?;
        }
        Commands::Profiles => {
            cmd::common::list_profiles();
        }
    }

    Ok(())
}
