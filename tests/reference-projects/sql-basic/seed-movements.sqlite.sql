-- 10 000 synthetic stock movements (SQLite variant).
create table movement (
    id integer primary key,
    warehouse_id integer not null references warehouse (id),
    sku text not null references item (sku),
    delta integer not null,
    moved_at text not null
);
insert into movement (id, warehouse_id, sku, delta, moved_at)
with recursive seq (n) as (select 1 union all select n + 1 from seq where n < 10000)
select n, 1 + n % 2, case when n % 3 = 0 then 'B-2' else 'A-1' end,
       (n % 21) - 10, datetime('2026-01-01', '+' || (n * 7) || ' minutes')
from seq;
