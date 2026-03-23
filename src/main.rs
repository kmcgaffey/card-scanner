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

    /// Show top purchased cards by rarity, or scan for volume spikes
    Alerts {
        /// Profile name(s) from profiles.toml — omit with --all for all profiles
        profiles: Vec<String>,

        /// Run across all profiles in profiles.toml
        #[arg(long)]
        all: bool,

        /// Spike threshold multiplier for API-based scan (default: 2.0)
        #[arg(short, long, default_value_t = 2.0)]
        threshold: f64,

        /// History range: month, quarter, semi, annual (default: month)
        #[arg(short, long, default_value = "month")]
        range: String,

        /// Generate a graphic PNG of top cards with card art
        #[arg(long)]
        chart: bool,

        /// Output path for the graphic PNG
        #[arg(long, default_value = "alerts_chart.png")]
        chart_output: String,

        /// Post the generated graphic to X (Twitter)
        #[arg(long)]
        post: bool,
    },

    /// Fetch and display a single product by ID
    Fetch {
        /// TCGPlayer product ID
        product_id: u64,
    },

    /// Compute price index for one or all sets (excludes Signatures)
    Index {
        /// Profile name(s) — omit to use all profiles
        profiles: Vec<String>,
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

/// Load all profile names from profiles.toml.
fn all_profile_names() -> Vec<String> {
    let content = std::fs::read_to_string("profiles.toml").unwrap_or_else(|_| {
        eprintln!("No profiles.toml found.");
        std::process::exit(1);
    });
    let table: std::collections::HashMap<String, toml::Value> =
        toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("Failed to parse profiles.toml: {}", e);
            std::process::exit(1);
        });
    table.keys().cloned().collect()
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Collect { profile } => {
            cmd::collect::run(&profile).await?;
        }
        Commands::Alerts {
            profiles,
            all,
            threshold,
            range,
            chart,
            chart_output,
            post,
        } => {
            let names = if all {
                all_profile_names()
            } else if profiles.is_empty() {
                eprintln!("Provide profile name(s) or use --all");
                eprintln!("  tcg-scanner alerts spiritforged origins");
                eprintln!("  tcg-scanner alerts --all");
                std::process::exit(1);
            } else {
                profiles
            };
            cmd::alerts::run(
                &names,
                threshold,
                parse_range(&range),
                chart || post, // --post implies --chart
                &chart_output,
                post,
            )
            .await?;
        }
        Commands::Fetch { product_id } => {
            cmd::fetch::run(product_id).await?;
        }
        Commands::Index { profiles } => {
            cmd::index::run(&profiles)?;
        }
        Commands::Profiles => {
            cmd::common::list_profiles();
        }
    }

    Ok(())
}
