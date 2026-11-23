---
id: create-request
version: 1
answer: json
profile: smart
---
Task: write one HTTP request from this description.

Description:
<<description>>

Target folder: <<folder>>

Context (JSON; siblings are the folder's other requests):
<<context>>

Do not set authentication: the request inherits it from its folder or collection. Query parameters belong in the URL; list them in params too. Answer with one json block of this shape:
<<include:request-shape>>
