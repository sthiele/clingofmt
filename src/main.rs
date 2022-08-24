use anyhow::{Context, Result};
use clap::Parser;
use log::{debug, error, warn};
use std::fs;
use std::{
    io::{self, Write},
    path::PathBuf,
};

const MAX_LENGTH: usize = 60;

/// Format clingo code
#[derive(Parser, Debug)]
#[clap(version, author)]
struct Opt {
    /// Input file in clingo format
    #[clap(name = "FILE", parse(from_os_str))]
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
    stderrlog::new()
        .module(module_path!())
        .verbosity(3)
        .init()
        .unwrap();
    if let Err(err) = run() {
        error!("{:?}", err);
        std::process::exit(1);
    }
}

fn line_flush_stmt(out: &mut dyn Write, buf: &mut String) -> Result<()> {
    if let Some(last) = buf.pop() {
        if last != ' ' {
            write!(out, "{buf}{last}")?;
        } else {
            write!(out, "{buf}")?;
        }
        buf.clear();
    }
    Ok(())
}
fn line_flush_comment(out: &mut dyn Write, buf: &mut String) -> Result<()> {
    write!(out, "{}", buf.trim_end())?;
    buf.clear();
    Ok(())
}
fn indent(buf: &mut String, ident_level: usize) -> Result<()> {
    for _i in 0..ident_level {
        buf.push_str("    ");
    }
    Ok(())
}
fn run() -> Result<()> {
    let opt = Opt::parse();

    let path = opt.file.to_str().unwrap();
    let source_code =
        fs::read(path).with_context(|| format!("Error reading source file {}", path))?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(tree_sitter_clingo::language())
        .expect("Error loading clingo grammar");
    let tree = parser.parse(&source_code, None).unwrap();

    let mut buf = Vec::new();
    pass_one(&tree, &source_code, &mut buf, opt.debug)?;

    let mut out = std::io::stdout();
    let buf_str: String = String::from_utf8(buf)?;
    write!(out, "{buf_str}")?;
    Ok(())
}

fn flush_in_between(out: &mut dyn Write, buf: &mut String, state: &mut State) -> Result<()> {
    if !state.is_in_between {
        let is_fact = state.has_head_like & !state.has_body & !state.has_if;

        if state.in_fact_block {
            if !is_fact {
                writeln!(out)?;
                writeln!(out)?;
            } else {
                write!(out, " ")?;
            }
        } else if !state.in_block {
            writeln!(out)?;
        }
        line_flush_comment(out, buf)?;
    } else {
        line_flush_comment(out, buf)?;
    }
    writeln!(out)?;
    state.is_in_between = true;
    Ok(())
}

struct State {
    is_in_between: bool,
    in_fact_block: bool,
    in_block: bool,
    has_head_like: bool,
    has_if: bool,
    has_body: bool,
    is_in_statement: bool,
    in_conjunction: bool,
    in_optcondition: bool,
    in_termvec: usize,
    in_theory_atom_definition: bool,
}

