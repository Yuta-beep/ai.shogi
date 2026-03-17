-- Allow authenticated users to create their own players row when missing.
-- This complements signup trigger paths and enables safe client-side upsert by id.

do $$
begin
  if not exists (
    select 1
    from pg_policies
    where schemaname = 'public'
      and tablename = 'players'
      and policyname = 'players: self insert'
  ) then
    create policy "players: self insert"
      on public.players for insert
      with check (auth.uid() = id);
  end if;
end
$$;
