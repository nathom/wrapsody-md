use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::str::{self, Utf8Error};

use clap::Parser;
use comrak::arena_tree::Node;
use comrak::nodes::{Ast, AstNode, NodeLink, NodeValue};
use comrak::{format_commonmark, parse_document, Arena, ComrakOptions};
use itertools::{EitherOrBoth, Itertools};
use textwrap::core::{Fragment, Word};
use textwrap::wrap_algorithms::{wrap_optimal_fit, Penalties};
use textwrap::{self, WordSeparator};

/// Passible word styles
#[derive(Debug, Clone, PartialEq)]
enum Style {
    Emph,   // italics
    Strong, // bold
    Strikethrough,
    Superscript,
    Link(Vec<u8>),
    Image(Vec<u8>),
}

impl Style {
    fn to_node<'a>(&self) -> AstNode<'a> {
        let nv = match self {
            Style::Emph => NodeValue::Emph,
            Style::Strong => NodeValue::Strong,
            Style::Strikethrough => NodeValue::Strikethrough,
            Style::Superscript => NodeValue::Superscript,
            Style::Link(url) => NodeValue::Link(NodeLink {
                url: url.to_vec(),
                title: vec![],
            }),
            Style::Image(url) => NodeValue::Image(NodeLink {
                url: url.to_vec(),
                title: vec![],
            }),
        };
        Node::new(RefCell::new(Ast::new(nv)))
    }

    fn left_width(&self) -> usize {
        match self {
            Style::Emph => 1,
            Style::Strong => 2,
            Style::Strikethrough => 2,
            Style::Superscript => 4,
            Style::Link(_) => 1,
            Style::Image(_) => 2,
        }
    }

    fn right_width(&self) -> usize {
        match self {
            Style::Emph => 1,
            Style::Strong => 2,
            Style::Strikethrough => 2,
            Style::Superscript => 5,                // </sup>
            Style::Link(url) => 2 + url.len() + 1,  // ](url)
            Style::Image(url) => 2 + url.len() + 1, // ](url)
        }
    }
}

/// This is the struct that represents one word, which is moved around
/// by the textwrap library
///
/// StyledWord.style.is_empty() => normal text
/// Otherwise it is a list of styles that the string has
#[derive(Debug, PartialEq)]
struct StyledWord<'a> {
    word: Word<'a>,
    style: &'a Vec<Style>,
    style_width: usize,
}

impl<'a> StyledWord<'a> {
    fn from(mut word: Word<'a>, style: &'a Vec<Style>) -> Self {
        word.whitespace = " ";
        Self {
            word,
            style,
            style_width: 0,
        }
    }

    fn str(&self) -> &'a str {
        self.word.word
    }

    fn _len(&self) -> usize {
        self.word.word.len()
    }

    fn has_style(&self, s: &Vec<Style>) -> bool {
        self.style == s
    }

    fn add_width(&mut self, w: usize) {
        self.style_width += w;
    }
}

// Basically inheriting the Fragment trait from the Word member
// Allows StyledWord to be wrapped
impl Fragment for StyledWord<'_> {
    fn width(&self) -> f64 {
        self.word.width() + self.style_width as f64
    }
    fn whitespace_width(&self) -> f64 {
        self.word.whitespace_width()
    }
    fn penalty_width(&self) -> f64 {
        self.word.penalty_width()
    }
}

// Options passed in by the user
// WIP
struct Options {
    /// Maximum line width
    line_width: usize,
}

/// This is a more efficient intermediate representation
/// of Vec<StyledWord>
#[derive(Debug, PartialEq)]
struct TaggedString {
    /// All words joined with a space
    buf: String,
    /// Vec of (starting index, style)
    ///
    /// For example [(0, []), (5, [Emph]), (11, [])] would mean the following
    ///
    /// this is a sample
    /// nnnnneeeeennnnnn
    ///
    /// Where 'n' denotes normal ([]) style and 'e' is Emph
    styles: Vec<(usize, Vec<Style>)>,
}

