begin;

-- Add player progression fields.
alter table public.players
  add column if not exists player_rank integer not null default 1,
  add column if not exists player_exp bigint not null default 0;

-- Ensure valid bounds for progression fields.
do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'players_player_rank_chk'
      and conrelid = 'public.players'::regclass
  ) then
    alter table public.players
      add constraint players_player_rank_chk check (player_rank >= 1);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'players_player_exp_chk'
      and conrelid = 'public.players'::regclass
  ) then
    alter table public.players
      add constraint players_player_exp_chk check (player_exp >= 0);
  end if;
end
$$;

-- Backfill current rows and normalize game currency to 0 for now.
update public.players
set
  player_rank = 1,
  player_exp = 0,
  pawn_currency = 0,
  gold_currency = 0;

-- Keep defaults explicit for newly created users.
alter table public.players
  alter column pawn_currency set default 0,
  alter column gold_currency set default 0,
  alter column player_rank set default 1,
  alter column player_exp set default 0;

commit;
