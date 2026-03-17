#!/usr/bin/env bash
# Fetch m_skill + m_skill_effect from the remote DB and display as JSON.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# .env resolution (same as list-skills-from-env.sh)
if [[ -n "${ENV_FILE:-}" ]]; then
  CANDIDATE_ENV="$ENV_FILE"
elif [[ -f "$ROOT_DIR/.env" ]]; then
  CANDIDATE_ENV="$ROOT_DIR/.env"
elif [[ -f "$ROOT_DIR/../backend/.env" ]]; then
  CANDIDATE_ENV="$ROOT_DIR/../backend/.env"
else
  echo "Env file not found. Set ENV_FILE or place .env in project root." >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$CANDIDATE_ENV"
set +a

BASE="${SUPABASE_URL%/}/rest/v1"
KEY="${SUPABASE_SERVICE_ROLE_KEY}"
HEADERS=(-H "apikey: $KEY" -H "Authorization: Bearer $KEY" -H "Accept-Profile: master")

fetch() {
  curl -sS "${HEADERS[@]}" "$1"
}

echo "=== m_skill ==="
fetch "${BASE}/m_skill?select=skill_id,skill_name,skill_type,target_rule,effect_summary_type,trigger_timing,parse_status,proc_chance,duration_turns,script_hook,params_json&order=skill_id.asc" \
  | python3 -m json.tool

echo ""
echo "=== m_skill_effect ==="
fetch "${BASE}/m_skill_effect?select=skill_effect_id,skill_id,effect_order,effect_type,target_rule,trigger_timing,proc_chance,duration_turns,radius,value_num,value_text,params_json,is_active&order=skill_id.asc,effect_order.asc" \
  | python3 -m json.tool
