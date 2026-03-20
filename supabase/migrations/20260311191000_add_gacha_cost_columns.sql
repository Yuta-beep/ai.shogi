begin;

alter table master.m_gacha
  add column if not exists pawn_cost integer not null default 0,
  add column if not exists gold_cost integer not null default 0;

do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_gacha_pawn_cost_chk'
      and conrelid = 'master.m_gacha'::regclass
  ) then
    alter table master.m_gacha
      add constraint m_gacha_pawn_cost_chk check (pawn_cost >= 0);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_gacha_gold_cost_chk'
      and conrelid = 'master.m_gacha'::regclass
  ) then
    alter table master.m_gacha
      add constraint m_gacha_gold_cost_chk check (gold_cost >= 0);
  end if;

end
$$;

update master.m_gacha
set
  pawn_cost = case
    when gacha_code = 'kanken1' then 0
    else 30
  end,
  gold_cost = case
    when gacha_code = 'kanken1' then 1
    else 0
  end
where gacha_code in ('hihen', 'ukanmuri', 'shinnyo', 'kanken1');

do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'm_gacha_cost_nonzero_chk'
      and conrelid = 'master.m_gacha'::regclass
  ) then
    alter table master.m_gacha
      add constraint m_gacha_cost_nonzero_chk check (pawn_cost + gold_cost > 0);
  end if;
end
$$;

commit;
