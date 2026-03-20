begin;

create table if not exists public.player_stage_clears (
  player_id uuid not null references public.players(id) on delete cascade,
  stage_id bigint not null references master.m_stage(stage_id) on delete cascade,
  cleared_at timestamptz not null default now(),
  clear_count integer not null default 1,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (player_id, stage_id),
  constraint player_stage_clears_clear_count_chk check (clear_count >= 1)
);

create index if not exists player_stage_clears_stage_idx
  on public.player_stage_clears(stage_id, cleared_at desc);

alter table public.player_stage_clears enable row level security;

do $$
begin
  if not exists (
    select 1
    from pg_policies
    where schemaname = 'public'
      and tablename = 'player_stage_clears'
      and policyname = 'player_stage_clears: self read'
  ) then
    create policy "player_stage_clears: self read"
      on public.player_stage_clears for select
      using (auth.uid() = player_id);
  end if;

  if not exists (
    select 1
    from pg_policies
    where schemaname = 'public'
      and tablename = 'player_stage_clears'
      and policyname = 'player_stage_clears: self insert'
  ) then
    create policy "player_stage_clears: self insert"
      on public.player_stage_clears for insert
      with check (auth.uid() = player_id);
  end if;

  if not exists (
    select 1
    from pg_policies
    where schemaname = 'public'
      and tablename = 'player_stage_clears'
      and policyname = 'player_stage_clears: self update'
  ) then
    create policy "player_stage_clears: self update"
      on public.player_stage_clears for update
      using (auth.uid() = player_id)
      with check (auth.uid() = player_id);
  end if;
end
$$;

commit;