fn pass_one(
    tree: &tree_sitter::Tree,
    source_code: &[u8],
    out: &mut dyn Write,
    debug: bool,
) -> Result<()> {
    let mut state = State {
        is_in_between: false,
        in_fact_block: false,
        in_block: true,
        has_head_like: false,
        has_if: false,
        has_body: false,
        is_in_statement: false,
        in_conjunction: false,
        in_optcondition: false,
        in_termvec: 0,
        in_theory_atom_definition: false,
    };
    let mut has_errors = false;
    let mut cursor = tree.walk();

    let mut buf = String::new();
    let mut indent_level = 0;
    let mut mindent_level = 0;
    let mut did_visit_children = false;

    loop {
        let node = cursor.node();
        let is_named = node.is_named();
        if !did_visit_children {
            // what happens before the element
            if node.is_error() {
                has_errors = true;
                buf.clear();
                let start = node.start_position();
                let end = node.end_position();
                let text =
                    std::str::from_utf8(&source_code[node.start_byte()..node.end_byte()]).unwrap();

                warn!(
                    "SYNTAX ERROR at [{}, {}] - [{}, {}]",
                    start.row, start.column, end.row, end.column
                );
                warn!("{text}");
                if node.is_missing() {
                    if node.is_named() {
                        warn!("MISSING {}", node.kind());
                    } else {
                        warn!("MISSING \"{}\"", node.kind().replace('\n', "\\n"));
                    }
                }
                did_visit_children = true;
            } else {
                if is_named {
                    match node.kind() {
                        "statement" => {
                            state.has_head_like = false;
                            state.has_body = false;
                            state.is_in_statement = true;
                        }
                        "head" | "EDGE" => state.has_head_like = true,
                        "bodydot" => {
                            state.has_body = true;
                        }
                        "optcondition" | "optimizecond" => {
                            state.in_optcondition = true;
                            //incease mindent_level after COLON
                        }
                        "conjunction" => {
                            state.in_conjunction = true;
                            //incease mindent_level after COLON
                        }
                        "termvec" | "binaryargvec" => {
                            state.in_termvec += 1;
                        }
                        "theory_atom_definition" => {
                            state.in_termvec += 1;
                            state.in_theory_atom_definition = true;
                        }
                        "LBRACK" => {
                            buf.push(' ');
                            state.in_termvec += 1;
                            mindent_level += 1;
                        }
                        // cosmetic whitespace
                        "IF" => buf.push(' '),
                        "VBAR" | "cmp" | "COLON" => buf.push(' '),
                        "RBRACE" => {
                            if state.in_theory_atom_definition {
                                buf.push(' ');
                            } else {
                                mindent_level -= 1;
                                flush_in_between(out, &mut buf, &mut state)?;
                                indent(&mut buf, mindent_level)?;
                            }
                        }
                        "RPAREN" => {
                            mindent_level -= 1;
                        }
                        _ => {}
                    }
                }
                if debug {
                    let indent = "  ".repeat(indent_level);
                    let start = node.start_position();
                    let end = node.end_position();
                    if let Some(field_name) = cursor.field_name() {
                        debug!("{}: ", field_name);
                    }

                    debug!(
                        "{}({} [{}, {}] - [{}, {}]",
                        indent,
                        node.kind(),
                        start.row,
                        start.column,
                        end.row,
                        end.column
                    );
                }
                if cursor.goto_first_child() {
                    did_visit_children = false;
                    indent_level += 1;
                } else {
                    did_visit_children = true;
                }
            }
        } else {
            // what happens after the element

            // write token to buffer
            if node.child_count() == 0 {
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                let text = std::str::from_utf8(&source_code[start_byte..end_byte]).unwrap();

                buf.push_str(text);
            }

            if is_named {
                match node.kind() {
                    "source_file" => {
                        if state.in_fact_block {
                            writeln!(out)?;
                        }
                    }
                    "statement" => {
                        if !state.is_in_between {
                            let is_fact = state.has_head_like & !state.has_body;

                            if state.in_fact_block {
                                if !is_fact {
                                    writeln!(out)?;
                                    writeln!(out)?;
                                } else {
                                    write!(out, " ")?;
                                }
                            } else if !state.in_block {
                                writeln!(out)?;
                            }
                            if is_fact {
                                state.in_fact_block = true
                            } else {
                                state.in_fact_block = false;
                            }

                            line_flush_stmt(out, &mut buf)?;

                            if !state.in_fact_block {
                                writeln!(out)?;
                            }

                            state.in_block = false;
                        } else {
                            let is_fact = state.has_head_like & !state.has_body;
                            if is_fact {
                                state.in_fact_block = true
                            } else {
                                state.in_fact_block = false;
                            }

                            line_flush_stmt(out, &mut buf)?;

                            if !state.in_fact_block {
                                writeln!(out)?;
                            }

                            state.in_block = false;
                        }
                        state.is_in_between = false;
                        state.is_in_statement = false;
                    }
                    "single_comment" => {
                        if state.is_in_statement {
                            flush_in_between(out, &mut buf, &mut state)?;
                            indent(&mut buf, mindent_level)?;
                        } else {
                            if state.in_fact_block {
                                writeln!(out)?;
                                writeln!(out)?;
                            } else if !state.in_block {
                                writeln!(out)?;
                            }
                            line_flush_comment(out, &mut buf)?;

                            writeln!(out)?;
                            state.in_fact_block = false;
                            state.in_block = true;
                        }
                    }
                    "multi_comment" => {
                        if !state.is_in_statement {
                            if state.in_fact_block {
                                writeln!(out)?;
                                writeln!(out)?;
                            } else if !state.in_block {
                                writeln!(out)?;
                            }
                            line_flush_comment(out, &mut buf)?;

                            writeln!(out)?;
                            state.in_fact_block = false;
                            state.in_block = true;
                        }
                    }
                    "termvec" | "binaryargvec" => {
                        state.in_termvec -= 1;
                    }
                    "theory_atom_definition" => {
                        state.in_termvec -= 1;
                        state.in_theory_atom_definition = false;
                    }
                    "RBRACK" => {
                        mindent_level -= 1;
                        state.in_termvec -= 1;
                    }
                    "bodydot" => {
                        if state.has_if {
                            mindent_level -= 1;
                            state.has_if = false;
                        }
                    }
                    "optcondition" | "optimizecond" => {
                        state.in_optcondition = false;
                        mindent_level -= 1;
                    }
                    "conjunction" => {
                        state.in_conjunction = false;
                        mindent_level -= 1;
                    }
                    "LPAREN" => {
                        mindent_level += 1;
                    }
                    // Add semantic space
                    "NOT" | "aggregatefunction" | "theory_identifier" | "EXTERNAL" | "DEFINED"
                    | "CONST" | "SHOW" | "BLOCK" | "INCLUDE" | "PROJECT" | "HEURISTIC"
                    | "THEORY" | "MAXIMIZE" | "MINIMIZE" => buf.push(' '),
                    // Add cosmetic space
                    "cmp" | "VBAR" => buf.push(' '),

                    "SEM" => {
                        flush_in_between(out, &mut buf, &mut state)?;
                        indent(&mut buf, mindent_level)?;
                    }
                    "COLON" => {
                        if state.in_theory_atom_definition {
                            buf.push(' ');
                        } else {
                            if state.in_conjunction {
                                mindent_level += 1;
                            }
                            if state.in_optcondition {
                                mindent_level += 1;
                            }
                            flush_in_between(out, &mut buf, &mut state)?;
                            indent(&mut buf, mindent_level)?;
                        }
                    }
                    "LBRACE" => {
                        if state.in_theory_atom_definition {
                            buf.push(' ');
                        } else {
                            mindent_level += 1;
                            flush_in_between(out, &mut buf, &mut state)?;
                            indent(&mut buf, mindent_level)?;
                        }
                    }
                    "COMMA" => {
                        if state.in_termvec == 0
                        /*|| buf.len() >= MAX_LENGTH */
                        {
                            flush_in_between(out, &mut buf, &mut state)?;
                            indent(&mut buf, mindent_level)?;
                        } else {
                            buf.push(' ');
                        }
                    }
                    "IF" => {
                        state.has_if = true;
                        mindent_level += 1; // decrease after bodydot
                        if !state.has_head_like {
                            buf.push(' ');
                        } else {
                            flush_in_between(out, &mut buf, &mut state)?;
                            indent(&mut buf, mindent_level)?;
                        }
                    }
                    _ => {}
                }
            }
            if cursor.goto_next_sibling() {
                did_visit_children = false;
            } else if cursor.goto_parent() {
                did_visit_children = true;
                indent_level -= 1;
            } else {
                break;
            }
        }
    }
    if has_errors {
        Err(anyhow::Error::msg("Error while parsing"))
    } else {
        Ok(())
    }
}

