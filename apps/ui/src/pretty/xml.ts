// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Token-level XML pretty printer (two spaces). Tags with their attributes, comments, CDATA,
// processing instructions and the doctype are copied exactly as written. Only whitespace
// between the children of an element that holds nothing but elements, comments and
// processing instructions is replaced; an element with text or CDATA in it (`<a>text</a>`,
// mixed content) is copied as written, so no text ever changes.

const INDENT = "  ";
const NAME_START = /[A-Za-z_:À-￿]/;

type Node =
  | { kind: "element"; start: number; end: number; open: string; close: string; children: Node[] }
  | { kind: "text"; start: number; end: number; blank: boolean }
  | { kind: "cdata" | "markup"; start: number; end: number };

/** Index after the `>` that ends the markup starting at `i`, skipping quoted values; -1 if none. */
function tagEnd(text: string, i: number, brackets: boolean): number {
  let quote = 0;
  let depth = 0;
  for (let j = i; j < text.length; j++) {
    const c = text.charCodeAt(j);
    if (quote !== 0) {
      if (c === quote) quote = 0;
    } else if (c === 0x22 || c === 0x27) {
      quote = c;
    } else if (brackets && c === 0x5b) {
      depth++;
    } else if (brackets && c === 0x5d) {
      depth--;
    } else if (c === 0x3e && depth <= 0) {
      return j + 1;
    }
  }
  return -1;
}

function after(text: string, i: number, terminator: string): number {
  const j = text.indexOf(terminator, i);
  return j < 0 ? -1 : j + terminator.length;
}

/** Top-level nodes, or `null` if the text is not well-formed enough to re-indent. */
function parse(text: string, from: number): Node[] | null {
  const root: Node[] = [];
  const open: { node: Node & { kind: "element" }; name: string; children: Node[] }[] = [];
  const children = () => (open.length > 0 ? open[open.length - 1].children : root);
  let i = from;
  const n = text.length;
  while (i < n) {
    const lt = text.indexOf("<", i);
    if (lt !== i) {
      const end = lt < 0 ? n : lt;
      const blank = text.slice(i, end).trim() === "";
      if (!blank && open.length === 0) return null;
      children().push({ kind: "text", start: i, end, blank });
      i = end;
      continue;
    }
    let end: number;
    if (text.startsWith("<!--", i)) {
      end = after(text, i + 4, "-->");
    } else if (text.startsWith("<![CDATA[", i)) {
      if (open.length === 0) return null;
      end = after(text, i + 9, "]]>");
      if (end < 0) return null;
      children().push({ kind: "cdata", start: i, end });
      i = end;
      continue;
    } else if (text.startsWith("<?", i)) {
      end = after(text, i + 2, "?>");
    } else if (text.startsWith("<!", i)) {
      end = tagEnd(text, i + 2, true);
    } else if (text.startsWith("</", i)) {
      end = text.indexOf(">", i);
      if (end < 0) return null;
      end++;
      const name = text.slice(i + 2, end - 1).trimEnd();
      const top = open.pop();
      if (!top || top.name !== name) return null;
      top.node.close = text.slice(i, end);
      top.node.end = end;
      i = end;
      continue;
    } else {
      if (!NAME_START.test(text.charAt(i + 1))) return null;
      end = tagEnd(text, i + 1, false);
      if (end < 0) return null;
      const tag = text.slice(i, end);
      const name = /^<([^\s/>]+)/.exec(tag)![1];
      const node: Node & { kind: "element" } = { kind: "element", start: i, end, open: tag, close: "", children: [] };
      children().push(node);
      if (!tag.endsWith("/>")) open.push({ node, name, children: node.children });
      i = end;
      continue;
    }
    if (end < 0) return null;
    children().push({ kind: "markup", start: i, end });
    i = end;
  }
  if (open.length > 0) return null;
  return root.some((x) => x.kind === "element") ? root : null;
}

/** An element is re-indented only if it has markup children and no text in it. */
function isBlock(node: Node & { kind: "element" }): boolean {
  let markup = false;
  for (const c of node.children) {
    if (c.kind === "cdata" || (c.kind === "text" && !c.blank)) return false;
    if (c.kind !== "text") markup = true;
  }
  return markup;
}

/**
 * The XML text re-indented by two spaces, or `null` if it is malformed (unbalanced or
 * mismatched tags, unterminated markup, text outside the root). A trailing newline and a byte
 * order mark are kept.
 */
export function formatXml(text: string): string | null {
  const bom = text.charCodeAt(0) === 0xfeff ? "﻿" : "";
  const nodes = parse(text, bom.length);
  if (!nodes) return null;
  const out: string[] = [bom];
  const indents = [""];
  const indent = (depth: number) => {
    while (indents.length <= depth) indents.push(indents[indents.length - 1] + INDENT);
    return indents[depth];
  };
  const write = (node: Node, depth: number) => {
    if (node.kind === "element" && isBlock(node)) {
      out.push(node.open);
      for (const c of node.children) {
        if (c.kind === "text") continue;
        out.push("\n", indent(depth + 1));
        write(c, depth + 1);
      }
      out.push("\n", indent(depth), node.close);
    } else {
      out.push(text.slice(node.start, node.end));
    }
  };
  let first = true;
  for (const node of nodes) {
    if (node.kind === "text") continue;
    if (!first) out.push("\n");
    first = false;
    write(node, 0);
  }
  if (text.endsWith("\n")) out.push("\n");
  return out.join("");
}
