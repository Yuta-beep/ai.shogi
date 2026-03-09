-- Add image asset reference columns for piece master.
-- This keeps DB independent from concrete storage URL generation.

begin;

alter table master.m_piece
  add column if not exists image_source text not null default 'supabase',
  add column if not exists image_bucket text,
  add column if not exists image_key text,
  add column if not exists image_version integer not null default 1;

-- Optional integrity checks (idempotent via catalog guard).
do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_piece_image_source_chk'
      and conrelid = 'master.m_piece'::regclass
  ) then
    alter table master.m_piece
      add constraint m_piece_image_source_chk
      check (image_source in ('supabase', 's3'));
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_piece_image_version_chk'
      and conrelid = 'master.m_piece'::regclass
  ) then
    alter table master.m_piece
      add constraint m_piece_image_version_chk
      check (image_version >= 1);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_piece_image_key_format_chk'
      and conrelid = 'master.m_piece'::regclass
  ) then
    alter table master.m_piece
      add constraint m_piece_image_key_format_chk
      check (image_key is null or btrim(image_key) <> '');
  end if;
end
$$;

create index if not exists m_piece_image_lookup_idx
  on master.m_piece(image_source, image_bucket, image_key);

commit;
