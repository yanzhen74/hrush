use clap::Parser;
use anyhow::Result;

mod app;
mod buffer;
mod command;
mod editor;
mod import;
mod input;
mod large_file;
mod ui;
mod search;
mod undo;
mod frame;

#[derive(Parser, Debug)]
#[command(name = "hrush")]
#[command(about = "A hex editor TUI")]
struct Args {
    /// Optional file path to open
    file: Option<String>,

    /// Import hex text file
    #[arg(long)]
    import: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    app::run(args.file, args.import)
}
