begin;

alter table public.player_owned_pieces
  add column if not exists quantity integer not null default 1;

do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'player_owned_pieces_quantity_chk'
  ) then
    alter table public.player_owned_pieces
      add constraint player_owned_pieces_quantity_chk check (quantity >= 1);
  end if;
end
$$;

create or replace function public.seed_initial_owned_pieces(p_user_id uuid)
returns void
language plpgsql
security definer
set search_path = public, master
as $$
begin
  with standard_piece_qty as (
    select piece_id, qty
    from (
      values
        ('歩', 9),
        ('香', 2),
        ('桂', 2),
        ('銀', 2),
        ('金', 2),
        ('角', 1),
        ('飛', 1)
    ) as v(kanji, qty)
    join master.m_piece p on p.kanji = v.kanji
    union all
    select p.piece_id, 1 as qty
    from master.m_piece p
    where p.piece_id = (
      select k.piece_id
      from master.m_piece k
      where k.kanji in ('王', '玉')
      order by case when k.kanji = '王' then 0 else 1 end, k.piece_id
      limit 1
    )
  )
  insert into public.player_owned_pieces (player_id, piece_id, source, quantity)
  select p_user_id, spq.piece_id, 'initial', spq.qty
  from standard_piece_qty spq
  on conflict (player_id, piece_id) do update
  set
    source = 'initial',
    quantity = excluded.quantity;
end;
$$;

-- Backfill standard counts for all existing players.
do $$
declare
  r record;
begin
  for r in select id from public.players loop
    perform public.seed_initial_owned_pieces(r.id);
  end loop;
end
$$;

-- Remove duplicated initial king rows when both 王 and 玉 existed.
with selected_king as (
  select k.piece_id
  from master.m_piece k
  where k.kanji in ('王', '玉')
  order by case when k.kanji = '王' then 0 else 1 end, k.piece_id
  limit 1
)
delete from public.player_owned_pieces pop
using master.m_piece p
where pop.piece_id = p.piece_id
  and p.kanji in ('王', '玉')
  and pop.source = 'initial'
  and pop.piece_id <> (select piece_id from selected_king);

create or replace function public.handle_new_user()
returns trigger
language plpgsql
security definer
as $$
begin
  insert into public.players (id)
  values (new.id)
  on conflict (id) do nothing;

  perform public.seed_initial_owned_pieces(new.id);

  perform public.ensure_default_my_deck_for_user(new.id);

  return new;
end;
$$;

commit;
