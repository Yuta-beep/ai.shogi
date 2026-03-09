-- Players table for game user data.
-- id is a FK to auth.users (issued by Supabase anonymous sign-in).

create table if not exists public.players (
  id              uuid        primary key references auth.users(id) on delete cascade,
  display_name    text        not null default '将棋プレイヤー',
  rating          int         not null default 1500,
  pawn_currency   int         not null default 0,
  gold_currency   int         not null default 0,
  is_anonymous    boolean     not null default true,
  created_at      timestamptz not null default now(),
  updated_at      timestamptz not null default now()
);

-- RLS: players can only read/write their own row.
alter table public.players enable row level security;

create policy "players: self read"
  on public.players for select
  using (auth.uid() = id);

create policy "players: self update"
  on public.players for update
  using (auth.uid() = id);

-- Auto-create a players row when a new auth.users row is inserted.
create or replace function public.handle_new_user()
returns trigger language plpgsql security definer as $$
begin
  insert into public.players (id)
  values (new.id);
  return new;
end;
$$;

create trigger on_auth_user_created
  after insert on auth.users
  for each row execute procedure public.handle_new_user();
