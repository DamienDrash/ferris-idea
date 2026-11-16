-- Synthetic inventory schema; runs on PostgreSQL and SQLite.
create table warehouse (
    id integer primary key,
    city text not null
);

create table item (
    sku text primary key,
    name text not null,
    unit_price_cents integer not null check (unit_price_cents >= 0)
);

create table stock (
    warehouse_id integer not null references warehouse (id),
    sku text not null references item (sku),
    qty integer not null default 0,
    primary key (warehouse_id, sku)
);

create view stock_value as
select s.warehouse_id, sum(s.qty * i.unit_price_cents) as value_cents
from stock s join item i on i.sku = s.sku
group by s.warehouse_id;
