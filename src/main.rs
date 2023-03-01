use comrak::arena_tree::{Children, Node};
use comrak::nodes::{Ast, AstNode, NodeValue};
use comrak::{format_commonmark, parse_document, Arena, ComrakOptions};
use std::cell::RefCell;
use std::fs;

use std::fs::File;
use std::io::{self, Read};

use clap::Parser;
use std::str;
use textwrap::core::{Fragment, Word};
use textwrap::{self, wrap_algorithms::Penalties, WordSeparator, WrapAlgorithm};

// The returned nodes are created in the supplied Arena, and are bound by its lifetime.

// fn iter_nodes<'a, F>(node: &'a AstNode<'a>, f: &F)
// where
//     F: Fn(&'a AstNode<'a>),
// {
//     f(node);
//     for c in node.children() {
//         iter_nodes(c, f);
//     }
// }

#[derive(Debug)]
enum Style {
    Normal,
    Emph,
    Strong,
    Strikethrough,
    Superscript,
}

#[derive(Debug)]
struct StyledWord<'a> {
    word: Word<'a>,
    style: Style,
}

impl<'a> StyledWord<'a> {
    fn normal(word: Word<'a>) -> Self {
        Self {
            word,
            style: Style::Normal,
        }
    }
    fn emph(word: Word<'a>) -> Self {
        Self {
            word,
            style: Style::Emph,
        }
    }
    fn strong(word: Word<'a>) -> Self {
        Self {
            word,
            style: Style::Strong,
        }
    }
    fn strong(word: Word<'a>) -> Self {
        Self {
            word,
            style: Style::Strong,
        }
    }
}

impl Fragment for StyledWord<'_> {
    fn width(&self) -> f64 {
        self.word.width()
    }
    fn whitespace_width(&self) -> f64 {
        self.word.whitespace_width()
    }
    fn penalty_width(&self) -> f64 {
        self.word.penalty_width()
    }
}

struct Options {
    line_width: usize,
}

impl Options {
    // fn default() -> Self {
    //     Self { line_width: 50 }
    // }
}

struct Lines<'a> {
    opt: textwrap::Options<'a>,
    buf: String,
}

impl<'a> Lines<'a> {
    fn new(options: &Options) -> Self {
        let opt = textwrap::Options::new(options.line_width)
            .wrap_algorithm(WrapAlgorithm::OptimalFit(Penalties::new()));
        Self {
            opt,
            buf: String::new(),
        }
    }

    fn push(&mut self, v: &[u8]) -> Result<(), str::Utf8Error> {
        self.buf.push_str(str::from_utf8(v)?);
        Ok(())
    }

    fn push_char(&mut self, c: char) {
        self.buf.push(c);
    }

    fn extend(&mut self, l: Lines<'a>) {
        self.buf.push_str(&l.buf);
    }
}

struct WrappedLines {
    wrapped: Vec<String>,
}

impl<'a> Into<WrappedLines> for Lines<'a> {
    fn into(self) -> WrappedLines {
        let wrapped = textwrap::wrap(&self.buf, self.opt)
            .iter()
            .map(|c| c.clone().into_owned())
            .collect();
        // let wrapped = wrapped.iter().map(move |c| c.into_owned()).collect();
        WrappedLines { wrapped }
    }
}

impl WrappedLines {
    fn lines(&self) -> &Vec<String> {
        &self.wrapped
    }
}

// TODO: remove debug bound
fn wrap_text_elems<'a>(
    // parent: &mut Node<RefCell<Ast>>,
    nodes: &'a mut Children<RefCell<Ast>>,
    opt: &Options,
) -> Result<Lines<'a>, str::Utf8Error> {
    let mut lines = Lines::new(opt);
    for n in nodes {
        // println!("n: {:#?}", n);
        match &n.data.borrow().value {
            NodeValue::Text(text) => {
                lines.push(&text)?;
                lines.push_char(' ');
            }

            NodeValue::SoftBreak => (),
            // NodeValue::Emph => {
            //     let emph_lines = wrap_text_elems(&mut n.children(), opt);
            //     lines.extend(emph_lines?);
            // }
            other => println!("found other: {:?}", other),
            // _ => panic!(),
        }
    }
    // lines.wrap();
    Ok(lines)
}

