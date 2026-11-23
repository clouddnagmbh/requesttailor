---
id: suggest-tests
version: 1
answer: json
profile: smart
---
Task: suggest tests for this request and its response: a tests script and declarative assertions. Check what matters for this endpoint (status, the shape and key fields of the body, important headers, time). Do not repeat assertions or tests the request already has.

Tests script API: test(name, fn) defines a test; expect(value) is chai's expect (expect(x).to.equal(y), .to.be.a("array"), .to.have.property("id"), .to.be.above(n), .to.include(x)); rt.response.status, rt.response.json(), rt.response.text(), rt.response.headers.get(name), rt.response.time (ms). <<include:assertions-api>>

Context (JSON: the request as written and the report of what was sent and received):
<<context>>

Answer with one json block of this shape:
{
  "text": "Markdown: what the tests check",
  "tests": "JavaScript for the request's tests script",
  "assertions": [{ "expression": "res.status", "operator": "eq", "value": "200", "description": "status is OK" }]
}
