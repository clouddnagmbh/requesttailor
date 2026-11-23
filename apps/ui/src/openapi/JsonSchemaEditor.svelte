<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 The RequestTailor Authors -->
<!-- The body of "Ready to send?" as JSON in CodeMirror: completion of property names and enum
     values from the operation's schema at the cursor, inline errors from the same validation
     as the form (`form.ts`). Controlled: `value` in, `onchange` out. -->
<script lang="ts">
  import { autocompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
  import { syntaxTree } from "@codemirror/language";
  import { linter, type Diagnostic } from "@codemirror/lint";
  import { Annotation, EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { onMount } from "svelte";

  import { base, language, multiLine } from "../editor/codemirror";
  import { asSchema, effective, validateJson } from "./form";

  let { value, onchange, schema, label }: { value: string; onchange: (v: string) => void; schema: unknown; label: string } = $props();

  const external = Annotation.define<boolean>();
  let host: HTMLDivElement;
  let view: EditorView | undefined;

  /** Property names from the root to the node at `pos` (`Property` nodes of @lezer/json). */
  function pathAt(state: EditorState, pos: number): string[] {
    const out: string[] = [];
    for (let node = syntaxTree(state).resolveInner(pos, -1); node.parent; node = node.parent) {
      if (node.name === "Property") {
        const key = node.getChild("PropertyName");
        if (key && !(pos >= key.from && pos <= key.to)) out.unshift(state.sliceDoc(key.from + 1, key.to - 1));
      }
    }
    return out;
  }

  function schemaAt(path: string[]): Record<string, unknown> {
    let s = effective(schema);
    for (const name of path) {
      if (s.type === "array" || s.items) s = effective(s.items);
      s = effective(asSchema(s.properties)[name]);
    }
    if ((s.type === "array" || s.items) && !s.properties) s = effective(s.items);
    return s;
  }

  function complete(ctx: CompletionContext): CompletionResult | null {
    const word = ctx.matchBefore(/"?[\w$-]*/);
    if (!word || (word.from === word.to && !ctx.explicit)) return null;
    const node = syntaxTree(ctx.state).resolveInner(ctx.pos, -1);
    const inValue = node.name === "String" && node.parent?.name === "Property";
    const path = pathAt(ctx.state, ctx.pos);
    if (inValue) {
      const target = schemaAt(path);
      const values = Array.isArray(target.enum) ? target.enum : [];
      if (!values.length) return null;
      return { from: node.from, options: values.map((v) => ({ label: JSON.stringify(v), type: "enum" })) };
    }
    const target = schemaAt(path);
    const required = new Set(Array.isArray(target.required) ? (target.required as string[]) : []);
    const props = Object.entries(asSchema(target.properties));
    if (!props.length) return null;
    return {
      from: word.from,
      options: props.map(([name, p]) => ({
        label: `"${name}"`,
        apply: `"${name}": `,
        type: "property",
        detail: [String(asSchema(p).type ?? ""), required.has(name) ? "required" : ""].filter(Boolean).join(" · "),
        info: typeof asSchema(p).description === "string" ? (asSchema(p).description as string) : undefined,
      })),
    };
  }

  const lint = linter(
    (v): Diagnostic[] => {
      const text = v.state.doc.toString();
      if (text.trim() === "" || /\{\{[^}]+\}\}/.test(text)) return [];
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch (e) {
        const at = /position (\d+)/.exec(String(e))?.[1];
        const from = Math.min(at ? Number(at) : 0, text.length);
        return [{ from, to: Math.min(from + 1, text.length), severity: "error", message: "Not valid JSON" }];
      }
      return validateJson(schema, parsed)
        .slice(0, 10)
        .map((err) => {
          const name = /\.([\w$-]+)$/.exec(err.path)?.[1];
          const at = name ? text.indexOf(`"${name}"`) : -1;
          const from = at >= 0 ? at : 0;
          return { from, to: at >= 0 ? from + name!.length + 2 : Math.min(1, text.length), severity: "error", message: `${err.path}: ${err.message}` };
        });
    },
    { delay: 250 },
  );

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: value,
        extensions: [
          base(),
          multiLine(),
          language("json"),
          autocompletion({ override: [complete], activateOnTyping: true }),
          lint,
          EditorView.contentAttributes.of({ "aria-label": label, "data-testid": "ready-body-json" }),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged || u.transactions.some((tr) => tr.annotation(external))) return;
            onchange(u.state.doc.toString());
          }),
        ],
      }),
    });
    return () => view?.destroy();
  });

  $effect(() => {
    const v = value;
    if (!view) return;
    const current = view.state.doc.toString();
    if (v === current) return;
    view.dispatch({ changes: { from: 0, to: current.length, insert: v }, annotations: external.of(true) });
  });
</script>

<div class="json" bind:this={host}></div>

<style>
  .json :global(.cm-editor) {
    min-height: 160px;
    max-height: 320px;
  }
  .json :global(.cm-scroller) {
    overflow: auto;
  }
</style>
