#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -n "${ENV_FILE:-}" ]]; then
  CANDIDATE_ENV="$ENV_FILE"
elif [[ -f "$ROOT_DIR/.env" ]]; then
  CANDIDATE_ENV="$ROOT_DIR/.env"
elif [[ -f "$ROOT_DIR/../backend/.env" ]]; then
  CANDIDATE_ENV="$ROOT_DIR/../backend/.env"
else
  CANDIDATE_ENV=""
fi

if [[ -z "$CANDIDATE_ENV" || ! -f "$CANDIDATE_ENV" ]]; then
  cat <<'USAGE' >&2
Env file not found.
Set ENV_FILE or place .env in one of:
  - project root (.env)
  - ../backend/.env
USAGE
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$CANDIDATE_ENV"
set +a

SUPABASE_URL="${SUPABASE_URL:-}"
SUPABASE_SERVICE_ROLE_KEY="${SUPABASE_SERVICE_ROLE_KEY:-}"

if [[ -z "$SUPABASE_URL" || -z "$SUPABASE_SERVICE_ROLE_KEY" ]]; then
  cat <<'USAGE' >&2
Missing variables in env:
  - SUPABASE_URL
  - SUPABASE_SERVICE_ROLE_KEY
USAGE
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required but not installed." >&2
  exit 1
fi

API_URL="${SUPABASE_URL%/}/rest/v1/m_skill"
QUERY='select=*&order=skill_id.asc'

RESPONSE="$(
  curl -sS "${API_URL}?${QUERY}" \
    -H "apikey: ${SUPABASE_SERVICE_ROLE_KEY}" \
    -H "Authorization: Bearer ${SUPABASE_SERVICE_ROLE_KEY}" \
    -H "Accept-Profile: master"
)"

if command -v jq >/dev/null 2>&1; then
  echo "$RESPONSE" | jq .
else
  echo "$RESPONSE"
fi
