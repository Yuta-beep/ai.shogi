-- m_move_pattern_vector に capture_mode 列を追加する。
-- 'leap_over_one': 砲（cannon）型。その方向への通常移動は可能だが、
--   捕獲する場合は間に駒が1つ必要（その駒を跨いで取る）。
-- NULL: 通常の挙動（変更なし）。

alter table master.m_move_pattern_vector
  add column if not exists capture_mode text
    check (capture_mode in ('leap_over_one'));

-- cannon 移動パターンの全ベクトルを leap_over_one に設定する。
update master.m_move_pattern_vector as v
set    capture_mode = 'leap_over_one'
from   master.m_move_pattern mp
where  mp.move_pattern_id = v.move_pattern_id
  and  mp.move_code = 'cannon';
