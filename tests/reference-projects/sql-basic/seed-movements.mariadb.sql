-- 10 000 synthetic stock movements (MariaDB variant).
create table movement (
    id integer primary key,
    warehouse_id integer not null,
    sku varchar(32) not null,
    delta integer not null,
    moved_at datetime not null,
    constraint movement_warehouse_fk foreign key (warehouse_id) references warehouse (id),
    constraint movement_item_fk foreign key (sku) references item (sku)
);
set session max_recursive_iterations = 10001;
insert into movement (id, warehouse_id, sku, delta, moved_at)
with recursive seq (n) as (select 1 union all select n + 1 from seq where n < 10000)
select n, 1 + n % 2, case when n % 3 = 0 then 'B-2' else 'A-1' end,
       (n % 21) - 10, timestamp '2026-01-01 00:00:00' + interval (n * 7) minute
from seq;
