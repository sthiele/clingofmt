use anyhow::{Context, Result};
use clap::Parser;
use clingofmt::format_program;
use log::error;
use std::fs;
use std::{
    io::{self, Write},
    path::PathBuf,
};

/// Format clingo code
#[derive(Parser, Debug)]
#[clap(version, author)]
struct Opt {
    /// Input file in clingo format
    #[clap(name = "FILE")]
    file: PathBuf,

    /// Enable debug output
    #[clap(long)]
    debug: bool,
}

pub enum Reader<'a> {
    File(io::BufReader<fs::File>),
    Stdin(io::StdinLock<'a>),
}
impl<'a> io::Read for Reader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::File(reader) => reader.read(buf),
            Self::Stdin(guard) => guard.read(buf),
        }
    }
}
impl<'a> io::BufRead for Reader<'a> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self {
            Self::File(reader) => reader.fill_buf(),
            Self::Stdin(guard) => guard.fill_buf(),
        }
    }
    fn consume(&mut self, amt: usize) {
        match self {
            Self::File(reader) => reader.consume(amt),
            Self::Stdin(guard) => guard.consume(amt),
        }
    }
}

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .format_timestamp(None)
        .init();
    if let Err(err) = run() {
        error!("{:?}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let opt = Opt::parse();
    let cfg: clingofmt::Config = confy::load_path(".clingofmt")?;
    let path = opt.file.to_str().unwrap();
    let source_code =
        fs::read(path).with_context(|| format!("Error reading source file {}", path))?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_clingo::language())
        .expect("Error loading clingo grammar");
    let tree = parser.parse(&source_code, None).unwrap();

    let mut buf = Vec::new();
    format_program(&tree, &source_code, &mut buf, opt.debug, &cfg)?;

    let mut out = std::io::stdout();
    let buf_str = std::str::from_utf8(&buf)?;
    write!(out, "{buf_str}")?;
    Ok(())
}
