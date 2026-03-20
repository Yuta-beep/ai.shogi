begin;

-- default_deck_template_placements のY軸を修正
-- 正: row=2 王将段 / row=1 角飛段 / row=0 歩段
truncate table public.default_deck_template_placements;

insert into public.default_deck_template_placements (row_no, col_no, piece_id)
select v.row_no, v.col_no, p.piece_id
from (
  values
    (2,0,'香'),(2,1,'桂'),(2,2,'銀'),(2,3,'金'),(2,4,'王'),(2,5,'金'),(2,6,'銀'),(2,7,'桂'),(2,8,'香'),
    (1,1,'角'),(1,7,'飛'),
    (0,0,'歩'),(0,1,'歩'),(0,2,'歩'),(0,3,'歩'),(0,4,'歩'),(0,5,'歩'),(0,6,'歩'),(0,7,'歩'),(0,8,'歩')
) as v(row_no, col_no, kanji)
join master.m_piece p on p.kanji = v.kanji;

-- 王が存在しないデータセット向けフォールバック
insert into public.default_deck_template_placements (row_no, col_no, piece_id)
select 2, 4, p.piece_id
from master.m_piece p
where p.kanji = '玉'
  and not exists (select 1 from master.m_piece k where k.kanji = '王')
on conflict (row_no, col_no) do update
set piece_id = excluded.piece_id;

commit;
