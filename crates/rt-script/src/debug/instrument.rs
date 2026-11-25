// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! Source instrumentation for a debug run (and only for one): a call `__rt_b(line, id, locals,
//! closure, ev)` before each statement of the script being debugged, `__rt_e(name)` and
//! `__rt_x()` around the bodies of ordinary functions (so the debugger knows the call depth).
//! Nothing is added to the lines: only text inside lines, so a line in the instrumented
//! source is the line the user sees.
//!
//! This is a scanner, not a parser: it follows tokens, brackets and the few statement rules
//! that matter (blocks, control headers, `case` clauses, automatic semicolons). Where it is
//! unsure it does not insert anything: a missing marker costs a place to stop, a wrong one
//! would change the script. `ev` is `(__rt_q) => eval(__rt_q)`, a direct `eval` that sees the
//! scope of the statement.

use std::collections::BTreeSet;

/// The instrumented script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrumented {
    pub code: String,
    /// The lines a marker was inserted on, ascending: where the script can stop.
    pub lines: Vec<u32>,
    /// Names declared at the top level of the script, comma separated.
    pub globals: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ident,
    Num,
    Str,
    Template,
    Regex,
    Punct,
}

#[derive(Debug, Clone, Copy)]
struct Tok {
    kind: Kind,
    start: usize,
    end: usize,
    line: u32,
    /// A line break lies between the previous token and this one.
    nl_before: bool,
}

const THREE: [&str; 10] = [
    "===", "!==", "...", ">>>", "**=", "<<=", ">>=", "&&=", "||=", "??=",
];
const TWO: [&str; 23] = [
    "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=",
    "&=", "|=", "^=", "<<", ">>", "**", "<>",
];

fn is_ident_start(c: char) -> bool {
    c == '_' || c == '$' || c == '#' || c.is_alphabetic() || (c as u32) > 127
}

fn is_ident_part(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit()
}

/// Words after which a `/` starts a regular expression.
fn regex_after_word(word: &str) -> bool {
    matches!(
        word,
        "return"
            | "typeof"
            | "instanceof"
            | "in"
            | "of"
            | "new"
            | "delete"
            | "void"
            | "throw"
            | "case"
            | "do"
            | "else"
            | "yield"
            | "await"
    )
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
}

