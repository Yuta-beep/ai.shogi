-- Seed missing gacha piece master data (piece/move/skill/rule/effect) and bind to gacha pools.

begin;

with piece_defs as (
  select *
  from (
    values
      ('爆', '爆', 'piece_gacha_baku', 'move_gacha_baku', 'gold', 'UR', 'skill_gacha_baku', '爆発で周囲の敵駒を吹き飛ばす破壊的な駒。', '爆発で周囲の敵駒を吹き飛ばす。', 'hihen', 1),
      ('煽', '煽', 'piece_gacha_aori', 'move_gacha_aori', 'rook', 'SR', 'skill_gacha_aori', '相手を煽りたい人の為に。', '相手の戦術を乱す。', 'hihen', 2),
      ('室', '室', 'piece_gacha_muro', 'move_gacha_muro', 'gold', 'SR', 'skill_gacha_muro', 'セーフルームを用意して「王」を守る。', '王を守る結界を展開する。', 'ukanmuri', 2),
      ('定', '定', 'piece_gacha_sadame', 'move_gacha_sadame', 'pawn', 'R', 'skill_gacha_sadame', '相手の戦略を固定しろ。', '敵の行動を固定する。', 'ukanmuri', 3),
      ('安', '安', 'piece_gacha_an', 'move_gacha_an', 'gold', 'R', 'skill_gacha_an', '敵の駒を安くする。', '敵の能力効率を下げる。', 'ukanmuri', 3),
      ('宋', '宋', 'piece_gacha_so', 'move_gacha_so', 'gold', 'UR', 'skill_gacha_so', '味方に繁栄をもたらす。', '味方に繁栄の加護を与える。', 'ukanmuri', 1),
      ('辺', '辺', 'piece_gacha_hen', 'move_gacha_hen', 'silver', 'SR', 'skill_gacha_hen', '盤面の辺を利用した戦略。', '盤端からの攻めを強化する。', 'shinnyo', 2),
      ('逸', '逸', 'piece_gacha_itsu', 'move_gacha_itsu', 'silver', 'R', 'skill_gacha_itsu', '敵駒を盤面から逸脱させる。', '敵の狙いを逸らす。', 'shinnyo', 3),
      ('進', '進', 'piece_gacha_shin', 'move_gacha_shin', 'king', 'R', 'skill_gacha_shin', '次はどこに進んでいくのか。', '味方の進軍を促進する。', 'shinnyo', 3),
      ('逃', '逃', 'piece_gacha_to', 'move_gacha_to', 'king', 'UR', 'skill_gacha_to', '移動すると味方の王も同じ方向へ逃がす緊急離脱の駒。', '味方の王を緊急退避させる。', 'shinnyo', 1),
      ('艸', '艸', 'piece_gacha_sou', 'move_gacha_sou', 'gold', 'UR', 'skill_gacha_sou', '草の力を操り盤面を支配する自然の駒。', '自然の力で盤面を制御する。', 'kanken1', 1),
      ('閹', '閹', 'piece_gacha_en', 'move_gacha_en', 'gold', 'UR', 'skill_gacha_en', '敵の動きを封じる封印の駒。', '敵の行動を封じる。', 'kanken1', 1),
      ('賚', '賚', 'piece_gacha_lai', 'move_gacha_lai', 'gold', 'SSR', 'skill_gacha_lai', '報酬を与え味方を強化する恩恵の駒。', '味方に恩恵を与える。', 'kanken1', 1),
      ('殲', '殲', 'piece_gacha_sen', 'move_gacha_sen', 'gold', 'SSR', 'skill_gacha_sen', '敵を一掃する殲滅の駒。', '敵陣を殲滅する。', 'kanken1', 1),
      ('膠', '膠', 'piece_gacha_ko', 'move_gacha_ko', 'gold', 'UR', 'skill_gacha_ko', '盤面を膠着させ敵の動きを止める粘着の駒。', '敵の行動を鈍化させる。', 'kanken1', 1)
  ) as v(
    kanji,
    name,
    piece_code,
    move_code,
    base_move_code,
    rarity,
    skill_code,
    skill_name,
    skill_desc,
    gacha_code,
    gacha_weight
  )
),
base_vectors as (
  select *
  from (
    values
      ('pawn', 0, -1, 1),
      ('silver', -1, -1, 1),
      ('silver', 0, -1, 1),
      ('silver', 1, -1, 1),
      ('silver', -1, 1, 1),
      ('silver', 1, 1, 1),
      ('gold', -1, -1, 1),
      ('gold', 0, -1, 1),
      ('gold', 1, -1, 1),
      ('gold', -1, 0, 1),
      ('gold', 1, 0, 1),
      ('gold', 0, 1, 1),
      ('rook', 0, -1, 8),
      ('rook', 0, 1, 8),
      ('rook', -1, 0, 8),
      ('rook', 1, 0, 8),
      ('king', -1, -1, 1),
      ('king', 0, -1, 1),
      ('king', 1, -1, 1),
      ('king', -1, 0, 1),
      ('king', 1, 0, 1),
      ('king', -1, 1, 1),
      ('king', 0, 1, 1),
      ('king', 1, 1, 1)
  ) as v(base_move_code, dx, dy, max_step)
)
insert into master.m_move_pattern (
  move_code,
  move_name,
  is_repeatable,
  can_jump,
  constraints_json,
  is_active,
  created_at,
  updated_at
)
select
  d.move_code,
  d.name,
  false,
  false,
  jsonb_build_object('mode', 'piece_info_canMoveTo', 'source_move_code', d.base_move_code),
  true,
  now(),
  now()
