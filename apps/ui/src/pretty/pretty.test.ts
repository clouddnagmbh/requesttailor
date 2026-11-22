// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { formatJson } from "./json";
import { bodyLanguage, languageFromContentType, prettyBody, sniffLanguage } from "./language";
import { formatXml } from "./xml";

const idempotent = (format: (s: string) => string | null, input: string) => {
  const once = format(input);
  expect(once).not.toBeNull();
  expect(format(once!)).toBe(once);
};

describe("formatJson", () => {
  it("indents nested objects and arrays by two spaces", () => {
    expect(formatJson('{"a":1,"b":[true,false,null],"c":{"d":"e"}}')).toBe(
      '{\n  "a": 1,\n  "b": [\n    true,\n    false,\n    null\n  ],\n  "c": {\n    "d": "e"\n  }\n}',
    );
  });

  it("re-indents already formatted JSON", () => {
    expect(formatJson('{\n\t"a" :\n\t\t[ 1 ,2 ]\n}')).toBe('{\n  "a": [\n    1,\n    2\n  ]\n}');
  });

  it("writes empty containers compactly", () => {
    expect(formatJson('{ "a": { }, "b": [\n] }')).toBe('{\n  "a": {},\n  "b": []\n}');
    expect(formatJson("[]")).toBe("[]");
    expect(formatJson(" {} ")).toBe("{}");
  });

  it("keeps number and string text exactly as written", () => {
    const input = '[12345678901234567890123,1.0,-0,1e400,1E-7,"\\u00e9\\n\\"q\\"","é"]';
    expect(formatJson(input)).toBe(
      '[\n  12345678901234567890123,\n  1.0,\n  -0,\n  1e400,\n  1E-7,\n  "\\u00e9\\n\\"q\\"",\n  "é"\n]',
    );
  });

  it("is not confused by brackets, colons, commas and escaped quotes in strings", () => {
    expect(formatJson('{"a{":"[,]:","b\\"}":"x\\\\"}')).toBe('{\n  "a{": "[,]:",\n  "b\\"}": "x\\\\"\n}');
  });

  it("keeps scalar documents", () => {
    expect(formatJson(' "x" ')).toBe('"x"');
    expect(formatJson("42")).toBe("42");
  });

  it("keeps {{variables}} in unquoted positions byte-identical", () => {
    expect(formatJson('{"id":{{id}},"tags":[{{tag}}, "x"],"s":"{{name}}"}')).toBe(
      '{\n  "id": {{id}},\n  "tags": [\n    {{tag}},\n    "x"\n  ],\n  "s": "{{name}}"\n}',
    );
    expect(formatJson('{ {{key}} : {{$randomInt}} }')).toBe("{\n  {{key}}: {{$randomInt}}\n}");
    expect(formatJson('{"n":{{a}}{{b}}, "m": 1{{c}}}')).toBe('{\n  "n": {{a}}{{b}},\n  "m": 1{{c}}\n}');
    expect(formatJson('[{{{a}}: 1}]')).toBe("[\n  {\n    {{a}}: 1\n  }\n]");
  });

  it("keeps one trailing newline and a byte order mark", () => {
    expect(formatJson('{"a":1}\n')).toBe('{\n  "a": 1\n}\n');
    expect(formatJson('﻿[1]')).toBe("﻿[\n  1\n]");
  });

  it("returns null for invalid JSON", () => {
    for (const bad of [
      "",
      "   ",
      "{",
      "[1,]",
      '{"a" 1}',
      '{"a":1,}',
      '{"a":1 "b":2}',
      '{1: 2}',
      "[1] [2]",
      '"open',
      "[01]",
      "[NaN]",
      "{}}",
      '["a\nb"]',
      "// comment\n{}",
    ]) {
      expect(formatJson(bad), JSON.stringify(bad)).toBeNull();
    }
  });

  it("is idempotent", () => {
    idempotent(formatJson, '{"a":[1,{"b":[]},[[{}]]],"c":{{x}},"d":"\\"}"}');
    idempotent(formatJson, "[]\n");
  });

  it("formats a large body quickly", () => {
    const big = JSON.stringify(Array.from({ length: 50_000 }, (_, i) => ({ id: i, name: `n${i}`, tags: ["a", "b"] })));
    const t = performance.now();
    const out = formatJson(big);
    // Generous: guards against quadratic behaviour, not a benchmark (machines and CI runners vary).
    expect(performance.now() - t).toBeLessThan(10_000);
    expect(JSON.stringify(JSON.parse(out!))).toBe(big);
  }, 30_000);
});

