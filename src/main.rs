use comrak::arena_tree::Node;
use comrak::nodes::{Ast, AstNode, NodeValue};
use comrak::{format_commonmark, parse_document, Arena, ComrakOptions};
use std::fs;

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Read};

use clap::Parser;
use std::str;
use std::str::Utf8Error;
use textwrap::core::{Fragment, Word};
use textwrap::wrap_algorithms::{wrap_optimal_fit, Penalties};
use textwrap::{self, WordSeparator};

#[derive(Debug, Clone, PartialEq)]
enum Style {
    Emph,
    Strong,
    Strikethrough,
    Superscript,
}

impl Style {
    fn _added_width(&self) -> usize {
        match self {
            Self::Emph => 2,          // *__*
            Self::Strong => 4,        // **__**
            Self::Strikethrough => 4, // ~~__~~
            Self::Superscript => 11,  // <sup>__</sup>
        }
    }

    fn to_node<'a>(&self) -> AstNode<'a> {
        let nv = match self {
            Style::Emph => NodeValue::Emph,
            Style::Strong => NodeValue::Strong,
            Style::Strikethrough => NodeValue::Strikethrough,
            Style::Superscript => NodeValue::Superscript,
        };
        Node::new(RefCell::new(Ast::new(nv)))
    }
}

#[derive(Debug)]
struct StyledWord<'a> {
    word: Word<'a>,
    style: &'a Vec<Style>,
}

impl<'a> StyledWord<'a> {
    fn from(word: Word<'a>, style: &'a Vec<Style>) -> Self {
        Self { word, style }
    }

    fn str(&self) -> &'a str {
        self.word.word
    }

    fn has_style(&self, s: &Vec<Style>) -> bool {
        self.style == s
    }
}

