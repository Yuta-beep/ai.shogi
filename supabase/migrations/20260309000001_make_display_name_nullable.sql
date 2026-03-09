-- Allow display_name to be null so that new players are prompted to set a username.

alter table public.players
  alter column display_name drop default,
  alter column display_name drop not null;

-- Re-create trigger function without setting display_name (leave it null on signup).
create or replace function public.handle_new_user()
returns trigger language plpgsql security definer as $$
begin
  insert into public.players (id)
  values (new.id);
  return new;
end;
$$;
