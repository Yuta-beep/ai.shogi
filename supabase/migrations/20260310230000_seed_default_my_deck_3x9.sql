begin;

-- 1) 3x9 のデフォルト配置テンプレートをDBで管理
create table if not exists public.default_deck_template_placements (
  row_no integer not null,
  col_no integer not null,
  piece_id bigint not null references master.m_piece(piece_id),
  primary key (row_no, col_no),
  constraint default_deck_template_row_chk check (row_no between 0 and 2),
  constraint default_deck_template_col_chk check (col_no between 0 and 8)
);

-- 2) player_deck_placements も 3x9 制約へ
alter table public.player_deck_placements
  drop constraint if exists player_deck_placements_row_chk;

alter table public.player_deck_placements
  add constraint player_deck_placements_row_chk
  check (row_no between 0 and 2) not valid;

-- 3) テンプレート初期値（通常将棋の自陣3段）
-- row=2: 香桂銀金王/玉金銀桂香
-- row=1: 歩 x9
-- row=0: 角(1), 飛(7)
insert into public.default_deck_template_placements (row_no, col_no, piece_id)
select v.row_no, v.col_no, p.piece_id
from (
  values
    (2,0,'香'),(2,1,'桂'),(2,2,'銀'),(2,3,'金'),(2,4,'王'),(2,5,'金'),(2,6,'銀'),(2,7,'桂'),(2,8,'香'),
    (1,0,'歩'),(1,1,'歩'),(1,2,'歩'),(1,3,'歩'),(1,4,'歩'),(1,5,'歩'),(1,6,'歩'),(1,7,'歩'),(1,8,'歩'),
    (0,1,'角'),(0,7,'飛')
) as v(row_no, col_no, kanji)
join master.m_piece p on p.kanji = v.kanji
on conflict (row_no, col_no) do update
set piece_id = excluded.piece_id;

-- 王が無い構成向けフォールバック（玉）
insert into public.default_deck_template_placements (row_no, col_no, piece_id)
select 2, 4, p.piece_id
from master.m_piece p
where p.kanji = '玉'
  and not exists (select 1 from master.m_piece k where k.kanji = '王')
on conflict (row_no, col_no) do update
set piece_id = excluded.piece_id;

-- 4) ユーザーのマイデッキを作成/再構築する関数
create or replace function public.ensure_default_my_deck_for_user(p_user_id uuid)
returns void
language plpgsql
security definer
set search_path = public, master
as $$
declare
  v_deck_id bigint;
begin
  select d.deck_id
    into v_deck_id
  from public.player_decks d
  where d.player_id = p_user_id
    and d.name = 'マイデッキ'
  order by d.deck_id asc
  limit 1;

  if v_deck_id is null then
    insert into public.player_decks (player_id, name)
    values (p_user_id, 'マイデッキ')
    returning deck_id into v_deck_id;
  end if;

  delete from public.player_deck_placements
  where deck_id = v_deck_id;

  insert into public.player_deck_placements (deck_id, row_no, col_no, piece_id)
  select v_deck_id, t.row_no, t.col_no, t.piece_id
  from public.default_deck_template_placements t;

  update public.player_decks
  set updated_at = now()
  where deck_id = v_deck_id;
end;
$$;

-- 5) 既存トリガー関数を拡張（新規ユーザー時に自動投入）
create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
begin
  insert into public.players (id)
  values (new.id)
  on conflict (id) do nothing;

  insert into public.player_owned_pieces (player_id, piece_id, source)
  select new.id, p.piece_id, 'initial'
  from master.m_piece p
  where p.kanji in ('歩', '香', '桂', '銀', '金', '角', '飛', '王', '玉')
  on conflict (player_id, piece_id) do nothing;

  perform public.ensure_default_my_deck_for_user(new.id);

  return new;
end;
$$;

-- 6) 既存ユーザーのうち、マイデッキ未作成の人だけバックフィル
do $$
declare
  r record;
begin
  for r in
    select p.id as player_id
    from public.players p
    where not exists (
      select 1
      from public.player_decks d
      where d.player_id = p.id
        and d.name = 'マイデッキ'
    )
  loop
    perform public.ensure_default_my_deck_for_user(r.player_id);
  end loop;
end
$$;

commit;
