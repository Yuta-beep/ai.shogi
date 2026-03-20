begin;

alter table public.player_owned_pieces
  drop constraint if exists player_owned_pieces_source_chk;

alter table public.player_owned_pieces
  add constraint player_owned_pieces_source_chk
  check (source in ('gacha', 'shop', 'initial', 'stage_clear'));

insert into master.m_reward (reward_code, reward_type, reward_name, item_code, is_active)
values
  ('currency_pawn', 'currency', '歩', 'pawn', true),
  ('currency_gold', 'currency', '金', 'gold', true)
on conflict (reward_code) do update
set
  reward_type = excluded.reward_type,
  reward_name = excluded.reward_name,
  item_code = excluded.item_code,
  is_active = true,
  updated_at = now();

with currency_reward_map as (
  select s.stage_id, r.reward_id, t.reward_timing, t.quantity, t.sort_order
  from master.m_stage s
  cross join lateral (
    values
      ('first_clear'::text, 'currency_pawn'::text, 10::integer, 1::smallint),
      ('first_clear'::text, 'currency_gold'::text, 2::integer, 2::smallint),
      ('clear'::text, 'currency_pawn'::text, 2::integer, 1::smallint)
  ) as t(reward_timing, reward_code, quantity, sort_order)
  join master.m_reward r on r.reward_code = t.reward_code
)
insert into master.m_stage_reward (
  stage_id,
  reward_id,
  reward_timing,
  quantity,
  sort_order,
  is_active
)
select
  stage_id,
  reward_id,
  reward_timing,
  quantity,
  sort_order,
  true
from currency_reward_map
on conflict (stage_id, reward_id, reward_timing, sort_order) do update
set
  quantity = excluded.quantity,
  is_active = true,
  updated_at = now();

with piece_rewards(stage_no, kanji, quantity, sort_order) as (
  values
    (2, '忍', 1, 10),
    (2, '影', 1, 11),
    (2, '砲', 1, 12),
    (4, '竜', 1, 10),
    (4, '鳳', 1, 11),
    (5, '炎', 1, 10),
    (5, '火', 1, 11),
    (6, '水', 1, 10),
    (6, '波', 1, 11),
    (7, '木', 1, 10),
    (7, '葉', 1, 11),
    (8, '光', 1, 10),
    (8, '星', 1, 11),
    (9, '闇', 1, 10),
    (9, '魔', 1, 11),
    (10, '銅', 1, 10),
    (10, '鉄', 1, 11),
    (10, '錫', 1, 12),
    (10, '鉛', 1, 13),
    (11, '宝', 1, 10),
    (12, '電', 1, 10),
    (12, '雷', 1, 11),
    (13, '時', 1, 10),
    (14, '氷', 1, 10),
    (14, '雪', 1, 11),
    (15, '砂', 1, 10),
    (15, '風', 1, 11),
    (16, '苔', 1, 10),
    (16, '魚', 1, 11),
    (17, '雲', 1, 10),
    (17, '虹', 1, 11),
    (18, '毒', 1, 10),
    (18, '沼', 1, 11),
    (19, '鏡', 1, 10),
    (19, '映', 1, 11),
    (20, 'あ', 1, 10),
    (21, '牢', 1, 10),
    (21, '柵', 1, 11),
    (22, '嶺', 1, 10),
    (22, '峰', 1, 11),
    (22, '山', 1, 12),
    (23, '岩', 1, 10),
    (23, '鉱', 1, 11),
    (24, '霊', 1, 10),
    (24, '墓', 1, 11),
    (25, '幻', 1, 10),
    (25, '霧', 1, 11),
    (26, '月', 1, 10),
    (26, '舟', 1, 11),
    (27, '機', 1, 10),
    (27, '歯', 1, 11),
    (28, '家', 1, 10),
    (28, '民', 1, 11),
    (28, '畑', 1, 12),
    (29, '泉', 1, 10),
    (30, 'K', 1, 10),
    (30, '実', 1, 11),
    (30, '異', 1, 12),
    (31, '刀', 1, 10),
    (31, '鎧', 1, 11),
    (31, '銃', 1, 12),
    (32, '書', 1, 10),
    (32, '封', 1, 11),
    (33, '轟', 1, 10),
    (33, '犇', 1, 11),
    (34, '礼', 1, 10),
    (34, '聖', 1, 11),
    (35, '剣', 1, 10),
    (35, '盾', 1, 11),
    (36, '病', 1, 10),
    (36, '薬', 1, 11),
    (37, '滝', 1, 10),
    (38, '穴', 1, 10),
    (38, '淵', 1, 11),
    (39, '鬼', 1, 10),
    (40, '朧', 1, 10),
    (40, '死', 1, 11),
    (40, '魂', 1, 12),
    (41, '獣', 1, 10),
    (41, '禽', 1, 11),
    (42, '悟', 1, 10),
    (42, '心', 1, 11),
    (43, '鬱', 1, 10),
    (43, '乙', 1, 11),
    (44, '薔', 1, 10),
    (44, '菊', 1, 11),
    (44, '桜', 1, 12),
    (45, '凹', 1, 10),
    (45, '凸', 1, 11),
    (46, '焼', 1, 10),
    (46, '炒', 1, 11),
    (46, '煮', 1, 12),
    (47, '陽', 1, 10),
    (47, '陰', 1, 11),
    (48, '牛', 1, 10),
    (48, '豚', 1, 11),
    (48, '鶏', 1, 12),
    (49, '銭', 1, 10),
    (49, '財', 1, 11),
    (50, '巨', 1, 10)
), mapped_piece_rewards as (
  select
    pr.stage_no,
    s.stage_id,
    p.piece_id,
    p.name,
    pr.quantity,
    pr.sort_order
  from piece_rewards pr
  join master.m_stage s
    on s.stage_no = pr.stage_no
  join master.m_piece p
    on p.kanji = pr.kanji
)
insert into master.m_reward (reward_code, reward_type, reward_name, piece_id, is_active)
select distinct
  'piece_' || mpr.piece_id,
  'piece',
  '駒: ' || coalesce(mpr.name, mpr.piece_id::text),
  mpr.piece_id,
  true