from piece_defs d
on conflict (move_code) do update
set
  move_name = excluded.move_name,
  constraints_json = excluded.constraints_json,
  is_active = true,
  updated_at = now();

with piece_defs as (
  select *
  from (
    values
      ('move_gacha_baku', 'gold'),
      ('move_gacha_aori', 'rook'),
      ('move_gacha_muro', 'gold'),
      ('move_gacha_sadame', 'pawn'),
      ('move_gacha_an', 'gold'),
      ('move_gacha_so', 'gold'),
      ('move_gacha_hen', 'silver'),
      ('move_gacha_itsu', 'silver'),
      ('move_gacha_shin', 'king'),
      ('move_gacha_to', 'king'),
      ('move_gacha_sou', 'gold'),
      ('move_gacha_en', 'gold'),
      ('move_gacha_lai', 'gold'),
      ('move_gacha_sen', 'gold'),
      ('move_gacha_ko', 'gold')
  ) as v(move_code, base_move_code)
),
base_vectors as (
  select *
  from (
    values
      ('pawn', 0, -1, 1),
      ('silver', -1, -1, 1),
      ('silver', 0, -1, 1),
      ('silver', 1, -1, 1),
      ('silver', -1, 1, 1),
      ('silver', 1, 1, 1),
      ('gold', -1, -1, 1),
      ('gold', 0, -1, 1),
      ('gold', 1, -1, 1),
      ('gold', -1, 0, 1),
      ('gold', 1, 0, 1),
      ('gold', 0, 1, 1),
      ('rook', 0, -1, 8),
      ('rook', 0, 1, 8),
      ('rook', -1, 0, 8),
      ('rook', 1, 0, 8),
      ('king', -1, -1, 1),
      ('king', 0, -1, 1),
      ('king', 1, -1, 1),
      ('king', -1, 0, 1),
      ('king', 1, 0, 1),
      ('king', -1, 1, 1),
      ('king', 0, 1, 1),
      ('king', 1, 1, 1)
  ) as v(base_move_code, dx, dy, max_step)
)
insert into master.m_move_pattern_vector (move_pattern_id, dx, dy, max_step, capture_only, move_only)
select
  mp.move_pattern_id,
  bv.dx,
  bv.dy,
  bv.max_step,
  false,
  false
