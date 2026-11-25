# Orders walkthrough

This notebook logs in, creates an order and reads it back. Prose is kept as written,
including *emphasis*, `code` and a fenced example that stays text:

```yaml
# not a request block
ref: nothing.yml
```

```rt-request
id: login
method: POST
url: {{base}}/login
headers:
  Accept: application/json
body: {"user":"alice"}
set:
  token: $.access_token
  userId: $.user.id
```

Now read the orders of that user.

```rt-request
id: list
method: GET
url: {{base}}/orders
headers:
  Authorization: Bearer {{token}}
show:
  - response
  - table: $.items[*]
  - chart: $.items[*].total
```

The same with a request file of the collection:

```rt-request
ref: orders/get-order.yml
```