/// function to simplify tests
fn _fmt_and_cmp_new(source_code: &str, res: &str) {
    let mut buf = Vec::new();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(tree_sitter_clingo::language())
        .expect("Error loading clingo grammar");

    let tree = parser.parse(&source_code, None).unwrap();

    pass_one(&tree, source_code.as_bytes(), &mut buf, false).unwrap();
    let parse_res = std::str::from_utf8(&buf).unwrap();
    assert_eq!(parse_res, res)
}

#[test]
fn test_pass_new() {
    _fmt_and_cmp_new(" \n \n ", "");
    _fmt_and_cmp_new("% bla blub       ", "% bla blub\n");
    _fmt_and_cmp_new("% bla\n% blub       ", "% bla\n% blub\n");
    _fmt_and_cmp_new(
        "%* multi  \n    line\n    comment  *%",
        "%* multi  \n    line\n    comment  *%\n",
    );
    _fmt_and_cmp_new(" pred(something).        ", "pred(something).\n");
    _fmt_and_cmp_new(
        " pred(something).     % bla   ",
        "pred(something).\n\n% bla\n",
    );
    _fmt_and_cmp_new("% bla blub\n   a:-b.   ", "% bla blub\na :-\n    b.\n");
    _fmt_and_cmp_new(
        "% fact block\n a(1).\n a(2). a(3).",
        "% fact block\na(1). a(2). a(3).\n",
    );
    _fmt_and_cmp_new(
        "%* fact block *%  \n  a(1).   \na(2). a(3).",
        "%* fact block *%\na(1). a(2). a(3).\n",
    );
    _fmt_and_cmp_new(
        "%* fact block *%  \n  a(1%*bla*%   ).   \na(2). a(3).",
        "%* fact block *%\na(1%*bla*%). a(2). a(3).\n",
    );
    _fmt_and_cmp_new(
        "% fact block1 \n  a(1%*bla*%   ).  \na(2). a(3).%* fact block2 *%  b(1%*bla*%   ).  \nb(2). b(3).",
        "% fact block1\na(1%*bla*%). a(2). a(3).\n\n%* fact block2 *%\nb(1%*bla*%). b(2). b(3).\n",
    );
}

