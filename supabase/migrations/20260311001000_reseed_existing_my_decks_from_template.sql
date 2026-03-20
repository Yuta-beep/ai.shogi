begin;

-- 既存ユーザーのマイデッキを最新テンプレート配置で再生成する
-- 注意: 既存の「マイデッキ」配置は上書きされる
 do $$
declare
  r record;
begin
  for r in
    select p.id as player_id
    from public.players p
  loop
    perform public.ensure_default_my_deck_for_user(r.player_id);
  end loop;
end
$$;

commit;
