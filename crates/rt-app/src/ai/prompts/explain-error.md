---
id: explain-error
version: 1
answer: json
profile: smart
---
Task: explain why this request failed (or did not do what was expected) and how to fix it: in the request, its variables, auth or the server.

Context (JSON: the request as written and the report of what was sent and received):
<<context>>

Answer with one json block of this shape:
{
  "text": "Markdown: the most likely cause first, then concrete steps to fix it"
}
