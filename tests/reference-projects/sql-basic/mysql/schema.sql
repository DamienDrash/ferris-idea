-- MySQL variant of ../schema.sql (MySQL needs bounded key lengths). Foreign keys are table
-- constraints: MySQL accepts the column form `references t (c)` of ../schema.sql but does not
-- create a key from it, so join suggestions would have nothing to work with (OBS-0026 L38).
create table warehouse (
    id integer primary key,
    city varchar(100) not null
);

create table item (
    sku varchar(32) primary key,
    name varchar(200) not null,
    unit_price_cents integer not null check (unit_price_cents >= 0)
);

create table stock (
    warehouse_id integer not null,
    sku varchar(32) not null,
    qty integer not null default 0,
    primary key (warehouse_id, sku),
    constraint stock_warehouse_fk foreign key (warehouse_id) references warehouse (id),
    constraint stock_item_fk foreign key (sku) references item (sku)
);

create view stock_value as
select s.warehouse_id, sum(s.qty * i.unit_price_cents) as value_cents
from stock s join item i on i.sku = s.sku
group by s.warehouse_id;