from mapped_piece_rewards mpr
on conflict (reward_code) do update
set
  reward_type = excluded.reward_type,
  reward_name = excluded.reward_name,
  piece_id = excluded.piece_id,
  is_active = true,
  updated_at = now();

with piece_rewards(stage_no, kanji, quantity, sort_order) as (
  values
    (2, '忍', 1, 10),
    (2, '影', 1, 11),
    (2, '砲', 1, 12),
    (4, '竜', 1, 10),
    (4, '鳳', 1, 11),
    (5, '炎', 1, 10),
    (5, '火', 1, 11),
    (6, '水', 1, 10),
    (6, '波', 1, 11),
    (7, '木', 1, 10),
    (7, '葉', 1, 11),
    (8, '光', 1, 10),
    (8, '星', 1, 11),
    (9, '闇', 1, 10),
    (9, '魔', 1, 11),
    (10, '銅', 1, 10),
    (10, '鉄', 1, 11),
    (10, '錫', 1, 12),
    (10, '鉛', 1, 13),
    (11, '宝', 1, 10),
    (12, '電', 1, 10),
    (12, '雷', 1, 11),
    (13, '時', 1, 10),
    (14, '氷', 1, 10),
    (14, '雪', 1, 11),
    (15, '砂', 1, 10),
    (15, '風', 1, 11),
    (16, '苔', 1, 10),
    (16, '魚', 1, 11),
    (17, '雲', 1, 10),
    (17, '虹', 1, 11),
    (18, '毒', 1, 10),
    (18, '沼', 1, 11),
    (19, '鏡', 1, 10),
    (19, '映', 1, 11),
    (20, 'あ', 1, 10),
    (21, '牢', 1, 10),
    (21, '柵', 1, 11),
    (22, '嶺', 1, 10),
    (22, '峰', 1, 11),
    (22, '山', 1, 12),
    (23, '岩', 1, 10),
    (23, '鉱', 1, 11),
    (24, '霊', 1, 10),
    (24, '墓', 1, 11),
    (25, '幻', 1, 10),
    (25, '霧', 1, 11),
    (26, '月', 1, 10),
    (26, '舟', 1, 11),
    (27, '機', 1, 10),
    (27, '歯', 1, 11),
    (28, '家', 1, 10),
    (28, '民', 1, 11),
    (28, '畑', 1, 12),
    (29, '泉', 1, 10),
    (30, 'K', 1, 10),
    (30, '実', 1, 11),
    (30, '異', 1, 12),
    (31, '刀', 1, 10),
    (31, '鎧', 1, 11),
    (31, '銃', 1, 12),
    (32, '書', 1, 10),
    (32, '封', 1, 11),
    (33, '轟', 1, 10),
    (33, '犇', 1, 11),
    (34, '礼', 1, 10),
    (34, '聖', 1, 11),
    (35, '剣', 1, 10),
    (35, '盾', 1, 11),
    (36, '病', 1, 10),
    (36, '薬', 1, 11),
    (37, '滝', 1, 10),
    (38, '穴', 1, 10),
    (38, '淵', 1, 11),
    (39, '鬼', 1, 10),
    (40, '朧', 1, 10),
    (40, '死', 1, 11),
    (40, '魂', 1, 12),
    (41, '獣', 1, 10),
    (41, '禽', 1, 11),
    (42, '悟', 1, 10),
    (42, '心', 1, 11),
    (43, '鬱', 1, 10),
    (43, '乙', 1, 11),
    (44, '薔', 1, 10),
    (44, '菊', 1, 11),
    (44, '桜', 1, 12),
    (45, '凹', 1, 10),
    (45, '凸', 1, 11),
    (46, '焼', 1, 10),
    (46, '炒', 1, 11),
    (46, '煮', 1, 12),
    (47, '陽', 1, 10),
    (47, '陰', 1, 11),
    (48, '牛', 1, 10),
    (48, '豚', 1, 11),
    (48, '鶏', 1, 12),
    (49, '銭', 1, 10),
    (49, '財', 1, 11),
    (50, '巨', 1, 10)
), mapped_piece_rewards as (
  select
    s.stage_id,
    p.piece_id,
    pr.quantity,
    pr.sort_order
  from piece_rewards pr
  join master.m_stage s
    on s.stage_no = pr.stage_no
  join master.m_piece p
    on p.kanji = pr.kanji
), stage_piece_reward_map as (
  select
    mpr.stage_id,
    r.reward_id,
    'first_clear'::text as reward_timing,
    mpr.quantity,
    mpr.sort_order
  from mapped_piece_rewards mpr
  join master.m_reward r
    on r.reward_code = 'piece_' || mpr.piece_id
)
insert into master.m_stage_reward (
  stage_id,
  reward_id,
  reward_timing,
  quantity,
  sort_order,
  is_active
)
select
  stage_id,
  reward_id,
  reward_timing,
  quantity,
  sort_order,
  true
from stage_piece_reward_map
on conflict (stage_id, reward_id, reward_timing, sort_order) do update
set
  quantity = excluded.quantity,
  is_active = true,
  updated_at = now();

commit;
