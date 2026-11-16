-- Synthetic inventory schema, SQL Server variant (T-SQL has no `text` keys).
create table warehouse (
    id integer primary key,
    city nvarchar(100) not null
);

create table item (
    sku nvarchar(32) primary key,
    name nvarchar(100) not null,
    unit_price_cents integer not null check (unit_price_cents >= 0)
);

create table stock (
    warehouse_id integer not null references warehouse (id),
    sku nvarchar(32) not null references item (sku),
    qty integer not null default 0,
    primary key (warehouse_id, sku)
);
GO

create view stock_value as
select s.warehouse_id, sum(s.qty * i.unit_price_cents) as value_cents
from stock s join item i on i.sku = s.sku
group by s.warehouse_id;
GO
