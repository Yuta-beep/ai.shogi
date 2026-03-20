begin;

update master.m_reward
set
  reward_name = regexp_replace(reward_name, '^駒:\s*', ''),
  updated_at = now()
where reward_type = 'piece'
  and reward_code like 'piece_%'
  and reward_name ~ '^駒:\s*';

commit;
