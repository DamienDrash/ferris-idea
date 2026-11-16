-- 10 000 synthetic stock movements for paging, sorting and filtering in result grids.
create table movement (
    id integer primary key,
    warehouse_id integer not null references warehouse (id),
    sku text not null references item (sku),
    delta integer not null,
    moved_at timestamp not null
);
insert into movement (id, warehouse_id, sku, delta, moved_at)
select n, 1 + n % 2, case when n % 3 = 0 then 'B-2' else 'A-1' end,
       (n % 21) - 10, timestamp '2026-01-01' + n * interval '7 minutes'
from generate_series(1, 10000) as n;