from piece_defs d
join master.m_move_pattern mp on mp.move_code = d.move_code
join base_vectors bv on bv.base_move_code = d.base_move_code
on conflict (move_pattern_id, dx, dy, capture_only, move_only) do nothing;

with piece_defs as (
  select *
  from (
    values
      ('move_gacha_baku', 'gold'),
      ('move_gacha_aori', 'rook'),
      ('move_gacha_muro', 'gold'),
      ('move_gacha_sadame', 'pawn'),
      ('move_gacha_an', 'gold'),
      ('move_gacha_so', 'gold'),
      ('move_gacha_hen', 'silver'),
      ('move_gacha_itsu', 'silver'),
      ('move_gacha_shin', 'king'),
      ('move_gacha_to', 'king'),
      ('move_gacha_sou', 'gold'),
      ('move_gacha_en', 'gold'),
      ('move_gacha_lai', 'gold'),
      ('move_gacha_sen', 'gold'),
      ('move_gacha_ko', 'gold')
  ) as v(move_code, base_move_code)
)
insert into master.m_move_pattern_rule (
  move_pattern_id,
  rule_type,
  priority,
  params_json,
  is_active,
  created_at,
  updated_at
)
select
  mp.move_pattern_id,
  'custom',
  100,
  jsonb_build_object('mode', 'piece_info_canMoveTo', 'source_move_code', d.base_move_code),
  true,
  now(),
  now()
from piece_defs d
join master.m_move_pattern mp on mp.move_code = d.move_code
where not exists (
  select 1
  from master.m_move_pattern_rule r
  where r.move_pattern_id = mp.move_pattern_id
    and r.rule_type = 'custom'
    and r.priority = 100
    and r.is_active = true
);

with skill_defs as (
  select *
  from (
    values
      ('skill_gacha_baku', '爆発で周囲の敵駒を吹き飛ばす破壊的な駒。'),
      ('skill_gacha_aori', '相手を煽りたい人の為に。'),
      ('skill_gacha_muro', 'セーフルームを用意して「王」を守る。'),
      ('skill_gacha_sadame', '相手の戦略を固定しろ。'),
      ('skill_gacha_an', '敵の駒を安くする。'),
      ('skill_gacha_so', '味方に繁栄をもたらす。'),
      ('skill_gacha_hen', '盤面の辺を利用した戦略。'),
      ('skill_gacha_itsu', '敵駒を盤面から逸脱させる。'),
      ('skill_gacha_shin', '次はどこに進んでいくのか。'),
      ('skill_gacha_to', '移動すると味方の王も同じ方向へ逃がす緊急離脱の駒。'),
      ('skill_gacha_sou', '草の力を操り盤面を支配する自然の駒。'),
      ('skill_gacha_en', '敵の動きを封じる封印の駒。'),
      ('skill_gacha_lai', '報酬を与え味方を強化する恩恵の駒。'),
      ('skill_gacha_sen', '敵を一掃する殲滅の駒。'),
      ('skill_gacha_ko', '盤面を膠着させ敵の動きを止める粘着の駒。')
  ) as v(skill_code, skill_desc)
)
insert into master.m_skill (
  skill_code,
  skill_name,
  skill_desc,
  trigger_timing,
  skill_type,
  target_rule,
  effect_summary_type,
  parse_status,
  params_json,
  is_active,
  created_at,
  updated_at
)
select
  d.skill_code,
  d.skill_code,
  d.skill_desc,
  'on_move',
  'active_or_passive',
  'unspecified',
  'scripted',
  'manual',
  '{}'::jsonb,
  true,
  now(),
  now()
from skill_defs d
on conflict (skill_code) do update
set
  skill_desc = excluded.skill_desc,
  is_active = true,
  updated_at = now();

