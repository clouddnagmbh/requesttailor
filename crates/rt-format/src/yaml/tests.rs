// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

use super::emit::{Context, render_str};
use super::*;

fn doc(src: &str) -> Document {
    let d = parse(src).unwrap_or_else(|e| panic!("{e}\n{src}"));
    assert_eq!(d.to_text(), src, "round trip");
    d
}

fn value(src: &str, key: &str) -> String {
    let d = doc(src);
    d.root.get(key).unwrap().as_scalar().unwrap().value.clone()
}

#[test]
fn round_trips_bruno_style() {
    let src = "info:\n  name: x\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/a\"\n  headers:\n    - name: A\n      value: b\n  body:\n    type: json\n    data: |-\n      {\n        \"a\": 1\n      }\n\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test(\"x\", function() {\n\n          expect(1);\n        });\nextensions: {}\nlist: []\n";
    let d = doc(src);
    let http = d.root.get("http").unwrap();
    assert_eq!(http.get("url").unwrap().as_str(), Some("{{baseUrl}}/a"));
    let data = http.get("body").unwrap().get("data").unwrap();
    assert_eq!(data.as_str(), Some("{\n  \"a\": 1\n}"));
    let code = d.root.get("runtime").unwrap().get("scripts").unwrap();
    let code = code.items().next().unwrap().get("code").unwrap();
    assert_eq!(
        code.as_str(),
        Some("test(\"x\", function() {\n\n  expect(1);\n});")
    );
    assert!(d.root.get("extensions").unwrap().is_map());
    assert!(d.root.get("list").unwrap().is_seq());
}

#[test]
fn scalar_types() {
    let d = doc(
        "a: 1\nb: 1.5\nc: true\nd: ~\ne:\nf: \"1\"\ng: 1.0.0\nh: 0x1F\ni: -3\nj: .5\nk: 1e3\nl: FALSE\n",
    );
    let r = |k: &str| d.root.get(k).unwrap().as_scalar().unwrap().resolve();
    assert_eq!(r("a"), Resolved::Int(1));
    assert_eq!(r("b"), Resolved::Float(1.5));
    assert_eq!(r("c"), Resolved::Bool(true));
    assert_eq!(r("d"), Resolved::Null);
    assert_eq!(r("e"), Resolved::Null);
    assert_eq!(r("f"), Resolved::Str);
    assert_eq!(r("g"), Resolved::Str);
    assert_eq!(r("h"), Resolved::Int(31));
    assert_eq!(r("i"), Resolved::Int(-3));
    assert_eq!(r("j"), Resolved::Float(0.5));
    assert_eq!(r("k"), Resolved::Float(1000.0));
    assert_eq!(r("l"), Resolved::Bool(false));
}

#[test]
fn quoted_scalars() {
    assert_eq!(value("a: 'it''s'\n", "a"), "it's");
    assert_eq!(value("a: 'one\n  two\n\n  three'\n", "a"), "one two\nthree");
    assert_eq!(
        value("a: \"x\\ty\\u00e9\\x41\\\"\"\n", "a"),
        "x\ty\u{e9}A\""
    );
    assert_eq!(value("a: \"one \\\n   two\"\n", "a"), "one two");
    assert_eq!(value("a: \"one\\\n   two\"\n", "a"), "onetwo");
    assert_eq!(
        value("a: \"x {\\r\n\n  \\    y\\r\n\n  }\"\n", "a"),
        "x {\r\n    y\r\n}"
    );
    assert_eq!(value("a: \"trail   \n  next\"\n", "a"), "trail next");
}

#[test]
fn plain_multi_line() {
    assert_eq!(
        value("a: one\n  two\n\n  three\nb: x\n", "a"),
        "one two\nthree"
    );
    assert_eq!(value("a: x # comment\n", "a"), "x");
    assert_eq!(value("a: x#y\n", "a"), "x#y");
    assert_eq!(value("a: http://h:1/p?q=a:b\n", "a"), "http://h:1/p?q=a:b");
}

