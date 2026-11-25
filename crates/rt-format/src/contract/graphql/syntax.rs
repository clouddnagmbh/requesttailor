// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

//! GraphQL text: the type system (SDL) and the executable part (operations and fragments), read
//! into the little structures the comparison needs. Written here instead of taking a parser
//! crate: the grammar is small, nothing in it can panic or recurse without a bound, and it
//! adds no dependency to build for every target.
//!
//! What is read: all definitions and their extensions, descriptions, `@deprecated`, defaults
//! (only whether there is one), directive definitions, operations, fragments, and selections
//! (fields, arguments by name, inline fragments, spreads). Values and directive arguments are
//! skipped. Commas are white space, as in GraphQL.

use std::collections::BTreeMap;
use std::fmt;

/// Deepest nesting of types, values and selections.
const MAX_DEPTH: u32 = 64;

// ---- tokens ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Punct(char),
    /// `...`
    Spread,
    Name(String),
    Number,
    /// A string; the text of a block string is kept for descriptions.
    Str(String),
    Eof,
}

struct Token {
    tok: Tok,
    line: u32,
}

fn lex(src: &str) -> Result<Vec<Token>, String> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let (mut i, mut line) = (0usize, 1u32);
    if src.starts_with('\u{feff}') {
        i += 3;
    }
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | b',' => i += 1,
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'!' | b'$' | b'&' | b'(' | b')' | b':' | b'=' | b'@' | b'[' | b']' | b'{' | b'|'
            | b'}' => {
                out.push(Token {
                    tok: Tok::Punct(c as char),
                    line,
                });
                i += 1;
            }
            b'.' => {
                if bytes[i..].starts_with(b"...") {
                    out.push(Token {
                        tok: Tok::Spread,
                        line,
                    });
                    i += 3;
                } else {
                    return Err(format!("line {line}: unexpected `.`"));
                }
            }
            b'"' => {
                let start_line = line;
                if bytes[i..].starts_with(b"\"\"\"") {
                    let mut j = i + 3;
                    let mut text = String::new();
                    loop {
                        if j >= bytes.len() {
                            return Err(format!("line {start_line}: block string is not closed"));
                        }
                        if bytes[j..].starts_with(b"\\\"\"\"") {
                            text.push_str("\"\"\"");
                            j += 4;
                        } else if bytes[j..].starts_with(b"\"\"\"") {
                            j += 3;
                            break;
                        } else {
                            if bytes[j] == b'\n' {
                                line += 1;
                            }
                            let ch = src[j..].chars().next().unwrap_or(' ');
                            text.push(ch);
                            j += ch.len_utf8();
                        }
                    }
                    out.push(Token {
                        tok: Tok::Str(text.trim().to_owned()),
                        line: start_line,
                    });
                    i = j;
                } else {
                    let mut j = i + 1;
                    let mut text = String::new();
                    loop {
                        match bytes.get(j) {
                            None | Some(b'\n') => {
                                return Err(format!("line {line}: string is not closed"));
                            }
                            Some(b'"') => {
                                j += 1;
                                break;
                            }
                            Some(b'\\') => {
                                // Escapes keep their text; only equality matters here.
                                text.push('\\');
                                j += 1;
                                if let Some(ch) = src[j..].chars().next() {
                                    text.push(ch);
                                    j += ch.len_utf8();
                                }
                            }
                            Some(_) => {
                                let ch = src[j..].chars().next().unwrap_or(' ');
                                text.push(ch);
                                j += ch.len_utf8();
                            }
                        }
                    }
                    out.push(Token {
                        tok: Tok::Str(text),
                        line,
                    });
                    i = j;
                }
            }
            b'-' | b'0'..=b'9' => {
                let mut j = i + 1;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric()
                        || bytes[j] == b'.'
                        || ((bytes[j] == b'+' || bytes[j] == b'-')
                            && matches!(bytes[j - 1], b'e' | b'E')))
                {
                    j += 1;
                }
                if c == b'-' && j == i + 1 {
                    return Err(format!("line {line}: a lone `-`"));
                }
                out.push(Token {
                    tok: Tok::Number,
                    line,
                });
                i = j;
            }
            b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j] == b'_' || bytes[j].is_ascii_alphanumeric()) {
                    j += 1;
                }
                out.push(Token {
                    tok: Tok::Name(src[i..j].to_owned()),
                    line,
                });
                i = j;
            }
            _ => {
                let ch = src[i..].chars().next().unwrap_or('?');
                return Err(format!("line {line}: unexpected character {ch:?}"));
            }
        }
    }
    out.push(Token {
        tok: Tok::Eof,
        line,
    });
    Ok(out)
}

