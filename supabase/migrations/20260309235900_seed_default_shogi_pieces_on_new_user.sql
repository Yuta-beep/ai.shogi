-- Grant default standard shogi pieces to each user.
-- 1) Backfill existing users
-- 2) Extend new-user trigger to auto-seed on auth signup

-- Backfill for users already created before this migration.
insert into public.player_owned_pieces (player_id, piece_id, source)
select
  pl.id,
  p.piece_id,
  'initial'
from public.players pl
cross join master.m_piece p
where p.kanji in ('歩', '香', '桂', '銀', '金', '角', '飛', '王', '玉')
on conflict (player_id, piece_id) do nothing;

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
  select
    new.id,
    p.piece_id,
    'initial'
  from master.m_piece p
  where p.kanji in ('歩', '香', '桂', '銀', '金', '角', '飛', '王', '玉')
  on conflict (player_id, piece_id) do nothing;

  return new;
end;
$$;
