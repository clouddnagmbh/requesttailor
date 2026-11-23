---
id: suggest-names
version: 1
answer: json
profile: fast
---
Task: <<ask>> Give up to five proposals, best first, each different from the others and from the current name. Use the collection's naming style where the context shows one.

Context (JSON):
<<context>>

Answer with one json block of this shape:
{
  "names": ["first proposal", "second proposal"]
}
