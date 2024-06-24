use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};

use clap::Parser;
use comrak::{format_commonmark, parse_document, Arena, ComrakOptions};

/// Wrap markdown files
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input file, use stdio if none provided
    #[arg(short, long)]
    file: Option<String>,

    /// Output file, use stdout if none provided
    #[arg(short, long)]
    outfile: Option<String>,

    /// Maximum line width in chars
    #[arg(short, long, default_value_t = 80)]
    linewidth: usize,
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let buffer = match args.file {
        Some(filename) => fs::read_to_string(filename)?,
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
    };

    let arena = Arena::new();

    let mut comrak_options = ComrakOptions::default();
    comrak_options.render.width = args.linewidth;
    comrak_options.extension.table = true;

    let root = parse_document(&arena, &buffer, &comrak_options);
    match args.outfile {
        Some(filename) => {
            let mut outfile = BufWriter::new(File::open(filename)?);
            format_commonmark(root, &comrak_options, &mut outfile)?;
            outfile.flush()?;
        }
        None => {
            let mut outfile = BufWriter::new(io::stdout());
            format_commonmark(root, &comrak_options, &mut outfile)?;
            outfile.flush()?;
        }
    }

    Ok(())
}
