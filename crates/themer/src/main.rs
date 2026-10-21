use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Apply userChrome.css to Chrome's Webium UI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Inspect {
        pak: PathBuf,
    },
    Apply {
        pak: PathBuf,
        theme: PathBuf,
        #[arg(long, help = "Include theme scripts with Chrome UI privileges")]
        allow_unsafe_theme_files: bool,
    },
    Restore {
        pak: PathBuf,
    },
}

fn main() -> Result<()> {
    let inspection = match Cli::parse().command {
        Command::Inspect { pak } => chromethemer::inspect(&pak),
        Command::Apply {
            pak,
            theme,
            allow_unsafe_theme_files,
        } => {
            let files = if allow_unsafe_theme_files {
                chromethemer::ThemeFiles::Unsafe
            } else {
                chromethemer::ThemeFiles::CssOnly
            };
            chromethemer::apply(&pak, &theme, files)
        }
        Command::Restore { pak } => chromethemer::restore(&pak),
    }?;
    println!("{}", serde_json::to_string_pretty(&inspection)?);
    Ok(())
}