impl<'a> Lexer<'a> {
    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn skip_string(&mut self, quote: u8) {
        self.pos += 1;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'\\' {
                if self.bytes.get(self.pos + 1) == Some(&b'\n') {
                    self.line += 1;
                }
                self.pos += 2;
                continue;
            }
            if b == b'\n' {
                // An unterminated string: stop at the line end.
                return;
            }
            self.pos += 1;
            if b == quote {
                return;
            }
        }
    }

    /// After the opening backtick: to the closing one, `${…}` included.
    fn skip_template(&mut self) {
        self.pos += 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'\\' => {
                    if self.bytes.get(self.pos + 1) == Some(&b'\n') {
                        self.line += 1;
                    }
                    self.pos += 2;
                }
                b'`' => {
                    self.pos += 1;
                    return;
                }
                b'$' if self.bytes.get(self.pos + 1) == Some(&b'{') => {
                    self.pos += 2;
                    self.skip_braced();
                }
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
    }

    /// Inside `${`: to the matching `}`.
    fn skip_braced(&mut self) {
        let mut depth = 1;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'{' => {
                    depth += 1;
                    self.pos += 1;
                }
                b'}' => {
                    depth -= 1;
                    self.pos += 1;
                    if depth == 0 {
                        return;
                    }
                }
                b'\'' => self.skip_string(b'\''),
                b'"' => self.skip_string(b'"'),
                b'`' => self.skip_template(),
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => self.pos += 1,
            }
        }
    }

    fn skip_regex(&mut self) {
        self.pos += 1;
        let mut in_class = false;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            match b {
                b'\\' => {
                    self.pos += 2;
                    continue;
                }
                b'\n' => return,
                b'[' => in_class = true,
                b']' => in_class = false,
                b'/' if !in_class => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            self.pos += 1;
        }
        while let Some(c) = self.peek_char() {
            if !is_ident_part(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
    }
}

fn tokenize(src: &str) -> Vec<Tok> {
    let mut lx = Lexer {
        src,
        bytes: src.as_bytes(),
        pos: 0,
        line: 1,
    };
    let mut toks: Vec<Tok> = Vec::new();
    let mut nl = false;
    while lx.pos < lx.bytes.len() {
        let b = lx.bytes[lx.pos];
        match b {
            b'\n' => {
                lx.line += 1;
                lx.pos += 1;
                nl = true;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                lx.pos += 1;
                continue;
            }
            b'/' if lx.bytes.get(lx.pos + 1) == Some(&b'/') => {
                while lx.pos < lx.bytes.len() && lx.bytes[lx.pos] != b'\n' {
                    lx.pos += 1;
                }
                continue;
            }
            b'/' if lx.bytes.get(lx.pos + 1) == Some(&b'*') => {
                lx.pos += 2;
                while lx.pos < lx.bytes.len() {
                    if lx.bytes[lx.pos] == b'*' && lx.bytes.get(lx.pos + 1) == Some(&b'/') {
                        lx.pos += 2;
                        break;
                    }
                    if lx.bytes[lx.pos] == b'\n' {
                        lx.line += 1;
                        nl = true;
                    }
                    lx.pos += 1;
                }
                continue;
            }
            _ => {}
        }
        let start = lx.pos;
        let line = lx.line;
        let kind;
        match b {
            b'\'' | b'"' => {
                lx.skip_string(b);
                kind = Kind::Str;
            }
            b'`' => {
                lx.skip_template();
                kind = Kind::Template;
            }
            b'0'..=b'9' => {
                lx.pos += 1;
                while lx.pos < lx.bytes.len() {
                    let c = lx.bytes[lx.pos];
                    let exp_sign = (c == b'+' || c == b'-')
                        && matches!(lx.bytes[lx.pos - 1], b'e' | b'E')
                        && !src[start..lx.pos].starts_with("0x");
                    if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || exp_sign {
                        lx.pos += 1;
                    } else {
                        break;
                    }
                }
                kind = Kind::Num;
            }
            b'.' if lx.bytes.get(lx.pos + 1).is_some_and(u8::is_ascii_digit) => {
                lx.pos += 1;
                while lx.pos < lx.bytes.len() && lx.bytes[lx.pos].is_ascii_alphanumeric() {
                    lx.pos += 1;
                }
                kind = Kind::Num;
            }
            b'/' => {
                let allowed = match toks.last() {
                    None => true,
                    Some(t) => match t.kind {
                        Kind::Num | Kind::Str | Kind::Template | Kind::Regex => false,
                        Kind::Ident => regex_after_word(&src[t.start..t.end]),
                        Kind::Punct => {
                            !matches!(&src[t.start..t.end], ")" | "]" | "}" | "++" | "--")
                        }
                    },
                };
                if allowed {
                    lx.skip_regex();
                    kind = Kind::Regex;
                } else {
                    lx.pos += if lx.bytes.get(lx.pos + 1) == Some(&b'=') {
                        2
                    } else {
                        1
                    };
                    kind = Kind::Punct;
                }
            }
            _ => {
                let c = lx.peek_char().expect("a char");
                if is_ident_start(c) {
                    while let Some(c) = lx.peek_char() {
                        if !is_ident_part(c) {
                            break;
                        }
                        lx.pos += c.len_utf8();
                    }
                    kind = Kind::Ident;
                } else {
                    let rest = &src[lx.pos..];
                    let n = if THREE.iter().any(|p| rest.starts_with(p)) {
                        3
                    } else if TWO.iter().any(|p| rest.starts_with(p)) {
                        2
                    } else {
                        c.len_utf8()
                    };
                    lx.pos += n;
                    kind = Kind::Punct;
                }
            }
        }
        toks.push(Tok {
            kind,
            start,
            end: lx.pos,
            line,
            nl_before: nl,
        });
        nl = false;
    }
    toks
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Program,
    Block,
    Object,
    Paren,
    Bracket,
    Class,
}

#[derive(Debug)]
struct Frame {
    kind: FrameKind,
    /// Block: the body of a function (names of the parameters are in `names`).
    func: bool,
    /// Block: whether the function is tracked for the call depth (`__rt_e`/`__rt_x`).
    tracked: bool,
    /// Block: the next token after its `}` can start a statement.
    statement: bool,
    switch: bool,
    do_body: bool,
    /// Paren: a control header (`if`, `for`, `while`, `switch`, `with`, `catch`).
    control: Option<&'static str>,
    /// Object/Bracket: a binding pattern of a declaration (`const { a, b } = …`).
    pattern: bool,
    names: Vec<String>,
}

impl Frame {
    fn new(kind: FrameKind) -> Self {
        Self {
            kind,
            func: false,
            tracked: false,
            statement: false,
            switch: false,
            do_body: false,
            control: None,
            pattern: false,
            names: Vec::new(),
        }
    }
}

fn is_keyword_not_ending(word: &str) -> bool {
    matches!(
        word,
        "else"
            | "do"
            | "try"
            | "finally"
            | "new"
            | "typeof"
            | "void"
            | "delete"
            | "in"
            | "of"
            | "instanceof"
            | "await"
            | "extends"
            | "class"
            | "function"
            | "var"
            | "let"
            | "const"
            | "case"
            | "async"
            | "static"
            | "get"
            | "set"
    )
}

struct Scanner<'a> {
    src: &'a str,
    toks: &'a [Tok],
    frames: Vec<Frame>,
    /// Insertions `(offset, order, text)`.
    inserts: Vec<(usize, usize, String)>,
    marker_lines: BTreeSet<u32>,
    at_stmt: bool,
    /// The statement position is the body of a control statement: no marker, and no marker
    /// for the single statement (a marker there would end the body).
    suppress: bool,
    pending_names: Vec<String>,
    last_paren: Vec<String>,
    /// Inside a `case` clause: ternary `?` not yet answered by `:`.
    case_ternary: Option<u32>,
    dowhile: bool,
    next_paren_control: Option<&'static str>,
    next_block_switch: bool,
    next_block_do: bool,
    /// The next `{` after `function …()` / `) =>` is a function body: its name and kind.
    async_flag: bool,
    generator_flag: bool,
    last_binding: Option<String>,
    func_name: Option<String>,
    func_is_decl: bool,
    decl: Option<usize>,
    class_decl: bool,
    /// The token being handled starts a statement.
    stmt_here: bool,
    /// The token is the body of a control statement (`if (…) body`, `else body`, …).
    body_position: bool,
    async_stmt: bool,
    seq: usize,
}

