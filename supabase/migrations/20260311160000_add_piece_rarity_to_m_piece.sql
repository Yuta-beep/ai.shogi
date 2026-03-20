-- Add rarity classification to piece master.

begin;

alter table master.m_piece
  add column if not exists rarity text not null default 'N';

do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_piece_rarity_chk'
      and conrelid = 'master.m_piece'::regclass
  ) then
    alter table master.m_piece
      add constraint m_piece_rarity_chk
      check (rarity in ('N', 'R', 'SR', 'UR', 'SSR'));
  end if;
end
$$;

create index if not exists m_piece_rarity_idx
  on master.m_piece(rarity);

commit;
