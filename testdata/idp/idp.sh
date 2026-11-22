#!/usr/bin/env sh
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 The RequestTailor Authors
#
# Test identity provider for the OAuth 2.0 integration tests: Keycloak (official image,
# pinned) with the realm in realm-requesttailor.json, on http://localhost:${RT_IDP_PORT:-8180}.
#
#   testdata/idp/idp.sh start    start (or restart) the container and wait until it is ready
#   testdata/idp/idp.sh stop     stop and remove the container
#   testdata/idp/idp.sh status   print the URL if it answers, exit 1 otherwise
#
# Then: RT_IDP_URL=http://localhost:8180 cargo test -p rt-core --test oauth2_idp

set -eu

IMAGE="quay.io/keycloak/keycloak:26.8.2"
NAME="${RT_IDP_CONTAINER:-rt-idp}"
PORT="${RT_IDP_PORT:-8180}"
HERE="$(cd "$(dirname "$0")" && pwd)"
URL="http://localhost:${PORT}"
READY="${URL}/realms/requesttailor/.well-known/openid-configuration"

ready() {
  curl -fsS -o /dev/null "$READY" 2>/dev/null
}

case "${1:-}" in
  start)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    # The realm file is copied in (not bind-mounted), so this also works where the Docker
    # daemon cannot see the checkout (remote daemons, some CI setups).
    docker create --name "$NAME" -p "${PORT}:8080" \
      -e KC_BOOTSTRAP_ADMIN_USERNAME=admin \
      -e KC_BOOTSTRAP_ADMIN_PASSWORD=admin \
      "$IMAGE" start-dev --import-realm >/dev/null
    staging="$(mktemp -d)"
    cp "$HERE/realm-requesttailor.json" "$staging/"
    # mktemp -d is 0700; Keycloak runs as an unprivileged user and must read it.
    chmod 755 "$staging"
    chmod 644 "$staging/realm-requesttailor.json"
    # `dir/.` into a missing destination creates it with the directory's contents.
    docker cp "$staging/." "$NAME:/opt/keycloak/data/import"
    rm -rf "$staging"
    docker start "$NAME" >/dev/null
    printf 'waiting for Keycloak on %s ' "$URL"
    i=0
    until ready; do
      i=$((i + 1))
      running="$(docker inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null || echo false)"
      if [ "$i" -gt 180 ] || [ "$running" != "true" ]; then
        echo
        docker logs --tail 50 "$NAME" >&2
        echo "Keycloak did not become ready" >&2
        exit 1
      fi
      printf '.'
      sleep 1
    done
    echo " ready"
    echo "RT_IDP_URL=$URL"
    ;;
  stop)
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    ;;
  status)
    if ready; then
      echo "$URL"
    else
      echo "not running" >&2
      exit 1
    fi
    ;;
  *)
    echo "usage: $0 start|stop|status" >&2
    exit 2
    ;;
esac