with skill_defs as (
  select *
  from (
    values
      ('skill_gacha_baku', '敵を吹き飛ばす効果'),
      ('skill_gacha_aori', '敵を攪乱する効果'),
      ('skill_gacha_muro', '王を守る効果'),
      ('skill_gacha_sadame', '敵行動固定効果'),
      ('skill_gacha_an', '敵弱体化効果'),
      ('skill_gacha_so', '味方強化効果'),
      ('skill_gacha_hen', '盤端戦術効果'),
      ('skill_gacha_itsu', '逸脱効果'),
      ('skill_gacha_shin', '進軍補助効果'),
      ('skill_gacha_to', '緊急離脱効果'),
      ('skill_gacha_sou', '盤面制御効果'),
      ('skill_gacha_en', '封印効果'),
      ('skill_gacha_lai', '恩恵付与効果'),
      ('skill_gacha_sen', '殲滅効果'),
      ('skill_gacha_ko', '膠着効果')
  ) as v(skill_code, value_text)
)
insert into master.m_skill_effect (
  skill_id,
  effect_order,
  effect_type,
  target_rule,
  trigger_timing,
  value_text,
  params_json,
  is_active,
  created_at,
  updated_at
)
select
  s.skill_id,
  1,
  'custom',
  'unspecified',
  'on_move',
  d.value_text,
  '{}'::jsonb,
  true,
  now(),
  now()
from skill_defs d
join master.m_skill s on s.skill_code = d.skill_code
on conflict (skill_id, effect_order) do update
set
  effect_type = excluded.effect_type,
  target_rule = excluded.target_rule,
  trigger_timing = excluded.trigger_timing,
  value_text = excluded.value_text,
  params_json = excluded.params_json,
  is_active = true,
  updated_at = now();

with piece_defs as (
  select *
  from (
    values
      ('爆', '爆', 'piece_gacha_baku', 'move_gacha_baku', 'skill_gacha_baku', 'UR', 'hihen', 1),
      ('煽', '煽', 'piece_gacha_aori', 'move_gacha_aori', 'skill_gacha_aori', 'SR', 'hihen', 2),
      ('室', '室', 'piece_gacha_muro', 'move_gacha_muro', 'skill_gacha_muro', 'SR', 'ukanmuri', 2),
      ('定', '定', 'piece_gacha_sadame', 'move_gacha_sadame', 'skill_gacha_sadame', 'R', 'ukanmuri', 3),
      ('安', '安', 'piece_gacha_an', 'move_gacha_an', 'skill_gacha_an', 'R', 'ukanmuri', 3),
      ('宋', '宋', 'piece_gacha_so', 'move_gacha_so', 'skill_gacha_so', 'UR', 'ukanmuri', 1),
      ('辺', '辺', 'piece_gacha_hen', 'move_gacha_hen', 'skill_gacha_hen', 'SR', 'shinnyo', 2),
      ('逸', '逸', 'piece_gacha_itsu', 'move_gacha_itsu', 'skill_gacha_itsu', 'R', 'shinnyo', 3),
      ('進', '進', 'piece_gacha_shin', 'move_gacha_shin', 'skill_gacha_shin', 'R', 'shinnyo', 3),
      ('逃', '逃', 'piece_gacha_to', 'move_gacha_to', 'skill_gacha_to', 'UR', 'shinnyo', 1),
      ('艸', '艸', 'piece_gacha_sou', 'move_gacha_sou', 'skill_gacha_sou', 'UR', 'kanken1', 1),
      ('閹', '閹', 'piece_gacha_en', 'move_gacha_en', 'skill_gacha_en', 'UR', 'kanken1', 1),
      ('賚', '賚', 'piece_gacha_lai', 'move_gacha_lai', 'skill_gacha_lai', 'SSR', 'kanken1', 1),
      ('殲', '殲', 'piece_gacha_sen', 'move_gacha_sen', 'skill_gacha_sen', 'SSR', 'kanken1', 1),
      ('膠', '膠', 'piece_gacha_ko', 'move_gacha_ko', 'skill_gacha_ko', 'UR', 'kanken1', 1)
  ) as v(kanji, name, piece_code, move_code, skill_code, rarity, gacha_code, gacha_weight)
)
insert into master.m_piece (
  piece_code,
  kanji,
  name,
  move_pattern_id,
  skill_id,
  move_description_ja,
  rarity,
  image_source,
  image_bucket,
  image_key,
  image_version,
  is_active,
  published_at,
  unpublished_at,
  created_at,
  updated_at
)
select
  d.piece_code,
  d.kanji,
  d.name,
  mp.move_pattern_id,
  s.skill_id,
  d.name || 'の移動ルール',
  d.rarity,
  'supabase',
  null,
  null,
  1,
  true,
  now(),
  '2026-04-01 23:59:59+09',
  now(),
  now()
