insert into warehouse (id, city) values (1, 'Leipzig'), (2, 'Köln');
insert into item (sku, name, unit_price_cents) values
    ('A-1', 'Bolt; zinc', 25),      -- semicolon inside a literal
    ('B-2', 'Nut ''M6''', 10);      /* doubled quote */
insert into stock (warehouse_id, sku, qty) values (1, 'A-1', 100), (2, 'B-2', 40);