impl<'a> Scanner<'a> {
    fn text(&self, t: &Tok) -> &'a str {
        &self.src[t.start..t.end]
    }

    fn is(&self, i: usize, s: &str) -> bool {
        self.toks
            .get(i)
            .is_some_and(|t| t.kind == Kind::Punct && self.text(t) == s)
    }

    fn is_word(&self, i: usize, s: &str) -> bool {
        self.toks
            .get(i)
            .is_some_and(|t| t.kind == Kind::Ident && self.text(t) == s)
    }

    fn insert(&mut self, offset: usize, text: String) {
        self.seq += 1;
        self.inserts.push((offset, self.seq, text));
    }

    fn top(&self) -> &Frame {
        self.frames.last().expect("a frame")
    }

    fn in_statement_list(&self) -> bool {
        matches!(self.top().kind, FrameKind::Block | FrameKind::Program)
    }

    /// Records a declared name: with the function (`var`) or the innermost scope.
    fn declare(&mut self, name: &str, hoist: bool) {
        let name = name.to_owned();
        if hoist {
            for f in self.frames.iter_mut().rev() {
                if f.func || f.kind == FrameKind::Program {
                    f.names.push(name);
                    return;
                }
            }
        }
        if let Some(f) = self.frames.last_mut() {
            f.names.push(name);
        }
    }

    /// The scope lists for a marker: names of the blocks up to and including the innermost
    /// function (`local`), the scopes around it (`closure`).
    fn scopes(&self) -> (String, String) {
        let mut local: Vec<&str> = Vec::new();
        let mut closure: Vec<&str> = Vec::new();
        let mut in_local = true;
        for f in self.frames.iter().rev() {
            if f.kind == FrameKind::Program {
                break;
            }
            if !matches!(f.kind, FrameKind::Block) {
                continue;
            }
            let target = if in_local { &mut local } else { &mut closure };
            target.extend(f.names.iter().map(String::as_str));
            if in_local && f.func {
                in_local = false;
            }
        }
        if in_local {
            // No function: everything outside the program level is local.
            closure.clear();
        }
        let join = |v: Vec<&str>| {
            let mut seen = BTreeSet::new();
            let v: Vec<&str> = v.into_iter().filter(|n| seen.insert(*n)).collect();
            v.join(",")
        };
        (join(local), join(closure))
    }

    fn marker(&mut self, tok: &Tok) {
        let (local, closure) = self.scopes();
        self.marker_lines.insert(tok.line);
        let text = format!(
            "__rt_b({},{},\"{}\",\"{}\",(__rt_q)=>eval(__rt_q));",
            tok.line, tok.start, local, closure
        );
        self.insert(tok.start, text);
    }

    /// Whether the token at `i` can start a statement (given that one may start here).
    fn can_start(&self, i: usize) -> bool {
        let t = &self.toks[i];
        match t.kind {
            Kind::Ident => {
                let w = self.text(t);
                if matches!(
                    w,
                    "else" | "catch" | "finally" | "case" | "default" | "extends"
                ) {
                    return false;
                }
                if w == "while" && self.dowhile {
                    return false;
                }
                // Declarations are not executed: no place to stop.
                if w == "function" || (w == "async" && self.is_word(i + 1, "function")) {
                    return false;
                }
                true
            }
            Kind::Num | Kind::Str | Kind::Template | Kind::Regex => true,
            Kind::Punct => matches!(self.text(t), "(" | "[" | "{" | "!" | "~" | "++" | "--"),
        }
    }

    /// Whether the token at `i`, first on its line, starts a new statement because the
    /// previous token ended one (automatic semicolon insertion).
    fn asi_start(&self, i: usize) -> bool {
        if i == 0 || !self.toks[i].nl_before || !self.in_statement_list() {
            return false;
        }
        let prev = &self.toks[i - 1];
        let ends = match prev.kind {
            Kind::Num | Kind::Str | Kind::Template | Kind::Regex => true,
            Kind::Ident => !is_keyword_not_ending(self.text(prev)),
            Kind::Punct => matches!(self.text(prev), ")" | "]" | "}" | "++" | "--"),
        };
        if !ends {
            return false;
        }
        let t = &self.toks[i];
        match t.kind {
            Kind::Ident => !matches!(
                self.text(t),
                "in" | "of"
                    | "instanceof"
                    | "else"
                    | "catch"
                    | "finally"
                    | "extends"
                    | "case"
                    | "default"
            ),
            Kind::Num | Kind::Str => true,
            Kind::Template | Kind::Regex => false,
            Kind::Punct => matches!(self.text(t), "!" | "~" | "{"),
        }
    }

    fn run(&mut self) {
        self.frames.push(Frame::new(FrameKind::Program));
        self.at_stmt = true;
        let mut i = 0;
        while i < self.toks.len() {
            i += self.step(i);
        }
    }

    /// Handles token `i`; returns how many tokens it used.
    fn step(&mut self, i: usize) -> usize {
        let t = self.toks[i];
        let text = self.text(&t);
        let prev_word = |s: &Self, w: &str| i > 0 && s.is_word(i - 1, w);

        // Statement position: a marker, unless something rules it out.
        let mut statement_start = self.at_stmt;
        self.body_position = self.at_stmt && self.suppress;
        if !statement_start && self.asi_start(i) {
            statement_start = true;
            self.suppress = false;
        }
        let _ = prev_word;
        self.stmt_here = statement_start;
        if statement_start {
            if self.can_start(i) && !self.suppress && self.in_statement_list() {
                self.marker(&t);
            }
            self.suppress = false;
            self.at_stmt = false;
            self.decl = None;
        }

        match t.kind {
            Kind::Ident => self.ident(i, text),
            Kind::Punct => self.punct(i, &t, text),
            _ => {
                self.last_binding = None;
            }
        }
        // `case x:` colon tracking.
        1
    }

    fn ident(&mut self, i: usize, text: &str) {
        match text {
            "var" | "let" | "const" => {
                self.decl = Some(self.frames.len());
                if let Some(n) = self.toks.get(i + 1)
                    && n.kind == Kind::Ident
                {
                    let name = self.text(n).to_owned();
                    if !is_keyword_not_ending(&name) || name == "of" {
                        self.declare(&name, text == "var");
                    }
                }
                self.last_binding = None;
                return;
            }
            "function" => {
                let decl =
                    self.stmt_here || (i > 0 && self.is_word(i - 1, "async") && self.async_stmt);
                self.func_is_decl = decl;
                self.generator_flag = self.is(i + 1, "*");
                let name_at = if self.generator_flag { i + 2 } else { i + 1 };
                self.func_name = None;
                if let Some(n) = self.toks.get(name_at)
                    && n.kind == Kind::Ident
                {
                    let name = self.text(n).to_owned();
                    if decl {
                        self.declare(&name, false);
                    }
                    self.func_name = Some(name);
                }
                self.last_binding = None;
                return;
            }
            "class" => {
                if let Some(n) = self.toks.get(i + 1)
                    && n.kind == Kind::Ident
                    && !self.is_word(i + 1, "extends")
                {
                    let name = self.text(n).to_owned();
                    self.declare(&name, false);
                }
                self.class_decl = self.stmt_here;
                self.last_binding = None;
                return;
            }
            "async" => {
                self.async_flag = true;
                self.async_stmt = self.stmt_here;
                return;
            }
            "if" | "for" | "while" | "switch" | "with" | "catch" => {
                if text == "while" && self.dowhile {
                    self.dowhile = false;
                } else if self.is(i + 1, "(") || (text == "for" && self.is_word(i + 1, "await")) {
                    self.next_paren_control = Some(match text {
                        "if" => "if",
                        "for" => "for",
                        "while" => "while",
                        "switch" => "switch",
                        "with" => "with",
                        _ => "catch",
                    });
                } else if text == "catch" {
                    // `catch {`: a block right away.
                    self.at_stmt = true;
                    self.suppress = true;
                }
                return;
            }
            "else" | "try" | "finally" => {
                self.at_stmt = true;
                self.suppress = true;
                return;
            }
            "do" => {
                self.at_stmt = true;
                self.suppress = true;
                self.next_block_do = true;
                return;
            }
            "case" | "default" => {
                if self.top().switch && self.at_clause_start(i) {
                    self.case_ternary = Some(0);
                }
                return;
            }
            _ => {}
        }
        let binding_position = i > 0
            && self.toks[i - 1].kind == Kind::Punct
            && matches!(
                self.text(&self.toks[i - 1]),
                "(" | "," | "..." | "{" | "[" | ":"
            );
        let key = self.is(i + 1, ":");
        // Parameters (any parenthesis: a call's arguments are collected too and never used).
        if binding_position && !key {
            self.collect_binding(text);
        }
        // `let a = 1, b = 2`, `const { a, b: c } = x`: names of a declaration.
        if let Some(depth) = self.decl {
            let top = self.top();
            if matches!(top.kind, FrameKind::Object | FrameKind::Bracket) {
                if top.pattern && binding_position && !key {
                    self.declare_outside_patterns(text);
                }
            } else if depth == self.frames.len() && i > 0 && self.is(i - 1, ",") {
                self.declare(text, false);
            }
        }
        // Arrow function name: `name = (…) =>`.
        if self.is(i + 1, "=") || self.is(i + 1, ":") {
            self.last_binding = Some(text.to_owned());
        }
    }

    fn collect_binding(&mut self, text: &str) {
        // The nearest parenthesis, through patterns only.
        for f in self.frames.iter_mut().rev() {
            match f.kind {
                FrameKind::Paren => {
                    f.names.push(text.to_owned());
                    return;
                }
                FrameKind::Object | FrameKind::Bracket => continue,
                _ => return,
            }
        }
    }

    fn at_clause_start(&self, i: usize) -> bool {
        i == 0
            || self.is(i - 1, ":")
            || self.is(i - 1, ";")
            || self.is(i - 1, "{")
            || self.is(i - 1, "}")
    }

    /// A name of a destructuring pattern in a declaration: into the scope outside the
    /// pattern's brackets.
    fn declare_outside_patterns(&mut self, name: &str) {
        let mut idx = self.frames.len();
        while idx > 0
            && matches!(
                self.frames[idx - 1].kind,
                FrameKind::Object | FrameKind::Bracket
            )
        {
            idx -= 1;
        }
        if idx > 0 {
            self.frames[idx - 1].names.push(name.to_owned());
        }
    }

    fn punct(&mut self, i: usize, t: &Tok, text: &str) {
        match text {
            "(" => {
                let mut f = Frame::new(FrameKind::Paren);
                f.control = self.next_paren_control.take();
                self.frames.push(f);
                self.at_stmt = false;
            }
            ")" => {
                if self.frames.len() > 1 && self.top().kind == FrameKind::Paren {
                    let f = self.frames.pop().expect("a paren");
                    match f.control {
                        Some(kind) => {
                            if kind == "catch" || kind == "for" {
                                self.pending_names.extend(f.names);
                            }
                            self.next_block_switch = kind == "switch";
                            self.at_stmt = true;
                            self.suppress = true;
                        }
                        None => {
                            self.last_paren = f.names;
                        }
                    }
                }
            }
            "[" => {
                let mut f = Frame::new(FrameKind::Bracket);
                f.pattern = self.pattern_opening(i);
                self.frames.push(f);
                self.at_stmt = false;
            }
            "]" => {
                if self.top().kind == FrameKind::Bracket {
                    self.frames.pop();
                }
            }
            "{" => self.open_brace(i, t),
            "}" => self.close_brace(t),
            ";" => {
                if self.in_statement_list() {
                    self.at_stmt = true;
                    self.suppress = false;
                    self.decl = None;
                    self.pending_names.clear();
                    self.async_flag = false;
                }
            }
            "=>" => {
                // Parameters: `x =>` or `(…) =>`.
                let prev = i.checked_sub(1).map(|p| &self.toks[p]);
                if let Some(p) = prev {
                    if p.kind == Kind::Ident {
                        self.pending_names = vec![self.text(p).to_owned()];
                    } else if self.text(p) == ")" {
                        self.pending_names = std::mem::take(&mut self.last_paren);
                    }
                }
                self.func_name = self.last_binding.clone();
                self.func_is_decl = false;
                self.generator_flag = false;
            }
            ":" => {
                if let Some(depth) = self.case_ternary {
                    if depth == 0 {
                        self.case_ternary = None;
                        self.at_stmt = true;
                        self.suppress = false;
                    } else {
                        self.case_ternary = Some(depth - 1);
                    }
                }
            }
            "?" => {
                if let Some(depth) = self.case_ternary {
                    self.case_ternary = Some(depth + 1);
                }
            }
            "," => {}
            _ => {}
        }
        if text != "=" && text != ":" {
            // A binding name only counts right before `=` or `=>`.
        }
        if !matches!(text, "(" | "=>" | "=") {
            self.last_binding = if text == ")" {
                self.last_binding.take()
            } else {
                None
            };
        }
        let _ = t;
    }

    /// Whether a `{` or `[` at `i` opens (part of) a binding pattern of a declaration.
    fn pattern_opening(&self, i: usize) -> bool {
        if self.top().pattern {
            return true;
        }
        let Some(depth) = self.decl else {
            return false;
        };
        depth == self.frames.len()
            && i > 0
            && (self.is(i - 1, ",")
                || ["var", "let", "const"]
                    .iter()
                    .any(|k| self.is_word(i - 1, k)))
    }

    fn open_brace(&mut self, i: usize, t: &Tok) {
        let prev_text = i.checked_sub(1).map(|p| self.text(&self.toks[p]));
        let prev_is_paren_close = prev_text == Some(")");
        let prev_is_arrow = prev_text == Some("=>");
        let in_class = self.top().kind == FrameKind::Class;
        // The `)` of a control header was just handled: this is its body.
        let after_control = prev_is_paren_close && self.body_position;
        let block = self.stmt_here
            || prev_is_paren_close
            || prev_is_arrow
            || (in_class && prev_text == Some("static"));
        if !block {
            // An object literal, a pattern, or a class body.
            let class = self.class_body_ahead(i);
            let mut f = Frame::new(if class {
                FrameKind::Class
            } else {
                FrameKind::Object
            });
            f.statement = class && self.class_decl;
            f.pattern = !class && self.pattern_opening(i);
            self.frames.push(f);
            self.at_stmt = false;
            self.class_decl = false;
            return;
        }
        let mut f = Frame::new(FrameKind::Block);
        f.switch = self.next_block_switch;
        f.do_body = self.next_block_do;
        f.names = std::mem::take(&mut self.pending_names);
        let function_body = (prev_is_paren_close && !after_control) || prev_is_arrow;
        if function_body {
            f.func = true;
            f.tracked = !self.async_flag && !self.generator_flag;
            f.statement = self.func_is_decl;
            let name = match self.func_name.take() {
                Some(n) => n,
                None if prev_is_arrow => "(anonymous)".to_owned(),
                None => self.method_name(i),
            };
            if f.tracked {
                self.insert(t.end, format!("__rt_e({});try{{", js_string(&name)));
            }
            self.async_flag = false;
            self.generator_flag = false;
            self.func_is_decl = false;
            f.names.extend(std::mem::take(&mut self.last_paren));
        } else {
            f.statement = true;
        }
        self.next_block_switch = false;
        self.next_block_do = false;
        self.frames.push(f);
        self.at_stmt = true;
        self.suppress = false;
    }

    /// The name of a method whose body starts at `i`: the identifier before its `(`.
    fn method_name(&self, i: usize) -> String {
        // Walk back over the parameter list.
        let mut depth = 0i32;
        let mut p = i;
        while p > 0 {
            p -= 1;
            let t = &self.toks[p];
            if t.kind == Kind::Punct {
                match self.text(t) {
                    ")" => depth += 1,
                    "(" => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        if p > 0 && self.toks[p - 1].kind == Kind::Ident {
            let n = self.text(&self.toks[p - 1]);
            if !matches!(
                n,
                "function" | "if" | "for" | "while" | "switch" | "catch" | "with"
            ) {
                return n.to_owned();
            }
        }
        "(anonymous)".to_owned()
    }

    fn class_body_ahead(&self, i: usize) -> bool {
        // `class A {` / `class A extends B {` / `class {`: look back to a `class` keyword
        // within the same header.
        let mut p = i;
        let mut steps = 0;
        while p > 0 && steps < 12 {
            p -= 1;
            steps += 1;
            let t = &self.toks[p];
            if t.kind == Kind::Ident && self.text(t) == "class" {
                return true;
            }
            if t.kind == Kind::Punct && matches!(self.text(t), ";" | "{" | "}" | "=" | "(" | ")") {
                // `extends mixin(…)` keeps looking a little: only stop at `;`, `{`, `}`.
                if matches!(self.text(t), ";" | "{" | "}") {
                    return false;
                }
            }
        }
        false
    }

    fn close_brace(&mut self, t: &Tok) {
        if self.frames.len() <= 1 {
            return;
        }
        let f = self.frames.pop().expect("a frame");
        match f.kind {
            FrameKind::Block => {
                if f.func && f.tracked {
                    self.insert(t.start, "}finally{__rt_x()}".to_owned());
                }
                if f.do_body {
                    self.dowhile = true;
                }
                if f.statement {
                    self.at_stmt = self.in_statement_list();
                    self.suppress = false;
                } else {
                    self.at_stmt = false;
                }
                self.case_ternary = None;
            }
            FrameKind::Class => {
                self.at_stmt = f.statement && self.in_statement_list();
                self.suppress = false;
            }
            _ => {
                self.at_stmt = false;
            }
        }
        self.decl = None;
    }
}

fn js_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Instruments `source` for a debug run.
pub fn instrument(source: &str) -> Instrumented {
    let toks = tokenize(source);
    let mut scanner = Scanner {
        src: source,
        toks: &toks,
        frames: Vec::new(),
        inserts: Vec::new(),
        marker_lines: BTreeSet::new(),
        at_stmt: true,
        suppress: false,
        pending_names: Vec::new(),
        last_paren: Vec::new(),
        case_ternary: None,
        dowhile: false,
        next_paren_control: None,
        next_block_switch: false,
        next_block_do: false,
        async_flag: false,
        generator_flag: false,
        last_binding: None,
        func_name: None,
        func_is_decl: false,
        decl: None,
        class_decl: false,
        stmt_here: false,
        body_position: false,
        async_stmt: false,
        seq: 0,
    };
    scanner.run();
    let globals = {
        let mut seen = BTreeSet::new();
        scanner
            .frames
            .first()
            .map(|f| {
                f.names
                    .iter()
                    .filter(|n| seen.insert(n.as_str()))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default()
    };
    let mut inserts = std::mem::take(&mut scanner.inserts);
    inserts.sort_by_key(|(offset, order, _)| (*offset, *order));
    let lines: Vec<u32> = scanner.marker_lines.iter().copied().collect();
    let mut code = String::with_capacity(source.len() + inserts.len() * 48);
    let mut at = 0;
    for (offset, _, text) in &inserts {
        code.push_str(&source[at..*offset]);
        code.push_str(text);
        at = *offset;
    }
    code.push_str(&source[at..]);
    Instrumented {
        code,
        lines,
        globals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(src: &str) -> Vec<u32> {
        instrument(src).lines
    }

    #[test]
    fn markers_stand_before_statements_and_keep_the_lines() {
        let src = "let a = 1;\nlet b = a + 1;\nconsole.log(b);\n";
        let out = instrument(src);
        assert_eq!(out.lines, [1, 2, 3]);
        assert_eq!(out.code.lines().count(), src.lines().count());
        assert!(out.code.starts_with("__rt_b(1,"));
        assert_eq!(out.globals, "a,b");
    }

    #[test]
    fn function_bodies_are_tracked_and_hold_their_own_markers() {
        let src = "function add(x, y) {\n  const s = x + y;\n  return s;\n}\nadd(1, 2);\n";
        let out = instrument(src);
        // The declaration itself is not a stop; its body is, and the call.
        assert_eq!(out.lines, [2, 3, 5]);
        assert!(out.code.contains("__rt_e(\"add\");try{"));
        assert!(out.code.contains("}finally{__rt_x()}}"));
        assert!(out.code.contains("\"s,x,y\"") || out.code.contains("\"x,y,s\""));
    }

    #[test]
    fn control_bodies_without_braces_are_left_alone() {
        let src = "if (a) b();\nelse c();\nd();\n";
        assert_eq!(marks(src), [1, 3]);
        let src = "for (let i = 0; i < 3; i++) f(i);\ng();\n";
        assert_eq!(marks(src), [1, 2]);
    }

    #[test]
    fn braced_control_bodies_get_markers() {
        let src = "if (a) {\n  b();\n} else {\n  c();\n}\nwhile (x) {\n  y();\n}\ndo {\n  z();\n} while (q);\nw();\n";
        assert_eq!(marks(src), [1, 2, 4, 6, 7, 9, 10, 12]);
    }

    #[test]
    fn strings_comments_templates_and_regexes_hide_their_content() {
        let src = "const s = \"a;b\\\" ;\"; // c; d;\n/* e; \n f; */ const t = `x ${ {a:1}.a; } ; y`;\nconst r = /;\\/[;]/g;\nfoo();\n";
        assert_eq!(marks(src), [1, 3, 4, 5]);
    }

    #[test]
    fn statements_without_semicolons_are_found_by_line() {
        let src = "const a = 1\nconst b = 2\nfoo(a,\n  b)\nbar()\n";
        assert_eq!(marks(src), [1, 2, 3, 5]);
        // A line starting with `(`, `[` or `.` continues the previous statement.
        let src = "foo()\n  .bar()\n  .baz()\n";
        assert_eq!(marks(src), [1]);
    }

    #[test]
    fn object_literals_arrow_expressions_and_switch() {
        let src = "const o = {\n  a: 1,\n  b() {\n    return 2;\n  },\n};\nconst f = (x) => x * 2;\nconst g = async () => {\n  await 1;\n};\nswitch (v) {\n  case 1:\n    one();\n    break;\n  default:\n    two();\n}\n";
        let out = instrument(src);
        assert_eq!(out.lines, [1, 4, 7, 8, 9, 11, 13, 14, 16]);
        // `b` is tracked, the async arrow is not.
        assert!(out.code.contains("__rt_e(\"b\")"));
        assert!(!out.code.contains("__rt_e(\"g\")"));
    }

    #[test]
    fn try_catch_finally_and_labels() {
        let src = "try {\n  a();\n} catch (e) {\n  b(e);\n} finally {\n  c();\n}\nouter: for (;;) { break outer; }\n";
        let out = instrument(src);
        assert_eq!(out.lines, [1, 2, 4, 6, 8]);
        assert!(!out.code.contains("outer: __rt_b"));
    }

    #[test]
    fn classes_do_not_get_statement_markers_between_members() {
        let src = "class A {\n  constructor(x) {\n    this.x = x;\n  }\n  get y() {\n    return this.x;\n  }\n}\nnew A(1);\n";
        let out = instrument(src);
        assert_eq!(out.lines, [1, 3, 6, 9]);
    }

    #[test]
    fn the_result_still_parses_in_shape() {
        // Braces and parentheses stay balanced.
        let src = "function f(a) {\n  if (a) { return 1; }\n  return [1, 2].map((v) => {\n    return v * 2;\n  });\n}\nf(1);\n";
        let out = instrument(src).code;
        let count = |c: char| out.chars().filter(|x| *x == c).count();
        assert_eq!(count('{'), count('}'));
        assert_eq!(count('('), count(')'));
    }
}