impl TaggedString {
    fn new() -> Self {
        Self {
            buf: String::new(),
            styles: vec![],
        }
    }

    fn _with_capacity(cap: usize) -> Self {
        Self {
            buf: String::with_capacity(cap),
            styles: vec![],
        }
    }

    fn to_words<'a>(&'a self) -> Vec<StyledWord<'a>> {
        // TODO: reserve?
        let mut ret = vec![];
        let sep = WordSeparator::new();

        let mut styles = self.styles.iter();
        let mut context: Vec<&Style> = vec![];
        let mut prev_style = styles.next().unwrap();
        context.extend(&prev_style.1);
        let mut left_width = context.iter().map(|s| s.left_width()).sum::<usize>();
        for style in styles {
            // let s = "[*link* with many **styles**](url)";

            // find styles that have been removed
            let (num_removed, mut new_context) = Self::calc_diff(&prev_style.1, &style.1);
            let right_width = (0..num_removed)
                .map(|_| context.pop().unwrap().right_width())
                .sum::<usize>();
            //

            let mut words: Vec<StyledWord> = sep
                .find_words(&self.buf[prev_style.0..style.0])
                .filter(move |w| w.width() > 0.0)
                .map(move |w| StyledWord::from(w, &prev_style.1))
                .collect();

            if words.len() == 0 {
                continue;
            }

            let l = words.len();
            words[0].add_width(left_width);
            words[l - 1].add_width(right_width);

            ret.append(&mut words);

            left_width = new_context.iter().map(|s| s.left_width()).sum::<usize>();
            context.append(&mut new_context);

            // find styles that have been added
            prev_style = style;
        }

        let right_width = context.iter().map(|s| s.right_width()).sum::<usize>();

        let mut words: Vec<StyledWord> = sep
            .find_words(&self.buf[prev_style.0..])
            .filter(move |w| w.width() > 0.0)
            .map(move |w| StyledWord::from(w, &prev_style.1))
            .collect();

        debug_assert!(words.len() > 0);

        let l = words.len();
        words[0].add_width(left_width);
        words[l - 1].add_width(right_width);

        ret.append(&mut words);

        ret
    }

    fn calc_diff<'a>(prev: &'a Vec<Style>, curr: &'a Vec<Style>) -> (usize, Vec<&'a Style>) {
        prev.iter().zip_longest(curr.iter()).fold(
            (0, vec![]),
            |(num_removed, mut new_context), lr| match lr {
                EitherOrBoth::Left(_) => (num_removed + 1, new_context),
                EitherOrBoth::Right(r) => {
                    new_context.push(r);
                    (num_removed, new_context)
                }
                EitherOrBoth::Both(l, r) => {
                    if l != r {
                        new_context.push(r);
                        (num_removed + 1, new_context)
                    } else {
                        (num_removed, new_context)
                    }
                }
            },
        )
    }
}

/// Re-"block" together the style values of each word in a folded line
fn fold_styles<'a>(words: &[StyledWord<'a>]) -> Vec<(usize, &'a Vec<Style>)> {
    let mut ret = vec![];
    let mut it = words.iter();
    let prev_w = it.next().unwrap();

    ret.push((0, prev_w.style));

    it.fold((1, prev_w), |(idx, pw), w| {
        if !w.has_style(pw.style) {
            ret.push((idx, w.style));
        }
        (idx + 1, w)
    });

    ret
}

