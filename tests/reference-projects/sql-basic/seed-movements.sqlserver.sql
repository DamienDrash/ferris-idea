-- 10 000 synthetic stock movements (SQL Server variant).
create table movement (
    id integer primary key,
    warehouse_id integer not null references warehouse (id),
    sku nvarchar(32) not null references item (sku),
    delta integer not null,
    moved_at datetime2 not null
);
with seq (n) as (select 1 union all select n + 1 from seq where n < 10000)
insert into movement (id, warehouse_id, sku, delta, moved_at)
select n, 1 + n % 2, case when n % 3 = 0 then 'B-2' else 'A-1' end,
       (n % 21) - 10, dateadd(minute, n * 7, cast('2026-01-01T00:00:00' as datetime2))
from seq
option (maxrecursion 10000);