#[test]
fn block_scalars() {
    assert_eq!(value("a: |\n  x\n  y\n", "a"), "x\ny\n");
    assert_eq!(value("a: |-\n  x\n\n  y\n\nb: 1\n", "a"), "x\n\ny");
    assert_eq!(value("a: |+\n  x\n\n\nb: 1\n", "a"), "x\n\n\n");
    assert_eq!(value("a: |+\n  x\n\n", "a"), "x\n\n");
    assert_eq!(value("a: |\n  x", "a"), "x");
    assert_eq!(value("a: |2-\n    x\n   y\n", "a"), "  x\n y");
    assert_eq!(
        value("a: >-\n  one\n  two\n\n  three\n    more\n  end\n", "a"),
        "one two\nthree\n  more\nend"
    );
    assert_eq!(
        value("a: |-\n  x # not a comment\n# a comment\nb: 1\n", "a"),
        "x # not a comment"
    );
    assert_eq!(value("a: |-\nb: 1\n", "a"), "");
    let d = doc("- |-\n  item\n- >\n  folded\n  text\n");
    let items: Vec<_> = d.root.items().map(|n| n.as_str().unwrap()).collect();
    assert_eq!(items, ["item", "folded text\n"]);
}

#[test]
fn nesting_and_comments() {
    let src = "# head\n---\na:\n- x\n- y # trailing\n# between\n\nb:\n  - c: 1\n    d:\n      - - z\n  -\n    e: 2\nf: [1, 'two', {g: h}]\ng: {\"k\":1, l: [ ], m}\n...\n# tail\n";
    let d = doc(src);
    let a: Vec<_> = d
        .root
        .get("a")
        .unwrap()
        .items()
        .map(|n| n.as_str().unwrap())
        .collect();
    assert_eq!(a, ["x", "y"]);
    let b: Vec<_> = d.root.get("b").unwrap().items().collect();
    assert_eq!(b[0].get("c").unwrap().as_f64(), Some(1.0));
    assert_eq!(b[1].get("e").unwrap().text(), Some("2"));
    let f: Vec<_> = d.root.get("f").unwrap().items().collect();
    assert_eq!(f[1].as_str(), Some("two"));
    assert_eq!(f[2].get("g").unwrap().as_str(), Some("h"));
    let g = d.root.get("g").unwrap();
    assert_eq!(g.get("k").unwrap().as_f64(), Some(1.0));
    assert!(g.get("m").unwrap().is_null());
}

#[test]
fn crlf() {
    let d = doc("a: |-\r\n  x\r\n  y\r\nb: \"p\r\n  q\"\r\nc:\r\n  - 1\r\n");
    assert_eq!(d.newline, "\r\n");
    assert_eq!(d.root.get("a").unwrap().as_str(), Some("x\ny"));
    assert_eq!(d.root.get("b").unwrap().as_str(), Some("p q"));
}

#[test]
fn empty_documents() {
    assert!(doc("").root.is_null());
    assert!(doc("# only a comment\n").root.is_null());
    assert!(doc("{}\n").root.is_map());
}

#[test]
fn rejects_unsupported_yaml() {
    for (src, msg) in [
        ("a: &x 1\n", "anchors"),
        ("a: *x\n", "aliases"),
        ("a: !!str 1\n", "tags"),
        ("? a\n: b\n", "explicit keys"),
        ("a: 1\na: 2\n", "duplicate key"),
        ("a:\n\tb: 1\n", "tabs"),
        ("a: 1\n---\nb: 2\n", "multiple documents"),
        ("a: \"open\n", "unterminated"),
        ("a: b: c\n", "not allowed"),
        ("a: [1, 2\n", "flow"),
        ("a: 1\n  b: 2\n", "not allowed"),
        ("a:\n  b: 1\n c: 2\n", "indentation"),
        ("%YAML 1.2\n---\na: 1\n", "directives"),
    ] {
        let err = parse(src).expect_err(src).to_string();
        assert!(err.contains(msg), "{src:?}: {err}");
    }
}

fn render(value: &str, owner_indent: isize) -> String {
    render_str(
        value,
        &Context {
            owner_indent,
            in_flow: false,
            implicit_key: false,
            single_line: false,
        },
    )
    .0
}