describe("formatXml", () => {
  it("indents element-only content", () => {
    expect(formatXml("<a><b><c/></b><d>text</d></a>")).toBe("<a>\n  <b>\n    <c/>\n  </b>\n  <d>text</d>\n</a>");
  });

  it("re-indents and drops whitespace-only text between elements", () => {
    expect(formatXml("<a>\n\t\t<b>1</b>   <b>2</b>\n</a>")).toBe("<a>\n  <b>1</b>\n  <b>2</b>\n</a>");
  });

  it("keeps attributes, comments, CDATA, processing instructions and the doctype verbatim", () => {
    const input =
      '<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE note [<!ENTITY x "y>z">]><!-- top --><note a=\'1\'  b="x > y" xmlns:p="urn:p"><p:to>Tove</p:to><!-- c --><body><![CDATA[<b>&</b>]]></body><?pi data?></note>';
    expect(formatXml(input)).toBe(
      [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<!DOCTYPE note [<!ENTITY x "y>z">]>',
        "<!-- top -->",
        '<note a=\'1\'  b="x > y" xmlns:p="urn:p">',
        "  <p:to>Tove</p:to>",
        "  <!-- c -->",
        "  <body><![CDATA[<b>&</b>]]></body>",
        "  <?pi data?>",
        "</note>",
      ].join("\n"),
    );
  });

  it("keeps mixed content inline as written", () => {
    expect(formatXml("<r><p>Hello <b>you</b>, &amp; bye</p><e></e><w> </w></r>")).toBe(
      "<r>\n  <p>Hello <b>you</b>, &amp; bye</p>\n  <e></e>\n  <w> </w>\n</r>",
    );
  });

  it("keeps multi-line start tags and {{variables}}", () => {
    expect(formatXml('<a\n   x="1"\n   y="{{y}}"><id>{{id}}</id></a>')).toBe('<a\n   x="1"\n   y="{{y}}">\n  <id>{{id}}</id>\n</a>');
  });

  it("formats SOAP envelopes", () => {
    const soap =
      '<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><m:Get xmlns:m="urn:m"><m:Id>7</m:Id></m:Get></soap:Body></soap:Envelope>';
    expect(formatXml(soap)).toBe(
      '<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/">\n  <soap:Body>\n    <m:Get xmlns:m="urn:m">\n      <m:Id>7</m:Id>\n    </m:Get>\n  </soap:Body>\n</soap:Envelope>',
    );
  });

  it("keeps one trailing newline", () => {
    expect(formatXml("<a><b/></a>\n")).toBe("<a>\n  <b/>\n</a>\n");
  });

  it("returns null for malformed XML", () => {
    for (const bad of [
      "",
      "text",
      "<a>",
      "<a></b>",
      "<a><b></a></b>",
      "</a>",
      "<a/>trailing",
      '<a b="1>',
      "<!-- open",
      "<a><![CDATA[x</a>",
      "<html><br></html>",
      "< a/>",
    ]) {
      expect(formatXml(bad), JSON.stringify(bad)).toBeNull();
    }
  });

  it("formats a large body quickly", () => {
    const big = `<items>${Array.from({ length: 50_000 }, (_, i) => `<item id="${i}"><name>n${i}</name><tags><t>a</t><t>b</t></tags></item>`).join("")}</items>`;
    const t = performance.now();
    const out = formatXml(big);
    // Generous: guards against quadratic behaviour, not a benchmark (machines and CI runners vary).
    expect(performance.now() - t).toBeLessThan(10_000);
    expect(out!.split("\n")).toHaveLength(2 + 50_000 * 7);
  }, 30_000);

  it("is idempotent", () => {
    idempotent(formatXml, '<?xml version="1.0"?><a><b x="1"><c>t</c><!-- k --></b><d/><m>a<i>b</i>c</m></a>');
  });
});

describe("language detection", () => {
  it("maps content types", () => {
    const cases: [string, string | null][] = [
      ["application/json", "json"],
      ["application/json; charset=utf-8", "json"],
      ["application/problem+json", "json"],
      ["application/vnd.api+json", "json"],
      ["text/json", "json"],
      ["application/xml", "xml"],
      ["text/xml; charset=ISO-8859-1", "xml"],
      ["application/soap+xml", "xml"],
      ["application/atom+xml", "xml"],
      ["text/html; charset=utf-8", "html"],
      ["application/xhtml+xml", "html"],
      ["application/javascript", "javascript"],
      ["text/javascript", "javascript"],
      ["TEXT/CSS", "css"],
      ["text/plain", null],
      ["application/octet-stream", null],
      ["", null],
    ];
    for (const [ct, lang] of cases) expect(languageFromContentType(ct), ct).toBe(lang);
    expect(languageFromContentType("application/yaml")).toBe("text");
    expect(languageFromContentType("text/csv")).toBe("text");
  });

  it("sniffs the first non-blank character", () => {
    expect(sniffLanguage('  \n{"a":1}')).toBe("json");
    expect(sniffLanguage("﻿[1]")).toBe("json");
    expect(sniffLanguage("<?xml version='1.0'?><a/>")).toBe("xml");
    expect(sniffLanguage("<!DOCTYPE html><html></html>")).toBe("html");
    expect(sniffLanguage("\n<html lang=en>")).toBe("html");
    expect(sniffLanguage("hello")).toBe("text");
    expect(sniffLanguage("")).toBe("text");
  });

  it("sniffs only when the type is missing or generic", () => {
    expect(bodyLanguage(null, "{}")).toBe("json");
    expect(bodyLanguage("text/plain; charset=utf-8", "<a/>")).toBe("xml");
    expect(bodyLanguage("text/html", "{}")).toBe("html");
    expect(bodyLanguage("text/csv", "[1]")).toBe("text");
  });
});

describe("prettyBody", () => {
  it("formats JSON and XML and falls back to the text as it is", () => {
    expect(prettyBody('{"a":1}', "json")).toEqual({ text: '{\n  "a": 1\n}', formatted: true });
    expect(prettyBody("{oops", "json")).toEqual({ text: "{oops", formatted: false });
    expect(prettyBody("<a><b/></a>", "xml")).toEqual({ text: "<a>\n  <b/>\n</a>", formatted: true });
    expect(prettyBody("<a><b></a>", "xml")).toEqual({ text: "<a><b></a>", formatted: false });
  });

  it("never reformats HTML, JavaScript or CSS", () => {
    for (const lang of ["html", "javascript", "css", "text"] as const) {
      expect(prettyBody("<pre>  x\n</pre>{a:1}", lang)).toEqual({ text: "<pre>  x\n</pre>{a:1}", formatted: false });
    }
  });
});
