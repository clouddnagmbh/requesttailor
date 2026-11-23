---
id: draft-docs
version: 1
answer: json
profile: fast
---
Task: write a short description of this request in Markdown: what it does, when to use it, which parameters and body fields matter and what they mean. Say only what the request itself shows; do not invent behavior, and write nothing about a response you have not seen. Use variables as {{name}}, never credential values.

Context (JSON: the request as written):
<<context>>

Answer with one json block of this shape:
{
  "docs": "Markdown: at most about 150 words"
}