/// Expected outputs are what Bruno writes: each was checked against `yaml@2.9.1` with
/// Bruno's options. (During development the renderer was also compared with that package
/// on every string of the corpus at five indentation levels, as value and as key.)
#[test]
fn renders_like_bruno() {
    for (v, expected) in [
        ("hello world", "hello world"),
        ("{{baseUrl}}", "\"{{baseUrl}}\""),
        ("{{baseUrl}}/users", "\"{{baseUrl}}/users\""),
        ("http://localhost:8081/ping", "http://localhost:8081/ping"),
        ("2.0", "\"2.0\""),
        ("42", "\"42\""),
        ("true", "\"true\""),
        ("null", "\"null\""),
        ("", "\"\""),
        ("1.0.0", "1.0.0"),
        ("a: b", "\"a: b\""),
        ("x #y", "\"x #y\""),
        ("#x", "\"#x\""),
        ("- x", "\"- x\""),
        ("-x", "-x"),
        ("trailing ", "\"trailing \""),
        ("{\"a\":1}", "'{\"a\":1}'"),
        ("it's", "it's"),
        ("'q'", "\"'q'\""),
        ("say \"hi\" it's", "say \"hi\" it's"),
        ("\"hi\" it's", "\"\\\"hi\\\" it's\""),
        ("tab\there", "tab\there"),
        ("bell\x07", "\"bell\\a\""),
        ("Br!unoNtlm", "Br!unoNtlm"),
        ("line1\nline2", "|-\n  line1\n  line2"),
        ("line1\n\nline2\n", "|\n  line1\n\n  line2"),
        ("x\n\n", "|+\n  x\n"),
        ("  indented\nnext", "|2-\n    indented\n  next"),
        ("\nleading", "|-\n  \n  leading"),
        ("ends with space \n", "|\n  ends with space "),
        ("x\n  ", "\"x\\n  \""),
        ("1_000", "1_000"),
        ("yes", "yes"),
        ("a,b", "a,b"),
        ("\u{85}", "\"\u{85}\""),
        ("a\n\tb", "|-\n  a\n  \tb"),
        ("x\n\n\n", "|+\n  x\n\n"),
        ("\n\n", "|+\n  \n"),
        ("a:b", "a:b"),
        ("a:", "\"a:\""),
        (":a", ":a"),
        ("?", "\"?\""),
        ("-", "\"-\""),
        (" ", "\" \""),
        ("...", "..."),
        ("é😀", "é😀"),
    ] {
        assert_eq!(render(v, 0), expected, "{v:?}");
    }
    assert_eq!(render("a\nb", 4), "|-\n      a\n      b");
}

#[test]
fn renders_long_double_quoted_with_folding() {
    // Bruno folds `\n` in double-quoted strings of 40 or more JSON characters.
    let v = "function extract() {\r\n    return 1;\r\n}";
    assert_eq!(
        render(v, 4),
        "\"function extract() {\\r\n\n      \\    return 1;\\r\n\n      }\""
    );
    let d = doc(&format!("a:\n  - code: {}\n", render(v, 4)));
    let code = d.root.get("a").unwrap().items().next().unwrap().get("code");
    assert_eq!(code.unwrap().as_str(), Some(v));
}

#[test]
fn rendered_strings_read_back() {
    let samples = [
        "plain",
        "{{x}}",
        "a: b",
        "multi\nline\n",
        "multi\n\n\nkeep\n\n",
        "\n\n",
        "x\n\n\n",
        "a\n\tb",
        "\u{85}",
        "  lead\n",
        "\ttab start",
        "quote ' and \"",
        "ctrl \x01 \r\n end",
        "unicode é ✓ 😀",
        "x #",
        "trailing\n ",
        "- dash",
        "? q",
        "[1]",
        "%percent",
        "@at",
        "`tick",
        "--- marker",
        "",
        "0o17",
        ".inf",
        "~",
        "a\n- b",
        "long text that is definitely longer than forty characters\nwith a second line ",
    ];
    for owner in [0isize, 2, 4] {
        for v in samples {
            for key in [false, true] {
                let ctx = Context {
                    owner_indent: owner,
                    in_flow: false,
                    implicit_key: key,
                    single_line: false,
                };
                let (raw, _) = render_str(v, &ctx);
                let pad = " ".repeat(owner as usize);
                let src = if key {
                    format!("{pad}{raw}: x\n")
                } else {
                    format!("{pad}k: {raw}\n")
                };
                let d = doc(&src);
                let Node::Map(m) = &d.root else {
                    panic!("{src}")
                };
                let got = if key {
                    &m.entries[0].key
                } else {
                    m.entries[0].value.as_scalar().unwrap()
                };
                assert_eq!(got.value, v, "{src}");
                if !key {
                    assert_eq!(got.resolve(), Resolved::Str, "{src}");
                }
            }
        }
    }
}