from piece_defs d
join master.m_move_pattern mp on mp.move_code = d.move_code
join master.m_skill s on s.skill_code = d.skill_code
where not exists (
  select 1
  from master.m_piece p
  where p.kanji = d.kanji
);

-- Ensure rarity for all gacha target pieces.
update master.m_piece as p
set rarity = v.rarity
from (
  values
    ('爆', 'UR'),
    ('煽', 'SR'),
    ('灯', 'R'),
    ('室', 'SR'),
    ('定', 'R'),
    ('安', 'R'),
    ('宋', 'UR'),
    ('辺', 'SR'),
    ('逸', 'R'),
    ('進', 'R'),
    ('逃', 'UR'),
    ('艸', 'UR'),
    ('閹', 'UR'),
    ('賚', 'SSR'),
    ('殲', 'SSR'),
    ('膠', 'UR')
) as v(kanji, rarity)
where p.kanji = v.kanji;

-- Assign known local image keys for currently available gacha piece image files.
update master.m_piece as p
set
  image_source = 'supabase',
  image_bucket = 'piece-images',
  image_key = v.image_key,
  image_version = 1,
  updated_at = now()
from (
  values
    ('爆', 'pieces/gacha/爆.png'),
    ('煽', 'pieces/gacha/煽.png'),
    ('灯', 'pieces/gacha/灯.png'),
    ('室', 'pieces/gacha/室.png'),
    ('定', 'pieces/gacha/定.png'),
    ('安', 'pieces/gacha/安.png'),
    ('宋', 'pieces/gacha/宋.png')
) as v(kanji, image_key)
where p.kanji = v.kanji;

-- Backfill gacha-piece relation after piece insertion.
with rel as (
  select *
  from (
    values
      ('hihen', '爆', 1),
      ('hihen', '煽', 2),
      ('hihen', '灯', 3),
      ('ukanmuri', '室', 2),
      ('ukanmuri', '定', 3),
      ('ukanmuri', '安', 3),
      ('ukanmuri', '宋', 1),
      ('shinnyo', '辺', 2),
      ('shinnyo', '逸', 3),
      ('shinnyo', '進', 3),
      ('shinnyo', '逃', 1),
      ('kanken1', '艸', 1),
      ('kanken1', '閹', 1),
      ('kanken1', '賚', 1),
      ('kanken1', '殲', 1),
      ('kanken1', '膠', 1)
  ) as v(gacha_code, kanji, weight)
)
insert into master.m_gacha_piece (gacha_id, piece_id, weight, is_active, created_at, updated_at)
select
  g.gacha_id,
  p.piece_id,
  r.weight,
  true,
  now(),
  now()
from rel r
join master.m_gacha g on g.gacha_code = r.gacha_code
join master.m_piece p on p.kanji = r.kanji
on conflict (gacha_id, piece_id) do update
set
  weight = excluded.weight,
  is_active = true,
  updated_at = now();

commit;
