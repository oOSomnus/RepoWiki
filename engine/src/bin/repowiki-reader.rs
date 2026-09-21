use clap::Parser;
use codewiki::reader::{self, ReaderConfig};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "repowiki-reader",
    version,
    about = "Open a generated .repowiki directory in a local read-only WebUI"
)]
struct Args {
    #[arg(
        value_name = "WIKI_DIR",
        help = "Path to the generated .repowiki directory"
    )]
    wiki_dir: PathBuf,
    #[arg(
        long,
        default_value_t = 0,
        help = "Local TCP port; 0 selects a free port"
    )]
    port: u16,
    #[arg(long, help = "Do not open the default browser")]
    no_open: bool,
}

fn main() {
    let args = Args::parse();
    let config = ReaderConfig {
        wiki_dir: args.wiki_dir,
        port: args.port,
        open_browser: !args.no_open,
    };
    if let Err(error) = reader::run(config) {
        eprintln!("repowiki-reader: {error:#}");
        std::process::exit(1);
    }
}