fn build_node<'a, 'b>(
    styles: &[(usize, &'b Vec<Style>)],
    line: &[StyledWord<'b>],
    arena: &'a Arena<AstNode<'a>>,
    parent: &'a AstNode<'a>,
    layer: usize,
) {
    // partition into plain text and styled text parts
    let mut styles_it = styles.iter().enumerate();
    let (_, &(mut pi, mut ps)) = styles_it.next().unwrap();
    for (j, &(i, s)) in styles_it {
        if ps.get(layer) != s.get(layer) {
            // style has switched from text -> style or style -> text
            match ps.get(layer) {
                None => {
                    // this means a plain text section has just ended
                    // so we add a plain text node
                    let buf = words_to_vec_u8(&line[pi..i], j > 1, true);
                    let child: &AstNode =
                        arena.alloc(Node::new(RefCell::new(Ast::new(NodeValue::Text(buf)))));
                    parent.append(child);
                }
                Some(s) => {
                    // this means a style section has just ended
                    // we create the appropriate node for the style
                    // and recursively call this function to handle its
                    // children

                    let new_parent = arena.alloc(s.to_node());
                    // delete the style layer we just handled

                    build_node(styles, &line[pi..i], arena, new_parent, layer + 1);

                    parent.append(new_parent);
                }
            }
            (pi, ps) = (i, s);
        }
    }

    match ps.get(layer) {
        None => {
            let buf = words_to_vec_u8(&line[pi..], false, false);
            let child: &AstNode =
                arena.alloc(Node::new(RefCell::new(Ast::new(NodeValue::Text(buf)))));
            parent.append(child);
        }
        Some(s) => {
            let new_parent = arena.alloc(s.to_node());
            // delete the style layer we just handled

            build_node(styles, &line[pi..], arena, new_parent, layer + 1);

            parent.append(new_parent);
        }
    }

    // arena.alloc();
}

fn words_to_vec_u8<'a>(
    line: &[StyledWord<'a>],
    leading_space: bool,
    trailing_space: bool,
) -> Vec<u8> {
    let mut buf = line.iter().map(move |w| w.str()).fold(
        if leading_space { vec![b' '] } else { vec![] },
        |mut buf, s| {
            buf.extend(s.bytes());
            buf.push(b' ');
            buf
        },
    );
    if !trailing_space {
        buf.pop();
    }
    buf
}

/// Given a wrapped 2D vec of StyledWords, replace the paragraph Node's
/// children with the wrapped text, with appropriate formatting
fn replace_paragraph_children<'a, 'b>(
    paragraph: &'a AstNode<'a>,
    arena: &'a Arena<AstNode<'a>>,
    _opt: &Options,
    wrapped: Vec<&[StyledWord<'b>]>,
) {
    // Detach all children
    for c in paragraph.children() {
        c.detach();
    }

    for line in wrapped {
        let styles = fold_styles(line);
        build_node(&styles, line, arena, paragraph, 0);

        let softbreak = Node::new(RefCell::new(Ast::new(NodeValue::SoftBreak)));
        paragraph.append(arena.alloc(softbreak));
    }
}

/// Given a node with text children (i.e. Paragraph) generate a TaggedString
/// that gives you a single string with all text, and metadata telling you
/// the starting positions of styles
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
                if s.trim().len() == 0 {
                    continue;
                }
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
            // FIX: these are broken (test 8)
            // These not being implemented is the cause of the unwrap panic
            // on test 8
            NodeValue::Link(link) => {
                // convert tree into "{title}\0{url}" with Style::Link
                // push that string and style into the buffer
                // let s = str::from_utf8(&link.url)?.to_string();
                context.push(Style::Link(link.url.clone()));
                tagged_string_from_node(child, opt, ts, context)?;
                context.pop();
            }
            NodeValue::Image(link) => {
                context.push(Style::Image(link.url.clone()));
                tagged_string_from_node(child, opt, ts, context)?;
                context.pop();
            }
            NodeValue::FootnoteReference(_name) => {
                todo!()
            }
            _ => todo!(),
        }
    }

    Ok(())
}

fn format_ast<'a>(node: &'a AstNode<'a>, arena: &'a Arena<AstNode<'a>>, opt: &Options) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            let mut ts = TaggedString::new();
            tagged_string_from_node(node, opt, &mut ts, &mut vec![]).unwrap();
            let words = ts.to_words();
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

    let arena = Arena::new();

    // Build AST
    let root = parse_document(&arena, &buffer, &ComrakOptions::default());

    // Format AST
    format_ast(&root, &arena, &options);

    // Write AST to output
    match args.outfile {
        Some(filename) => {
            let mut outfile = BufWriter::new(File::open(filename)?);
            format_commonmark(root, &ComrakOptions::default(), &mut outfile)?;
            outfile.flush()?;
        }
        None => {
            let mut outfile = BufWriter::new(io::stdout());
            format_commonmark(root, &ComrakOptions::default(), &mut outfile)?;
            outfile.flush()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_but_ignore_code() {
        run_test_file_no(1);
    }

    #[test]
    fn keep_emph_and_strong() {
        run_test_file_no(2);
    }

    #[test]
    fn block_quotes() {
        run_test_file_no(3);
    }

    #[test]
    fn rewrap_paragraphs() {
        run_test_file_no(4);
    }

    #[test]
    fn multiline_emph() {
        run_test_file_no(5);
    }

    #[test]
    fn ignore_link_paragraphs() {
        run_test_file_no(8);
    }

    #[test]
    fn style_within_link_preserved() {
        run_test_file_no(9);
    }

    #[test]
    fn short_list_elements_preserved() {
        run_test_file_no(11);
    }

    #[test]
    fn long_link_title_gets_wrapped() {
        run_test_file_no(12);
    }

    #[test]
    fn link_title_with_newline_gets_rewrapped() {
        run_test_file_no(13);
    }

    #[test]
    fn link_word_lengths() {
        let s = "[*link* with many **styles**](url)";
        let ts = tagged_string_from(s);
        let wv = ts.to_words();
        assert_eq!(
            wv.iter().map(|w| w.width() as usize).collect::<Vec<_>>(),
            vec![7, 4, 4, 16]
        );
    }

    #[test]
    fn italic_word_width_single() {
        let s1 = "*word*";
        let ts = tagged_string_from(s1);
        let wv = ts.to_words();
        assert_eq!(
            wv.iter().map(|w| w.width() as usize).collect::<Vec<_>>(),
            vec![6]
        );
    }

    #[test]
    fn italic_word_width_double() {
        let s1 = "*word word*";
        let ts = tagged_string_from(s1);
        let wv = ts.to_words();
        assert_eq!(
            wv.iter().map(|w| w.width() as usize).collect::<Vec<_>>(),
            vec![5, 5]
        );
    }

    #[test]
    fn italic_word_width_triple() {
        let s1 = "*word word word*";
        let ts = tagged_string_from(s1);
        let wv = ts.to_words();
        assert_eq!(
            wv.iter().map(|w| w.width() as usize).collect::<Vec<_>>(),
            vec![5, 4, 5]
        );
    }

    #[test]
    fn adjacent_italics_get_merged() {
        run_test_file_no(14);
    }

    #[test]
    fn adjacent_boldface_get_merged() {
        run_test_file_no(15);
    }

    fn tagged_string_from(s: &str) -> TaggedString {
        let arena = Arena::new();
        let root = parse_document(&arena, &s, &ComrakOptions::default());
        let paragraph = root.children().next().unwrap();
        let mut ts = TaggedString::new();
        let mut ctx = vec![];
        tagged_string_from_node(&paragraph, &Options { line_width: 80 }, &mut ts, &mut ctx)
            .unwrap();
        ts
    }

    fn run_test_file_no(testno: u32) {
        let input = fs::read_to_string(format!("tests/test{}.md", testno)).unwrap();
        let expected = fs::read_to_string(format!("tests/test{}_exp.md", testno)).unwrap();
        let mut output: Vec<u8> = vec![];

        let arena = Arena::new();
        let root = parse_document(&arena, &input, &ComrakOptions::default());
        format_ast(&root, &arena, &Options { line_width: 80 });
        format_commonmark(root, &ComrakOptions::default(), &mut output).unwrap();

        let out = str::from_utf8(&output).unwrap();
        assert_eq!(out, expected)
    }
}
