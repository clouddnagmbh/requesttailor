// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GraphQlApi` of the mock backend (wave 8, strand B): the library schema of
// `rt-testserver`'s `POST /graphql`, as the real shells print it. `schema` counts its calls
// and how many reached the "server" (not cached), so UI tests can see caching and refresh.

import type { GraphQlApi } from "./backend";

export const MOCK_SDL = `"The root of all queries."
type Query {
  "Greets \`name\`."
  hello(name: String = "world"): String!
  books(genre: Genre): [Book!]!
  book(id: ID!): Book
}

type Mutation {
  addBook(input: BookInput!): Book!
}

"A book in the test library."
type Book {
  id: ID!
  title: String!
  genre: Genre
  year: Int @deprecated(reason: "Use published.")
  published: String
}

enum Genre {
  FICTION
  SCIENCE
}

input BookInput {
  title: String!
  genre: Genre = FICTION
}
`;

export interface MockGraphQlApi extends GraphQlApi {
  /** Calls that reached the server (first per key, and every refresh). */
  fetched: number;
}

export function mockGraphQl(): MockGraphQlApi {
  const cache = new Map<string, string>();
  const api: MockGraphQlApi = {
    fetched: 0,
    async schema(path, input, refresh) {
      const key = `${input.environment ?? ""}\n${path}`;
      const hit = cache.get(key);
      if (hit !== undefined && !refresh) return hit;
      api.fetched += 1;
      cache.set(key, MOCK_SDL);
      return MOCK_SDL;
    },
  };
  return api;
}