impl Fragment for StyledWord<'_> {
    fn width(&self) -> f64 {
        self.word.width() // + (self.style.iter().map(|s| s.added_width()).sum::<usize>() as f64)
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

struct TaggedString {
    buf: String,
    styles: Vec<(usize, Vec<Style>)>,
}

impl TaggedString {
    fn new() -> Self {
        Self {
            buf: String::new(),
            styles: vec![],
        }
    }
}

fn replace_paragraph_children<'a, 'b>(
    paragraph: &'a AstNode<'a>,
    arena: &'a Arena<AstNode<'a>>,
    _opt: &Options,
    wrapped: Vec<&[StyledWord<'b>]>,
) {
    let mut curr_style: &Vec<Style> = &vec![];
    let mut buf: Vec<u8>;
    // Detach all children
    for c in paragraph.children() {
        c.detach();
    }
    for line in wrapped {
        // for word in line
        // if style has changed
        //
        // create appropriate node, move buffer into node, clear buffer, set style to new style,
        // continue
        //
        // if line is over
        // create appropriate node, move buffer into node, add softbreak, clear buffer, continue
        buf = vec![];
        for word in line {
            if !word.has_style(curr_style) {
                if word.style.is_empty() {
                    buf.pop();
                }
                let n = create_node_with_style(curr_style, buf, arena);
                paragraph.append(n);
                buf = vec![];
            }
            buf.extend(word.str().bytes());
            buf.push(b' ');
            curr_style = word.style;
        }
        buf.pop();
        let n = create_node_with_style(curr_style, buf, arena);
        paragraph.append(n);
        // add soft break
        paragraph.append(arena.alloc(Node::new(RefCell::new(Ast::new(NodeValue::SoftBreak)))));
    }
}

fn create_node_with_style<'a>(
    styles: &Vec<Style>,
    buf: Vec<u8>,
    arena: &'a Arena<AstNode<'a>>,
) -> &'a AstNode<'a> {
    let child: &AstNode = arena.alloc(Node::new(RefCell::new(Ast::new(NodeValue::Text(buf)))));
    if styles.is_empty() {
        return child;
    }

    let mut child = child;
    for style in styles.iter().rev() {
        let parent = arena.alloc(style.to_node());
        parent.append(child);
        child = parent;
    }

    return child;
}
fn tagged_string_from_node<'a>(
    node: &'a AstNode<'a>,
    opt: &Options,
    ts: &mut TaggedString,
    context: &mut Vec<Style>,
) -> Result<(), Utf8Error> {
    for child in node.children() {
        let val = &child.data.borrow().value;
        match val {
            NodeValue::Text(text) => {
                let s = str::from_utf8(&text)?;
                let prev_len = ts.buf.len();
                ts.buf.push_str(s);
                ts.styles.push((prev_len, context.clone()));
            }
            NodeValue::SoftBreak => {} // this ensures we re-wrap text
            NodeValue::Emph => {
                context.push(Style::Emph);
                tagged_string_from_node(child, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Strong => {
                context.push(Style::Strong);
                tagged_string_from_node(child, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Strikethrough => {
                context.push(Style::Strikethrough);
                tagged_string_from_node(child, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Superscript => {
                context.push(Style::Superscript);
                tagged_string_from_node(child, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Link(link) => {}
            NodeValue::Image(_link) => {}
            NodeValue::FootnoteReference(_name) => {}
            _ => panic!(),
        }
    }

    Ok(())
}

fn format_ast<'a>(node: &'a AstNode<'a>, arena: &'a Arena<AstNode<'a>>, opt: &Options) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            let mut ts = TaggedString::new();
            tagged_string_from_node(node, opt, &mut ts, &mut vec![]).unwrap();
            let words = tagged_string_to_word_vec(&ts);
            let wrapped =
                wrap_optimal_fit(&words, &[opt.line_width as f64], &Penalties::new()).unwrap();
            replace_paragraph_children(node, arena, opt, wrapped);
        }
        _ => (),
    }

    for n in node.children() {
        format_ast(n, arena, opt);
    }
}

fn tagged_string_to_word_vec<'a>(ts: &'a TaggedString) -> Vec<StyledWord<'a>> {
    // TODO: reserve?
    let mut words = vec![];
    let sep = WordSeparator::new();

    let mut styles = ts.styles.iter();
    let mut prev_style = styles.next().unwrap();
    for style in styles {
        words.extend(
            sep.find_words(&ts.buf[prev_style.0..style.0])
                .map(move |w| StyledWord::from(w, &prev_style.1)),
        );
        prev_style = style;
    }

    words.extend(
        sep.find_words(&ts.buf[prev_style.0..])
            .map(move |w| StyledWord::from(w, &prev_style.1)),
    );

    words
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
    let root = parse_document(&arena, &buffer, &ComrakOptions::default());
    println!("{:#?}", root);
    format_ast(&root, &arena, &options);
    format_commonmark(root, &ComrakOptions::default(), &mut outfile)?;

    // let mut buffer = String::new();
    // infile.read_to_string(&mut buffer)?;

    // let buffer = buffer.trim();
    // let mut outstream = BufWriter::new(outfile);
    // outstream.flush().unwrap();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_but_ignore_code() {
        assert!(is_expected(1));
    }

    #[test]
    fn keep_emph_and_strong() {
        assert!(is_expected(2));
    }

    #[test]
    fn block_quotes() {
        assert!(is_expected(3));
    }

    #[test]
    fn rewrap_paragraphs() {
        assert!(is_expected(4));
    }

    #[test]
    fn multiline_emph() {
        assert!(is_expected(5));
    }

    fn is_expected(testno: u32) -> bool {
        let input = fs::read_to_string(format!("tests/test{}.md", testno)).unwrap();
        let expected = fs::read_to_string(format!("tests/test{}_exp.md", testno)).unwrap();
        let mut output: Vec<u8> = vec![];

        let arena = Arena::new();
        let root = parse_document(&arena, &input, &ComrakOptions::default());
        format_ast(&root, &arena, &Options { line_width: 80 });
        format_commonmark(root, &ComrakOptions::default(), &mut output).unwrap();

        str::from_utf8(&output).unwrap() == expected
    }
}