// ---- the structures -------------------------------------------------------------------

/// A type reference: `Name`, `[T]`, `T!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ty {
    Named(String),
    List(Box<Ty>),
    NonNull(Box<Ty>),
}

impl Ty {
    /// The name at the bottom.
    pub(super) fn named(&self) -> &str {
        match self {
            Ty::Named(n) => n,
            Ty::List(t) | Ty::NonNull(t) => t.named(),
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Named(n) => f.write_str(n),
            Ty::List(t) => write!(f, "[{t}]"),
            Ty::NonNull(t) => write!(f, "{t}!"),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct InputValue {
    pub name: String,
    pub ty: Ty,
    pub has_default: bool,
    pub deprecated: bool,
    pub description: Option<String>,
}

impl InputValue {
    /// Has to be given: non-null without a default.
    pub(super) fn required(&self) -> bool {
        matches!(self.ty, Ty::NonNull(_)) && !self.has_default
    }
}

#[derive(Debug, Clone)]
pub(super) struct FieldDef {
    pub name: String,
    pub ty: Ty,
    pub args: Vec<InputValue>,
    pub deprecated: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct EnumValue {
    pub name: String,
    pub deprecated: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeKind {
    Scalar,
    Object,
    Interface,
    Union,
    Enum,
    Input,
}

impl TypeKind {
    pub(super) fn word(self) -> &'static str {
        match self {
            TypeKind::Scalar => "scalar",
            TypeKind::Object => "object type",
            TypeKind::Interface => "interface",
            TypeKind::Union => "union",
            TypeKind::Enum => "enum",
            TypeKind::Input => "input type",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct TypeDef {
    pub name: String,
    pub kind: TypeKind,
    pub description: Option<String>,
    /// Object and interface fields.
    pub fields: Vec<FieldDef>,
    /// Input object fields.
    pub inputs: Vec<InputValue>,
    pub implements: Vec<String>,
    pub members: Vec<String>,
    pub values: Vec<EnumValue>,
}

#[derive(Debug, Clone)]
pub(super) struct DirectiveDef {
    pub name: String,
    pub args: Vec<InputValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OpKind {
    Query,
    Mutation,
    Subscription,
}

#[derive(Debug, Clone)]
pub(super) enum Sel {
    Field {
        name: String,
        args: Vec<String>,
        sel: Vec<Sel>,
    },
    Inline {
        on: Option<String>,
        sel: Vec<Sel>,
    },
    Spread(String),
}

#[derive(Debug, Clone)]
pub(super) struct Operation {
    pub kind: OpKind,
    /// Named types of the variables.
    pub vars: Vec<String>,
    pub sel: Vec<Sel>,
}

#[derive(Debug, Clone)]
pub(super) struct Fragment {
    pub on: String,
    pub sel: Vec<Sel>,
}

/// One definition of the type system; `extend` says it adds to a definition elsewhere.
#[derive(Debug, Clone)]
pub(super) enum Def {
    Type {
        def: TypeDef,
        extend: bool,
    },
    Directive(DirectiveDef),
    /// `schema { query: Q }`: the root type names.
    Schema(Vec<(OpKind, String)>),
}

#[derive(Debug, Default)]
pub(super) struct Document {
    pub defs: Vec<Def>,
    pub operations: Vec<Operation>,
    pub fragments: BTreeMap<String, Fragment>,
}

// ---- the parser -----------------------------------------------------------------------

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: u32,
}

type R<T> = Result<T, String>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
    }

    fn fail<T>(&self, what: &str) -> R<T> {
        Err(format!("line {}: {what}", self.line()))
    }

    fn found(&self) -> String {
        match self.peek() {
            Tok::Punct(c) => format!("`{c}`"),
            Tok::Spread => "`...`".into(),
            Tok::Name(n) => format!("`{n}`"),
            Tok::Number => "a number".into(),
            Tok::Str(_) => "a string".into(),
            Tok::Eof => "the end of the text".into(),
        }
    }

    fn enter(&mut self) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.fail("nested too deeply");
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn is(&self, c: char) -> bool {
        matches!(self.peek(), Tok::Punct(p) if *p == c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.is(c) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> R<()> {
        if self.eat(c) {
            Ok(())
        } else {
            self.fail(&format!("expected `{c}`, found {}", self.found()))
        }
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == w)
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if self.is_word(w) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn name(&mut self) -> R<String> {
        if let Tok::Name(n) = self.peek() {
            let n = n.clone();
            self.advance();
            Ok(n)
        } else {
            self.fail(&format!("expected a name, found {}", self.found()))
        }
    }

    fn description(&mut self) -> Option<String> {
        if let Tok::Str(s) = self.peek() {
            let s = s.clone();
            self.advance();
            Some(s)
        } else {
            None
        }
    }

    // ---- values and directives (skipped) ----

    fn skip_value(&mut self) -> R<()> {
        self.enter()?;
        match self.peek().clone() {
            Tok::Punct('$') => {
                self.advance();
                self.name()?;
            }
            Tok::Number | Tok::Str(_) | Tok::Name(_) => self.advance(),
            Tok::Punct('[') => {
                self.advance();
                while !self.is(']') {
                    if *self.peek() == Tok::Eof {
                        return self.fail("a list is not closed");
                    }
                    self.skip_value()?;
                }
                self.advance();
            }
            Tok::Punct('{') => {
                self.advance();
                while !self.is('}') {
                    self.name()?;
                    self.expect(':')?;
                    self.skip_value()?;
                }
                self.advance();
            }
            _ => return self.fail(&format!("expected a value, found {}", self.found())),
        }
        self.leave();
        Ok(())
    }

    /// `(name: value ...)`; the names.
    fn arguments(&mut self) -> R<Vec<String>> {
        let mut names = Vec::new();
        if self.eat('(') {
            while !self.is(')') {
                names.push(self.name()?);
                self.expect(':')?;
                self.skip_value()?;
            }
            self.advance();
        }
        Ok(names)
    }

    /// `@a(..) @b`; true if `@deprecated` is among them.
    fn directives(&mut self) -> R<bool> {
        let mut deprecated = false;
        while self.eat('@') {
            if self.name()? == "deprecated" {
                deprecated = true;
            }
            self.arguments()?;
        }
        Ok(deprecated)
    }

    // ---- types ----

    fn ty(&mut self) -> R<Ty> {
        self.enter()?;
        let inner = if self.eat('[') {
            let t = self.ty()?;
            self.expect(']')?;
            Ty::List(Box::new(t))
        } else {
            Ty::Named(self.name()?)
        };
        self.leave();
        Ok(if self.eat('!') {
            Ty::NonNull(Box::new(inner))
        } else {
            inner
        })
    }

    fn input_value(&mut self) -> R<InputValue> {
        let description = self.description();
        let name = self.name()?;
        self.expect(':')?;
        let ty = self.ty()?;
        let has_default = self.eat('=');
        if has_default {
            self.skip_value()?;
        }
        let deprecated = self.directives()?;
        Ok(InputValue {
            name,
            ty,
            has_default,
            deprecated,
            description,
        })
    }

    fn arguments_def(&mut self) -> R<Vec<InputValue>> {
        let mut args = Vec::new();
        if self.eat('(') {
            while !self.is(')') {
                args.push(self.input_value()?);
            }
            self.advance();
        }
        Ok(args)
    }

    // ---- definitions ----

    fn implements(&mut self) -> R<Vec<String>> {
        let mut out = Vec::new();
        if self.eat_word("implements") {
            self.eat('&');
            out.push(self.name()?);
            while self.eat('&') {
                out.push(self.name()?);
            }
        }
        Ok(out)
    }

    fn fields_def(&mut self) -> R<Vec<FieldDef>> {
        let mut fields = Vec::new();
        if self.eat('{') {
            while !self.is('}') {
                let description = self.description();
                let name = self.name()?;
                let args = self.arguments_def()?;
                self.expect(':')?;
                let ty = self.ty()?;
                let deprecated = self.directives()?;
                fields.push(FieldDef {
                    name,
                    ty,
                    args,
                    deprecated,
                    description,
                });
            }
            self.advance();
        }
        Ok(fields)
    }

    fn type_def(&mut self, kind: &str, description: Option<String>) -> R<TypeDef> {
        let name = self.name()?;
        let mut def = TypeDef {
            name,
            kind: TypeKind::Scalar,
            description,
            fields: Vec::new(),
            inputs: Vec::new(),
            implements: Vec::new(),
            members: Vec::new(),
            values: Vec::new(),
        };
        match kind {
            "scalar" => {
                self.directives()?;
            }
            "type" | "interface" => {
                def.kind = if kind == "type" {
                    TypeKind::Object
                } else {
                    TypeKind::Interface
                };
                def.implements = self.implements()?;
                self.directives()?;
                def.fields = self.fields_def()?;
            }
            "union" => {
                def.kind = TypeKind::Union;
                self.directives()?;
                if self.eat('=') {
                    self.eat('|');
                    def.members.push(self.name()?);
                    while self.eat('|') {
                        def.members.push(self.name()?);
                    }
                }
            }
            "enum" => {
                def.kind = TypeKind::Enum;
                self.directives()?;
                if self.eat('{') {
                    while !self.is('}') {
                        let description = self.description();
                        let name = self.name()?;
                        let deprecated = self.directives()?;
                        def.values.push(EnumValue {
                            name,
                            deprecated,
                            description,
                        });
                    }
                    self.advance();
                }
            }
            _ => {
                def.kind = TypeKind::Input;
                self.directives()?;
                if self.eat('{') {
                    while !self.is('}') {
                        def.inputs.push(self.input_value()?);
                    }
                    self.advance();
                }
            }
        }
        Ok(def)
    }

    fn op_kind(word: &str) -> Option<OpKind> {
        match word {
            "query" => Some(OpKind::Query),
            "mutation" => Some(OpKind::Mutation),
            "subscription" => Some(OpKind::Subscription),
            _ => None,
        }
    }

    fn schema_def(&mut self) -> R<Def> {
        self.directives()?;
        let mut roots = Vec::new();
        if self.eat('{') {
            while !self.is('}') {
                let word = self.name()?;
                let Some(kind) = Self::op_kind(&word) else {
                    return self.fail(&format!("`{word}` is no operation type"));
                };
                self.expect(':')?;
                roots.push((kind, self.name()?));
            }
            self.advance();
        }
        Ok(Def::Schema(roots))
    }

    fn directive_def(&mut self) -> R<Def> {
        self.expect('@')?;
        let name = self.name()?;
        let args = self.arguments_def()?;
        self.eat_word("repeatable");
        if !self.eat_word("on") {
            return self.fail(&format!("expected `on`, found {}", self.found()));
        }
        self.eat('|');
        self.name()?;
        while self.eat('|') {
            self.name()?;
        }
        Ok(Def::Directive(DirectiveDef { name, args }))
    }

    // ---- operations ----

    fn selection_set(&mut self) -> R<Vec<Sel>> {
        self.enter()?;
        self.expect('{')?;
        let mut sels = Vec::new();
        while !self.is('}') {
            if *self.peek() == Tok::Eof {
                return self.fail("a selection set is not closed");
            }
            sels.push(self.selection()?);
        }
        if sels.is_empty() {
            return self.fail("a selection set is empty");
        }
        self.advance();
        self.leave();
        Ok(sels)
    }

    fn selection(&mut self) -> R<Sel> {
        if matches!(self.peek(), Tok::Spread) {
            self.advance();
            if self.is_word("on") {
                self.advance();
                let on = self.name()?;
                self.directives()?;
                return Ok(Sel::Inline {
                    on: Some(on),
                    sel: self.selection_set()?,
                });
            }
            if matches!(self.peek(), Tok::Name(_)) {
                let name = self.name()?;
                self.directives()?;
                return Ok(Sel::Spread(name));
            }
            self.directives()?;
            return Ok(Sel::Inline {
                on: None,
                sel: self.selection_set()?,
            });
        }
        let first = self.name()?;
        let name = if self.eat(':') { self.name()? } else { first };
        let args = self.arguments()?;
        self.directives()?;
        let sel = if self.is('{') {
            self.selection_set()?
        } else {
            Vec::new()
        };
        Ok(Sel::Field { name, args, sel })
    }

    fn variables(&mut self) -> R<Vec<String>> {
        let mut vars = Vec::new();
        if self.eat('(') {
            while !self.is(')') {
                self.expect('$')?;
                self.name()?;
                self.expect(':')?;
                vars.push(self.ty()?.named().to_owned());
                if self.eat('=') {
                    self.skip_value()?;
                }
                self.directives()?;
            }
            self.advance();
        }
        Ok(vars)
    }

    fn document(&mut self) -> R<Document> {
        let mut doc = Document::default();
        while *self.peek() != Tok::Eof {
            if self.is('{') {
                doc.operations.push(Operation {
                    kind: OpKind::Query,
                    vars: Vec::new(),
                    sel: self.selection_set()?,
                });
                continue;
            }
            let description = self.description();
            let Tok::Name(word) = self.peek().clone() else {
                return self.fail(&format!("unexpected {}", self.found()));
            };
            self.advance();
            match word.as_str() {
                "query" | "mutation" | "subscription" => {
                    let kind = Self::op_kind(&word).unwrap_or(OpKind::Query);
                    if matches!(self.peek(), Tok::Name(_)) {
                        self.advance();
                    }
                    let vars = self.variables()?;
                    self.directives()?;
                    let sel = self.selection_set()?;
                    doc.operations.push(Operation { kind, vars, sel });
                }
                "fragment" => {
                    let name = self.name()?;
                    if !self.eat_word("on") {
                        return self.fail("expected `on`");
                    }
                    let on = self.name()?;
                    self.directives()?;
                    let sel = self.selection_set()?;
                    doc.fragments.insert(name, Fragment { on, sel });
                }
                "schema" => doc.defs.push(self.schema_def()?),
                "directive" => doc.defs.push(self.directive_def()?),
                "scalar" | "type" | "interface" | "union" | "enum" | "input" => {
                    let def = self.type_def(&word, description)?;
                    doc.defs.push(Def::Type { def, extend: false });
                }
                "extend" => {
                    let Tok::Name(what) = self.peek().clone() else {
                        return self.fail("expected what to extend");
                    };
                    self.advance();
                    match what.as_str() {
                        "schema" => doc.defs.push(self.schema_def()?),
                        "scalar" | "type" | "interface" | "union" | "enum" | "input" => {
                            let def = self.type_def(&what, None)?;
                            doc.defs.push(Def::Type { def, extend: true });
                        }
                        other => return self.fail(&format!("cannot extend `{other}`")),
                    }
                }
                other => {
                    return Err(format!("line {}: unexpected `{other}`", self.line_before()));
                }
            }
        }
        Ok(doc)
    }

    fn line_before(&self) -> u32 {
        self.toks[self.pos.saturating_sub(1)].line
    }
}

/// Largest text read.
pub(super) const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Reads `bytes` as GraphQL; errors say `line N: …` (the caller adds the file).
pub(super) fn parse(bytes: &[u8]) -> Result<Document, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "the file is larger than {} MB",
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "the file is not valid UTF-8".to_owned())?;
    let toks = lex(text)?;
    Parser {
        toks,
        pos: 0,
        depth: 0,
    }
    .document()
}