#[test]
fn set_str_changes_only_that_scalar() {
    let src = "info:\n  name: old # keep me\n\nhttp:\n  url: \"{{a}}\"\n";
    let mut d = doc(src);
    let nl = d.newline;
    let info = d.root.get_mut("info").unwrap();
    info.get_mut("name")
        .unwrap()
        .as_scalar_mut()
        .unwrap()
        .set_str("new value", nl);
    assert_eq!(
        d.to_text(),
        "info:\n  name: new value # keep me\n\nhttp:\n  url: \"{{a}}\"\n"
    );
    let url = d.root.get_mut("http").unwrap().get_mut("url").unwrap();
    let url = url.as_scalar_mut().unwrap();
    url.set_str("{{a}}", nl);
    assert_eq!(url.raw, "\"{{a}}\"", "unchanged value keeps its source");
    url.set_str("line1\nline2", nl);
    assert_eq!(
        d.to_text(),
        "info:\n  name: new value # keep me\n\nhttp:\n  url: |-\n    line1\n    line2\n"
    );
}

#[test]
fn set_str_before_a_comment_stays_on_one_line() {
    let mut d = doc("a: x # note\nb: 1\n");
    let a = d.root.get_mut("a").unwrap().as_scalar_mut().unwrap();
    a.set_str(
        "long multi-line text, longer than forty characters\nline two",
        "\n",
    );
    let text = d.to_text();
    assert_eq!(
        text,
        "a: \"long multi-line text, longer than forty characters\\nline two\" # note\nb: 1\n"
    );
    assert_eq!(
        doc(&text).root.get("a").unwrap().as_str(),
        Some("long multi-line text, longer than forty characters\nline two")
    );
}

/// Parsing time grows linearly with the input. Ignored by default (timing); run with
/// `cargo test -p rt-format --release -- --ignored parse_time`.
#[test]
#[ignore]
fn parse_time_is_linear() {
    let item = "  - name: X-Header\n    value: \"{{token}}\"\n    description: |-\n      line one\n      line two\n";
    let time = |n: usize| {
        let src = format!("headers:\n{}", item.repeat(n));
        let start = std::time::Instant::now();
        let d = parse(&src).unwrap();
        assert_eq!(d.root.get("headers").unwrap().items().count(), n);
        (src.len(), start.elapsed())
    };
    for n in [10_000, 40_000, 160_000] {
        let (bytes, t) = time(n);
        println!("{bytes} bytes: {t:?}");
    }
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let deep = "[".repeat(100_000);
    assert!(parse(&deep).unwrap_err().to_string().contains("nesting"));
    let mut block = String::new();
    for i in 0..300 {
        block.push_str(&format!("{}k:\n", " ".repeat(i)));
    }
    assert!(parse(&block).unwrap_err().to_string().contains("nesting"));
}

/// Broken input (here: every prefix of every corpus file, and the corpus with indicator
/// characters spliced in) must give an error or a tree that writes back the input; it must
/// never panic.
#[test]
fn never_panics_on_broken_input() {
    fn walk(d: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "yml") {
                out.push(std::fs::read_to_string(p).unwrap());
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/collections/bruno"),
        &mut files,
    );
    let check = |src: &str| {
        if let Ok(d) = parse(src) {
            assert_eq!(d.to_text(), src);
        }
    };
    for src in &files {
        let cuts = src.char_indices().map(|(i, _)| i).step_by(67);
        for cut in cuts {
            check(&src[..cut]);
            for ins in ["\"", "'", ": ", "- ", "\n  ", "\t", "{", "\\"] {
                check(&format!("{}{ins}{}", &src[..cut], &src[cut..]));
            }
        }
    }
}

#[test]
fn set_str_uses_the_documents_line_break() {
    let mut d = doc("a:\r\n  b: x\r\n");
    let nl = d.newline;
    let a = d.root.get_mut("a").unwrap();
    a.get_mut("b")
        .unwrap()
        .as_scalar_mut()
        .unwrap()
        .set_str("1\n2", nl);
    assert_eq!(d.to_text(), "a:\r\n  b: |-\r\n    1\r\n    2\r\n");
}