fn wrap_list<'a>(
    list: &'a Node<'a, RefCell<Ast>>,
    // nodes: &'a mut Children<RefCell<Ast>>,
    arena: &'a Arena<Node<'a, RefCell<Ast>>>,
    opt: &Options,
) {
    for item in list.children() {
        let val = { &item.data.borrow().value };
        // println!("val: {:?}", n.data) ;
        match val {
            NodeValue::Item(_metadata) => {
                for paragraph in item.children() {
                    wrap_paragraph(paragraph, arena, opt);
                }
            }
            _ => panic!(),
        }
    }
}

fn wrap_paragraph<'a>(paragraph: &'a AstNode<'a>, arena: &'a Arena<AstNode<'a>>, opt: &Options) {
    let mut text_elems = paragraph.children();

    let mut words: Vec<StyledWord> = vec![];
    let sep = WordSeparator::new();
    let s;
    for n in text_elems {
        match n.data.borrow().value {
            NodeValue::Text(text) => {
                s = str::from_utf8(text);
                words.extend(sep.find_words(s).map(move |w| StyledWord::Normal(w)));
            }
            NodeValue::Emph => {}
            NodeValue::Strong => {}
            NodeValue::Strikethrough => {}
            NodeValue::Superscript => {}
            NodeValue::Link(link) => {}
            NodeValue::Image(link) => {}
            NodeValue::FootnoteReference(name) => {}
        }
    }

    let lines = wrap_text_elems(&mut text_elems, opt).unwrap();
    let mut wrapped = lines.into();

    for line in paragraph.children() {
        line.detach();
    }

    replace_paragraph_children(paragraph, &mut wrapped, arena);
}

fn replace_paragraph_children<'a, 'b>(
    paragraph: &'a AstNode<'a>,
    wrapped: &'b mut WrappedLines,
    arena: &'a Arena<AstNode<'a>>,
) {
    for line in wrapped.lines() {
        let a = Ast::new(NodeValue::Text(line.as_bytes().to_vec()));
        let text = arena.alloc(Node::new(RefCell::new(a)));
        let a = Ast::new(NodeValue::SoftBreak);
        let newline = arena.alloc(Node::new(RefCell::new(a)));
        paragraph.append(text);
        paragraph.append(newline);
    }
}

fn format_ast<'a>(root: &'a AstNode<'a>, arena: &'a Arena<AstNode<'a>>, opt: &Options) {
    for c in root.children() {
        // TODO: profile this
        let val = { c.data.borrow().value.clone() };
        // println!("{:#?}", val);
        match val {
            NodeValue::Paragraph => {
                wrap_paragraph(c, arena, opt);
            }
            NodeValue::List(_nl) => {
                wrap_list(c, arena, opt);
            }
            _ => (),
            // other => println!("found other: {:?}", other),
        };
    }
}
fn wrap_node<'a>(node: &'a AstNode<'a>, arena: &'a Arena<AstNode<'a>>, opt: &Options) {
    match &node.data.borrow().value {
        // NodeValue::Paragraph => wrap_paragraph(node, arena, opt),
        // NodeValue::Heading(_) => (),
        v => println!("node: {:?}", v),
    }

    for n in node.children() {
        println!("child of {:?}", node.data.borrow().value);
        wrap_node(n, arena, opt);
        println!("END child of {:?}", node.data.borrow().value);
    }
}

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
    let options = Options {
        line_width: args.linewidth,
    };
    // TODO change to Either
    let buffer = match args.file {
        Some(filename) => fs::read_to_string(filename)?,
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
    };

    let mut outfile: Box<dyn io::Write> = match args.outfile {
        Some(filename) => Box::new(File::open(filename)?),
        None => Box::new(io::stdout()),
    };

    let arena = Arena::new();
    let mut root = parse_document(&arena, &buffer, &ComrakOptions::default());
    wrap_node(&root, &arena, &options);
    // format_ast(&mut root, &arena, &options);
    format_commonmark(root, &ComrakOptions::default(), &mut outfile)?;

    // let mut buffer = String::new();
    // infile.read_to_string(&mut buffer)?;

    // let buffer = buffer.trim();
    // let mut outstream = BufWriter::new(outfile);
    // outstream.flush().unwrap();

    Ok(())
}
