use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_commonmark, parse_document, Arena, ComrakOptions};
use std::fs;

use std::fs::File;
use std::io::{self, Read};

use clap::Parser;
use std::str;
use std::str::Utf8Error;
// use std::string::FromUtf8Error;
use textwrap::core::{Fragment, Word};
use textwrap::wrap_algorithms::{wrap_optimal_fit, Penalties};
use textwrap::{self, WordSeparator};

#[derive(Debug, Clone)]
enum WordStyle {
    // Normal,
    Emph,
    Strong,
    Strikethrough,
    Superscript,
}

impl WordStyle {
    fn added_width(&self) -> usize {
        match self {
            Self::Emph => 2,          // *__*
            Self::Strong => 4,        // **__**
            Self::Strikethrough => 4, // ~~__~~
            Self::Superscript => 11,  // <sup>__</sup>
        }
    }
}

#[derive(Debug)]
struct StyledWord<'a> {
    word: Word<'a>,
    style: &'a Vec<WordStyle>,
}

impl<'a> StyledWord<'a> {
    fn from(word: Word<'a>, style: &'a Vec<WordStyle>) -> Self {
        Self { word, style }
    }
}

impl Fragment for StyledWord<'_> {
    fn width(&self) -> f64 {
        self.word.width() + (self.style.iter().map(|s| s.added_width()).sum::<usize>() as f64)
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
    styles: Vec<(usize, Vec<WordStyle>)>,
}

impl TaggedString {
    fn new() -> Self {
        Self {
            buf: String::new(),
            styles: vec![],
        }
    }
}
fn tag_paragraph_with_styles<'a>(
    paragraph: &'a AstNode<'a>,
    arena: &'a Arena<AstNode<'a>>,
    opt: &Options,
    ts: &mut TaggedString,
    context: &mut Vec<WordStyle>,
) -> Result<(), Utf8Error> {
    let text_elems = paragraph.children();

    for n in text_elems {
        let val = &n.data.borrow().value;
        match val {
            NodeValue::Text(text) => {
                let s = str::from_utf8(&text)?;
                let prev_len = ts.buf.len();
                ts.buf.push_str(s);
                ts.styles.push((prev_len, context.clone()));
            }
            NodeValue::Emph => {
                context.push(WordStyle::Emph);
                tag_paragraph_with_styles(n, arena, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Strong => {
                context.push(WordStyle::Strong);
                tag_paragraph_with_styles(n, arena, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Strikethrough => {
                context.push(WordStyle::Strikethrough);
                tag_paragraph_with_styles(n, arena, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Superscript => {
                context.push(WordStyle::Superscript);
                tag_paragraph_with_styles(n, arena, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Link(_link) => {}
            NodeValue::Image(_link) => {}
            NodeValue::FootnoteReference(_name) => {}
            _ => panic!(),
        }
    }

    // let lines = wrap_text_elems(&mut text_elems, opt).unwrap();
    // let mut wrapped = lines.into();
    //
    // for line in paragraph.children() {
    //     line.detach();
    // }
    //
    // replace_paragraph_children(paragraph, &mut wrapped, arena);
    Ok(())
}

// fn replace_paragraph_children<'a, 'b>(
//     paragraph: &'a AstNode<'a>,
//     wrapped: &'b mut WrappedLines,
//     arena: &'a Arena<AstNode<'a>>,
// ) {
//     for line in wrapped.lines() {
//         let a = Ast::new(NodeValue::Text(line.as_bytes().to_vec()));
//         let text = arena.alloc(Node::new(RefCell::new(a)));
//         let a = Ast::new(NodeValue::SoftBreak);
//         let newline = arena.alloc(Node::new(RefCell::new(a)));
//         paragraph.append(text);
//         paragraph.append(newline);
//     }
// }

fn wrap_node<'a>(node: &'a AstNode<'a>, arena: &'a Arena<AstNode<'a>>, opt: &Options) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            let mut ts = TaggedString::new();
            tag_paragraph_with_styles(node, arena, opt, &mut ts, &mut vec![]).unwrap();
            let words = tagged_string_to_word_vec(&ts);
            let wrapped =
                wrap_optimal_fit(&words, &[opt.line_width as f64], &Penalties::new()).unwrap();
            println!("{:#?}", wrapped);
            // println!("buf: {}", ts.buf);
            // println!("styles: {:?}", ts.styles);
        }
        // NodeValue::Heading(_) => (),
        // v => println!("node: {:?}", v),
        _ => (),
    }

    for n in node.children() {
        println!("child of {:?}", node.data.borrow().value);
        wrap_node(n, arena, opt);
        println!("END child of {:?}", node.data.borrow().value);
    }
}

fn tagged_string_to_word_vec<'a>(ts: &'a TaggedString) -> Vec<StyledWord<'a>> {
    let mut words = vec![];
    let sep = WordSeparator::new();

    let mut styles = ts.styles.iter().peekable();
    let mut prev_style = styles.next().unwrap();
    loop {
        if let Some(style) = styles.next() {
            words.extend(
                sep.find_words(&ts.buf[prev_style.0..style.0])
                    .map(move |w| StyledWord::from(w, &prev_style.1)),
            );
            prev_style = style;
        } else {
            words.extend(
                sep.find_words(&ts.buf)
                    .map(move |w| StyledWord::from(w, &prev_style.1)),
            );
            break;
        }
    }

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
