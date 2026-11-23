// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Small specs for the OpenAPI wizard tests (written for these tests, Apache-2.0).

export const PETSTORE3 = {
  openapi: "3.0.3",
  info: { title: "Petstore", version: "1.0.0", description: "Pets and their orders." },
  servers: [
    { url: "https://petstore.example.com/v1", description: "Production" },
    { url: "https://staging.petstore.example.com/v1", description: "Staging" },
  ],
  tags: [
    { name: "pets", description: "Everything about your pets" },
    { name: "store", description: "Orders" },
    { name: "users", description: "People" },
  ],
  paths: {
    "/pets": {
      get: {
        tags: ["pets"],
        summary: "List all pets",
        operationId: "listPets",
        parameters: [{ name: "limit", in: "query", schema: { type: "integer", minimum: 1, maximum: 100 } }],
        responses: { "200": { description: "ok" } },
      },
    },
    "/pets/{petId}": {
      get: {
        tags: ["pets"],
        summary: "Info for a specific pet",
        operationId: "showPetById",
        parameters: [
          { name: "petId", in: "path", required: true, description: "The id of the pet to retrieve", schema: { type: "integer", minimum: 1 } },
        ],
        responses: { "200": { description: "A pet", content: { "application/json": { schema: { $ref: "#/components/schemas/Pet" } } } } },
      },
    },
    "/store/orders": {
      post: {
        tags: ["store"],
        summary: "Place an order",
        operationId: "placeOrder",
        requestBody: { required: true, content: { "application/json": { schema: { $ref: "#/components/schemas/Order" } } } },
        responses: { "201": { description: "created" } },
      },
    },
    "/users/{id}": {
      get: { tags: ["users"], summary: "Get a user", operationId: "getUser", parameters: [{ name: "id", in: "path", required: true, schema: { type: "string", format: "uuid" } }], responses: { "200": { description: "ok" } } },
    },
  },
  components: {
    schemas: {
      Pet: { type: "object", required: ["id", "name"], properties: { id: { type: "integer" }, name: { type: "string" } } },
      Order: {
        type: "object",
        required: ["petId", "quantity"],
        properties: {
          petId: { type: "integer", minimum: 1 },
          quantity: { type: "integer", minimum: 1, maximum: 10 },
          status: { type: "string", enum: ["placed", "approved", "delivered"] },
        },
      },
    },
  },
};

export const SWAGGER2 = {
  swagger: "2.0",
  info: { title: "Legacy Shop", version: "0.9" },
  host: "shop.example.com",
  basePath: "/api",
  schemes: ["https"],
  tags: [{ name: "items" }],
  paths: {
    "/items": { get: { tags: ["items"], summary: "List items", parameters: [{ name: "q", in: "query", type: "string" }], responses: { "200": { description: "ok" } } } },
    "/items/{id}": {
      get: { tags: ["items"], summary: "Get an item", parameters: [{ name: "id", in: "path", required: true, type: "integer" }], responses: { "200": { description: "ok" } } },
      put: {
        tags: ["items"],
        summary: "Replace an item",
        parameters: [
          { name: "id", in: "path", required: true, type: "integer" },
          { name: "body", in: "body", required: true, schema: { $ref: "#/definitions/Item" } },
        ],
        responses: { "200": { description: "ok" } },
      },
    },
  },
  definitions: { Item: { type: "object", properties: { name: { type: "string" } } } },
};