#[test]
fn test_pass_old() {
    let source = r#"% Derive (varying) atoms
atom(A):-model(M),true(M,A).vary(A):-model(M),atom(A),not true(M,A).
% Derive lower bound LB and upper bound UB for size of prime implicants
% - LB: minimum number of varying atoms s.t. interpretations don't exceed models
% - UB: minimum of number of varying atoms and number of non-models
varies(X):-X = #count{ A : vary(A) }.models(Y):-Y = #count{ M : model(M) }.:- models(0). % must have some model
minsize(Y,2**X,0) :-varies(X),models(Y),1 < Y. % nothing varies if one model
minsize(Y,Z/2,L+1):-minsize(Y,Z,L),Y < Z.
bounds(L,(X+F-|X-F|)/2):-varies(X),minsize(Y,Z,L),not minsize(Y,Z/2,L+1),
                          F = 2**X-Y.
% Select literals for prime implicant
  select(A,1)         :-atom(A),not vary(A).{ select(A,0..1) } < 2:-vary(A),not bounds(0,0).
selected(A):-select(A,V),vary(A).
% Check lower and upper bounds via "Sinz counter" on selected varying atoms
index(A,I):-vary(A),I = #count{ B : vary(B),B <= A },not bounds(0,0).
counter(I,1)  :-index(A,I),bounds(L,U),L <= I,selected(A).counter(I,C+1):-index(A,I),bounds(L,U),C < U,selected(A),counter(I+1,C).counter(I,C)  :-index(A,I),bounds(L,U),L < C+I,counter(I+1,C).
:- bounds(L,U),0 < L,not counter(1,L).:- bounds(L,U),index(A,I),selected(A),counter(I+1,U).
% Derive models excluded by (some) selected literal
exclude(M,A):-model(M),select(A,0),true(M,A).exclude(M,A):-model(M),select(A,1),not true(M,A).
excluded(M):-exclude(M,A).
% Check that all interpretations extending prime implicant are models
:- bounds(L,U),varies(X),models(Y),
   #sum{ 2**(X-Z) : Z = L+1..X,not counter(1,Z);
              1,M : excluded(M) } >= Y.
% Check that removing any literal of prime implicant yields some non-model
:- bounds(L,U),varies(X),models(Y),index(A,I),   #sum{ 2**(X-Z) : Z = L..X,not counter(1,Z+1);   1,M : exclude(M,B),B != A } < Y.
% Display literals of prime implicant
#show select/2.
#show a(A) : b(A), field(AN).
#show select("root",X).
              n(s).bb(x).
output(@fmt(("The @fmt() function is flexible enough to take multi-line ",
             "strings containing many placeholders: {} and ",
             "{} and {} outputs"), (X,Y,Z))) :- num(X),string(Y),constant(Z).
sel_vat(H, V) :- sel_vat(N,W) : cons(Identifier,var(N,W));
   subgraph(N) : cons(Identifier, has_x("strong",N))%*jjj*%;
   %c1
   has_con(F, T, Na, Index) 
   %c0
   %c01
   : cons(Identifier, has_con("strong", F, T, Na, Index));
   %c2
   %c3
   not has_con(F, _, Na, Index) : cons(Identifier, has_con("weak", F, Na, Index));  %* c2
    sss *%
   cons(Identifier,tail(H,V)).

    bla%aa 
    %bb  
    :-%aa 
    %bb
     varies(X),
    #sum %aa 
    %bb
    { %aa 
    %bb
        2**(X-Z)%aa 
    %bb 
    :%aa 
    %bb
            Z = L+1..X,%aa 
    %bb
            not counter(1, Z); %aa 
    %bb
        1, M :
            excluded(M)
    } %aa 
    %bb
     >= %aa 
    %bb
     Y %aa 
    %bb
    ,models(Y).

:- bla(1,2).
#external a.
#const c= "Dd".
#minimize{fff}.
#maximise {X:ccc(X)}.
#include "fail1.lp".
#heuristic heu.[ha,ak]
#defined bla/2.
#project p("s",X).

#theory test {
    &q/1 : t, body;
    &r/0 : t, { < }, t, directive
}.
#edge(a,b).#edge(c,d).

"#;
    let result = r#"% Derive (varying) atoms
atom(A) :-
    model(M),
    true(M, A).

