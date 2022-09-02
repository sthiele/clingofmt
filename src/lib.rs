use anyhow::Result;
use log::{debug, warn};
use std::io::Write;

#[cfg(test)]
mod tests;
struct State {
    is_show: bool,
    is_include: bool,
    has_head_like: bool,
    has_if: bool,
    has_body: bool,
    in_conjunction: bool,
    in_optcondition: bool,
    in_termvec: usize,
    in_theory_atom_definition: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum StatementType {
    Fact,
    Show,    // show statement
    Include, // include statement
    Other,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FormatterState {
    Block(StatementType),
    Some,
    No,
}
struct Formatter<'a> {
    out: &'a mut dyn Write,
    state: FormatterState,
}
impl<'a> Formatter<'a> {
    fn new_block(&mut self, stmt_type: Option<StatementType>) -> Result<()> {
        match self.state {
            FormatterState::No => {}
            FormatterState::Block(StatementType::Other) | FormatterState::Some => {
                writeln!(self.out)?; // empty line before new block
            }
            _ => {
                writeln!(self.out)?; // end the last block
                writeln!(self.out)?; // empty line before new block
            }
        }
        match stmt_type {
            Some(StatementType::Other) => self.state = FormatterState::Block(StatementType::Other),
            Some(StatementType::Fact) => self.state = FormatterState::Block(StatementType::Fact),
            Some(StatementType::Show) => self.state = FormatterState::Block(StatementType::Show),
            Some(StatementType::Include) => {
                self.state = FormatterState::Block(StatementType::Include)
            }
            None => self.state = FormatterState::Some,
        }
        Ok(())
    }
    fn process_comment(&mut self, buf: &[u8]) -> Result<()> {
        match self.state {
            FormatterState::No => {}
            FormatterState::Some => {
                writeln!(self.out)?;
            }
            FormatterState::Block(StatementType::Other) => {
                writeln!(self.out)?;
            }
            _ => self.new_block(None)?,
        };
        let text = std::str::from_utf8(buf).unwrap();
        write!(self.out, "{}", text.trim_end())?;
        self.state = FormatterState::Some;

        Ok(())
    }
    fn process_statement(&mut self, stmt_type: StatementType, buf: &[u8]) -> Result<()> {
        match (self.state, stmt_type) {
            (FormatterState::Block(StatementType::Fact), StatementType::Fact) => {
                write!(self.out, " ")?
            }
            (FormatterState::Block(StatementType::Show), StatementType::Show)
            | (FormatterState::Block(StatementType::Include), StatementType::Include)
            | (FormatterState::Block(StatementType::Other), StatementType::Other) => {
                writeln!(self.out)?
            }

            _ => self.new_block(Some(stmt_type))?,
        }

        let buf_str = std::str::from_utf8(buf)?;
        write!(self.out, "{}", buf_str)?;

        if stmt_type == StatementType::Other {
            writeln!(self.out).unwrap(); // add new line after every rule like statement
        }
        Ok(())
    }
    fn finish_program(&mut self) -> Result<()> {
        if self.state != FormatterState::No
            && self.state != FormatterState::Block(StatementType::Other)
        {
            writeln!(self.out)?;
        }
        Ok(())
    }
}

pub fn format_program(
    tree: &tree_sitter::Tree,
    source_code: &[u8],
    out: &mut dyn Write,
    debug: bool,
) -> Result<()> {
    let mut formatter = Formatter {
        out,
        state: FormatterState::No,
    };
    let mut short_cut = false;
    let mut cursor = tree.walk();
    let has_errors = cursor.node().has_error();

    let mut indent_level = 0;
    let mut did_visit_children = false;

    loop {
        let node = cursor.node();
        if !did_visit_children {
            // What happens before the element
            if node.is_missing() {
                let start = node.start_position();
                if node.is_named() {
                    warn!(
                        "MISSING {} at [{}, {}]",
                        node.kind(),
                        start.row,
                        start.column
                    );
                } else {
                    warn!(
                        "MISSING \"{}\" at [{}, {}]",
                        node.kind().replace('\n', "\\n"),
                        start.row,
                        start.column
                    );
                }
                did_visit_children = true;
            } else if node.is_error() {
                let start = node.start_position();
                let end = node.end_position();
                let text =
                    std::str::from_utf8(&source_code[node.start_byte()..node.end_byte()]).unwrap();

                warn!(
                    "SYNTAX ERROR at [{}, {}] - [{}, {}]",
                    start.row, start.column, end.row, end.column
                );
                warn!("Unexpected: {text}");
                did_visit_children = true;
            } else {
                match node.kind() {
                    "statement" => {
                        let mut buf = Vec::new();
                        let stmt_type = format_statement(&node, source_code, &mut buf, debug)?;

                        formatter.process_statement(stmt_type, &buf)?;
                        short_cut = true;
                    }
                    "single_comment" | "multi_comment" => {
                        let start_byte = node.start_byte();
                        let end_byte = node.end_byte();
                        formatter.process_comment(&source_code[start_byte..end_byte])?;
                    }
                    _ => {}
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
                if short_cut {
                    did_visit_children = true;
                } else if cursor.goto_first_child() {
                    did_visit_children = false;
                    indent_level += 1;
                } else {
                    did_visit_children = true;
                }
            }
        } else {
            // What happens after the element
            match node.kind() {
                "source_file" => {
                    formatter.finish_program()?;
                }
                "statement" => short_cut = false,
                _ => {}
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

fn format_statement(
    node: &tree_sitter::Node,
    source_code: &[u8],
    out: &mut dyn Write,
    debug: bool,
) -> Result<StatementType> {
    let mut flush = false;
    let mut cosmetic_ws = false;
    let mut state = State {
        is_show: false,
        is_include: false,
        has_head_like: false,
        has_if: false,
        has_body: false,
        in_conjunction: false,
        in_optcondition: false,
        in_termvec: 0,
        in_theory_atom_definition: false,
    };
    let mut cursor = node.walk();

    let mut indent_level = 0;
    let mut mindent_level = 0;
    let mut did_visit_children = false;

    loop {
        let node = cursor.node();
        if !did_visit_children {
            // What happens before the element
            if node.is_missing() {
                let start = node.start_position();
                if node.is_named() {
                    warn!(
                        "MISSING {} at [{}, {}]",
                        node.kind(),
                        start.row,
                        start.column
                    );
                } else {
                    warn!(
                        "MISSING \"{}\" at [{}, {}]",
                        node.kind().replace('\n', "\\n"),
                        start.row,
                        start.column
                    );
                }
                did_visit_children = true;
            } else if node.is_error() {
                let start = node.start_position();
                let end = node.end_position();
                let text =
                    std::str::from_utf8(&source_code[node.start_byte()..node.end_byte()]).unwrap();

                warn!(
                    "SYNTAX ERROR at [{}, {}] - [{}, {}]",
                    start.row, start.column, end.row, end.column
                );
                warn!("Unexpected: {text}");
                did_visit_children = true;
            } else {
                match node.kind() {
                    "statement" => {
                        state.has_head_like = false;
                        state.has_body = false;
                    }
                    "head" | "EDGE" => state.has_head_like = true,
                    "bodydot" => state.has_body = true,
                    "optcondition" | "optimizecond" => {
                        state.in_optcondition = true;
                        //incease mindent_level after COLON
                    }
                    "conjunction" => {
                        state.in_conjunction = true;
                        //incease mindent_level after COLON
                    }
                    "termvec" | "binaryargvec" => state.in_termvec += 1,
                    "theory_atom_definition" => {
                        state.in_termvec += 1;
                        state.in_theory_atom_definition = true;
                    }
                    "LBRACK" => {
                        cosmetic_ws = true;
                        state.in_termvec += 1;
                        mindent_level += 1;
                    }
                    "IF" => cosmetic_ws = true,
                    "VBAR" | "cmp" | "COLON" => cosmetic_ws = true,
                    "RBRACE" => {
                        if state.in_theory_atom_definition {
                            cosmetic_ws = true;
                        } else {
                            mindent_level -= 1;
                            flush = true;
                        }
                    }
                    "RPAREN" => mindent_level -= 1,
                    _ => {}
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
            // What happens after the element
            if flush {
                writeln!(out)?;
                let indent = "    ".repeat(mindent_level);
                write!(out, "{indent}")?;
                flush = false
            }
            if cosmetic_ws {
                write!(out, " ")?;
                cosmetic_ws = false
            }
            // Write token to buffer
            if node.child_count() == 0 {
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                let text = std::str::from_utf8(&source_code[start_byte..end_byte]).unwrap();
                if node.kind() == "single_comment" {
                    write!(out, "{}", text.trim_end())?;
                } else {
                    write!(out, "{}", text)?;
                }
            }

            match node.kind() {
                "single_comment" => flush = true,
                "termvec" | "binaryargvec" => state.in_termvec -= 1,
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
                "LPAREN" => mindent_level += 1,
                // Add semantic whitespace
                "NOT" | "aggregatefunction" | "theory_identifier" | "EXTERNAL" | "DEFINED"
                | "CONST" | "BLOCK" | "PROJECT" | "HEURISTIC" | "THEORY" | "MAXIMIZE"
                | "MINIMIZE" => write!(out, " ")?,
                "INCLUDE" => {
                    write!(out, " ")?;
                    state.is_include = true;
                }
                "SHOW" => {
                    write!(out, " ")?;
                    state.is_show = true;
                }
                // Add cosmetic whitespace
                "cmp" | "VBAR" => write!(out, " ")?,
                "SEM" => flush = true,
                "COLON" => {
                    if state.in_theory_atom_definition | state.is_show {
                        write!(out, " ")?;
                    } else if state.in_conjunction | state.in_optcondition {
                        mindent_level += 1;
                        flush = true;
                    }
                }
                "LBRACE" => {
                    if state.in_theory_atom_definition {
                        write!(out, " ")?;
                    } else {
                        mindent_level += 1;
                        flush = true;
                    }
                }
                "COMMA" => {
                    if state.in_termvec == 0 && !state.is_show
                    /*|| buf.len() >= MAX_LENGTH */
                    {
                        flush = true;
                    } else {
                        write!(out, " ")?;
                    }
                }
                "IF" => {
                    state.has_if = true;
                    mindent_level += 1; // decrease after bodydot
                    if !state.has_head_like {
                        write!(out, " ")?;
                    } else {
                        flush = true;
                    }
                }
                _ => {}
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
    if state.has_head_like & !state.has_body {
        Ok(StatementType::Fact)
    } else if state.is_show {
        Ok(StatementType::Show)
    } else if state.is_include {
        Ok(StatementType::Include)
    } else {
        Ok(StatementType::Other)
    }
}
