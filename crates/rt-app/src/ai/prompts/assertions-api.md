---
id: assertions-api
version: 1
answer: none
profile: none
---
Assertions run without JavaScript: expression is res.status, res.body.<path> (e.g. res.body.items[0].id, res.body.items.length), res.headers.<name> or res.responseTime; operator is one of eq, neq, gt, gte, lt, lte, in, notIn, contains, notContains, length, matches, notMatches, startsWith, endsWith, between, isEmpty, isNotEmpty, isNull, isUndefined, isDefined, isTruthy, isFalsy, isJson, isNumber, isString, isBoolean, isArray; value is the expected value as text (empty for unary operators).