vary(A) :-
    model(M),
    atom(A),
    not true(M, A).

% Derive lower bound LB and upper bound UB for size of prime implicants
% - LB: minimum number of varying atoms s.t. interpretations don't exceed models
% - UB: minimum of number of varying atoms and number of non-models
varies(X) :-
    X = #count {
        A :
            vary(A)
    }.

models(Y) :-
    Y = #count {
        M :
            model(M)
    }.

 :- models(0).

% must have some model
minsize(Y, 2**X, 0) :-
    varies(X),
    models(Y),
    1 < Y.

% nothing varies if one model
minsize(Y, Z/2, L+1) :-
    minsize(Y, Z, L),
    Y < Z.

bounds(L, (X+F- | X-F | )/2) :-
    varies(X),
    minsize(Y, Z, L),
    not minsize(Y, Z/2, L+1),
    F = 2**X-Y.

% Select literals for prime implicant
select(A, 1) :-
    atom(A),
    not vary(A).

{
    select(A, 0..1)
} < 2 :-
    vary(A),
    not bounds(0, 0).

selected(A) :-
    select(A, V),
    vary(A).

% Check lower and upper bounds via "Sinz counter" on selected varying atoms
index(A, I) :-
    vary(A),
    I = #count {
        B :
            vary(B),
            B <= A
    },
    not bounds(0, 0).

counter(I, 1) :-
    index(A, I),
    bounds(L, U),
    L <= I,
    selected(A).

counter(I, C+1) :-
    index(A, I),
    bounds(L, U),
    C < U,
    selected(A),
    counter(I+1, C).

counter(I, C) :-
    index(A, I),
    bounds(L, U),
    L < C+I,
    counter(I+1, C).

 :- bounds(L, U),
    0 < L,
    not counter(1, L).

 :- bounds(L, U),
    index(A, I),
    selected(A),
    counter(I+1, U).

% Derive models excluded by (some) selected literal
exclude(M, A) :-
    model(M),
    select(A, 0),
    true(M, A).

exclude(M, A) :-
    model(M),
    select(A, 1),
    not true(M, A).

excluded(M) :-
    exclude(M, A).

% Check that all interpretations extending prime implicant are models
 :- bounds(L, U),
    varies(X),
    models(Y),
    #sum {
        2**(X-Z) :
            Z = L+1..X,
            not counter(1, Z);
        1, M :
            excluded(M)
    } >= Y.

% Check that removing any literal of prime implicant yields some non-model
 :- bounds(L, U),
    varies(X),
    models(Y),
    index(A, I),
    #sum {
        2**(X-Z) :
            Z = L..X,
            not counter(1, Z+1);
        1, M :
            exclude(M, B),
            B != A
    } < Y.

% Display literals of prime implicant
#show select/2.

#show a(A) :
b(A),
field(AN).

#show select("root", X).

n(s). bb(x).

output(@fmt(("The @fmt() function is flexible enough to take multi-line ", "strings containing many placeholders: {} and ", "{} and {} outputs"), (X, Y, Z))) :-
    num(X),
    string(Y),
    constant(Z).

sel_vat(H, V) :-
    sel_vat(N, W) :
        cons(Identifier, var(N, W));
    subgraph(N) :
        cons(Identifier, has_x("strong", N))%*jjj*%;
    %c1
    has_con(F, T, Na, Index)%c0
    %c01
     :
        cons(Identifier, has_con("strong", F, T, Na, Index));
    %c2
    %c3
    not has_con(F, _, Na, Index) :
        cons(Identifier, has_con("weak", F, Na, Index));
    %* c2
    sss *%cons(Identifier, tail(H, V)).

bla%aa
%bb
 :-
    %aa
    %bb
    varies(X),
    #sum %aa
    %bb
    {
        %aa
        %bb
        2**(X-Z)%aa
        %bb
         :
            %aa
            %bb
            Z = L+1..X,
            %aa
            %bb
            not counter(1, Z);
        %aa
        %bb
        1, M :
            excluded(M)
    }%aa
    %bb
     >= %aa
    %bb
    Y%aa
    %bb
    ,
    models(Y).

 :- bla(1, 2).

#external a.

#const c="Dd".

#minimize {
    fff
}.

#maximise {
    X :
        ccc(X)
}.

#include "fail1.lp".

#heuristic heu. [ha, ak]

#defined bla/2.

#project p("s", X).

#theory test {
    &q/1 : t, body;
    &r/0 : t, { < }, t, directive
}.

#edge(a, b). #edge(c, d).
"#;
    _fmt_and_cmp_new(source, result);
}
