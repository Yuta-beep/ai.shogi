#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${ENV_FILE:-$ROOT_DIR/.env}"

if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$ENV_FILE"
  set +a
fi

DB_URL="${SUPABASE_DB_URL:-${DATABASE_URL:-${POSTGRES_URL:-}}}"

if [[ -z "$DB_URL" ]]; then
  cat <<'USAGE' >&2
DB URL not found.
Set one of these in .env or environment:
  - SUPABASE_DB_URL
  - DATABASE_URL
  - POSTGRES_URL
USAGE
  exit 1
fi

if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required but not installed." >&2
  exit 1
fi

usage() {
  cat <<'USAGE'
Usage:
  scripts/run-sql-from-env.sh -c "SELECT now();"
  scripts/run-sql-from-env.sh -f sql/check_fk.sql

Options:
  -c <sql>    Execute inline SQL
  -f <file>   Execute SQL file
USAGE
}

if [[ $# -lt 2 ]]; then
  usage
  exit 1
fi

case "$1" in
  -c)
    shift
    psql "$DB_URL" -v ON_ERROR_STOP=1 -c "$*"
    ;;
  -f)
    shift
    psql "$DB_URL" -v ON_ERROR_STOP=1 -f "$1"
    ;;
  *)
    usage
    exit 1
    ;;
esac
