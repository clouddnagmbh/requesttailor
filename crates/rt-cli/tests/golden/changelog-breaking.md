# Orders API 1.0.0 → 2.0.0

## Breaking changes

- `GET /health`: response 200 removed (affects `health.yml`)
- `GET /orders`: type of `limit` changed from integer to string (affects `orders/list-orders.yml`)
- `GET /orders`: new required query parameter region (affects `orders/list-orders.yml`)
- `GET /orders`: query parameter status removed (affects `orders/list-orders.yml`)
- `GET /orders`: field `[].total` removed (affects `orders/list-orders.yml`)
- `POST /orders`: new required field `currency` (affects `orders/create-order.yml`)
- `POST /orders`: field `total` removed (affects `orders/create-order.yml`)
- `GET /orders/{id}`: field `total` removed (affects `orders/get-order.yml`)
- `DELETE /orders/{id}`: operation removed (affects `orders/delete-order.yml`)

## Other changes

- `GET /health`: response 204 added (affects `health.yml`)
- `GET /orders`: summary changed from "List orders" to "List orders, paged" (affects `orders/list-orders.yml`)
- `GET /orders`: new optional query parameter offset (affects `orders/list-orders.yml`)
- `GET /orders`: new value cancelled allowed for `[].status` (affects `orders/list-orders.yml`)
- `GET /orders`: new field `[].currency` (affects `orders/list-orders.yml`)
- `POST /orders`: new field `coupon` (affects `orders/create-order.yml`)
- `POST /orders`: new value cancelled allowed for `status` (affects `orders/create-order.yml`)
- `POST /orders`: new field `currency` (affects `orders/create-order.yml`)
- `POST /orders`: response 409 added (affects `orders/create-order.yml`)
- `GET /orders/{id}`: summary changed from "Read an order" to "Read one order by its id" (affects `orders/get-order.yml`)
- `GET /orders/{id}`: new value cancelled allowed for `status` (affects `orders/get-order.yml`)
- `GET /orders/{id}`: new field `currency` (affects `orders/get-order.yml`)
- `GET /orders/{id}/items`: operation added
