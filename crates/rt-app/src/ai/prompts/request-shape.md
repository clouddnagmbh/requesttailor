---
id: request-shape
version: 1
answer: none
profile: none
---
{
  "text": "one or two sentences about the request (Markdown)",
  "request": {
    "name": "short name",
    "method": "GET | POST | PUT | PATCH | DELETE | HEAD | OPTIONS",
    "url": "{{baseUrl}}/path?query=value",
    "headers": [{ "name": "Content-Type", "value": "application/json" }],
    "params": [{ "name": "query", "value": "value", "kind": "query | path" }],
    "body": { "type": "none | json | text | xml | form", "data": "body text for json, text, xml", "fields": [{ "name": "", "value": "" }] }
  }
}
