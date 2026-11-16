-- Catalogue sample for SQLite (SPEC-0013, SPEC-0089): one object per feature of the schema catalogue.
-- Built by build.sh; every name and value here is invented test data.
pragma foreign_keys = on;

-- Column affinities, NOT NULL, constant and expression defaults.
create table affinity (
    id integer primary key,
    whole int not null default 0,
    small smallint default -1,
    label text not null default 'none',
    short_name varchar(40) default 'it''s',
    amount numeric(10, 2) default 0.5,
    ratio real default 1e3,
    exact double precision,
    payload blob default x'00ff',
    flag boolean not null default true,
    born date default current_date,
    seen timestamp default current_timestamp,
    stamp integer default (strftime('%s', 'now')),
    code text default (lower(hex(randomblob(4)))),
    untyped,
    odd "my type"
);

-- Generated columns, virtual and stored.
create table generated (
    id integer primary key,
    net integer not null,
    tax integer not null default 19,
    gross integer generated always as (net + net * tax / 100) virtual,
    gross_kept integer as (net + net * tax / 100) stored,
    tag text generated always as ('n' || id) stored not null
);

-- Composite primary key, WITHOUT ROWID.
create table warehouse_slot (
    warehouse text not null,
    aisle integer not null,
    slot integer not null,
    note text,
    primary key (warehouse, aisle desc, slot)
) without rowid;

-- STRICT table.
create table strict_reading (
    id integer primary key,
    taken_at text not null,
    value real not null,
    raw blob,
    anything any
) strict;

-- AUTOINCREMENT.
create table ticket (
    id integer primary key autoincrement,
    title text not null
);

-- UNIQUE and CHECK constraints: column and table level, named and unnamed.
create table constrained (
    id integer primary key,
    email text not null unique,
    nick text constraint nick_unique unique collate nocase,
    age integer check (age >= 0),
    score integer constraint score_range check (score between 0 and 100),
    low integer,
    high integer,
    first_name text,
    last_name text,
    unique (first_name, last_name),
    constraint low_high_unique unique (low, high),
    check (low <= high),
    constraint names_given check (first_name is not null or last_name is not null)
);

-- Foreign keys: every action, DEFERRABLE, composite, self reference.
create table parent (
    id integer primary key,
    code text not null unique,
    region text not null,
    number integer not null,
    unique (region, number)
);

create table child_actions (
    id integer primary key,
    a_cascade integer references parent (id) on delete cascade on update cascade,
    a_set_null integer references parent (id) on delete set null on update set null,
    a_set_default integer default 1 references parent (id) on delete set default on update set default,
    a_restrict integer references parent (id) on delete restrict on update restrict,
    a_no_action integer references parent (id) on delete no action on update no action,
    a_plain integer references parent (id),
    a_deferred integer references parent (id) deferrable initially deferred,
    a_named integer constraint child_named_fk references parent (id) on delete cascade,
    by_code text references parent (code) on update cascade,
    implicit_pk integer references parent
);

create table child_composite (
    id integer primary key,
    region text not null,
    number integer not null,
    constraint child_region_number foreign key (region, number)
        references parent (region, number) on delete cascade on update restrict deferrable initially deferred
);

create table employee (
    id integer primary key,
    name text not null,
    boss_id integer references employee (id) on delete set null
);

-- Indexes: unique, partial, expression, descending, collation, composite.
create table indexed (
    id integer primary key,
    name text not null,
    email text,
    city text,
    created text,
    amount integer,
    deleted integer not null default 0
);
create index indexed_name on indexed (name);
create unique index indexed_email_unique on indexed (email);
create index indexed_active on indexed (city) where deleted = 0;
create index indexed_lower_name on indexed (lower(name));
create index indexed_created_desc on indexed (created desc, id asc);
create index indexed_city_nocase on indexed (city collate nocase);
create index indexed_mixed on indexed (city, amount * 2 desc, name collate nocase);

-- Views, also a view on a view, and one with a column list.
create view indexed_active_v as select id, name, city from indexed where deleted = 0;
create view indexed_city_count as select city, count(*) as people from indexed_active_v group by city;
create view named_columns (ident, title) as select id, title from ticket;

-- Triggers: BEFORE, AFTER, INSTEAD OF.
create table audit (
    id integer primary key autoincrement,
    what text not null,
    at text not null default current_timestamp
);
create trigger ticket_before_insert before insert on ticket
begin
    select raise(abort, 'title must not be empty') where trim(new.title) = '';
end;
create trigger ticket_after_update after update of title on ticket for each row when old.title <> new.title
begin
    insert into audit (what) values ('ticket ' || new.id || ' renamed');
end;
create trigger ticket_after_delete after delete on ticket
begin
    insert into audit (what) values ('ticket ' || old.id || ' deleted');
end;
create trigger named_columns_insert instead of insert on named_columns
begin
    insert into ticket (title) values (new.title);
end;

-- Virtual tables.
create virtual table doc_search using fts5 (title, body, tokenize = 'porter');
create virtual table area using rtree (id, min_x, max_x, min_y, max_y);

-- Identifiers that need quoting.
create table "order items" (
    "id" integer primary key,
    "select" text not null,
    "from" text default 'x',
    "group by" integer,
    "quo""te" text,
    "it's" text,
    "größe" real,
    "名前" text,
    "comma,name" text,
    "1st" integer,
    "MixedCase" text,
    "semi;colon" text
);
create table "select" (
    "table" integer primary key,
    "where" text
);
create table "wei""rd" (
    id integer primary key,
    "order id" integer references "order items" ("id") on delete cascade,
    "kw" integer references "select" ("table")
);
create table "Straße" (
    "nr" integer primary key,
    "länge" real
);
create table "顧客" (
    "番号" integer primary key,
    "名前" text not null
);
create index "idx order items select" on "order items" ("select", "group by" desc);
create index "idx comma" on "order items" ("comma,name", "quo""te");
create view "view with space" as select "id", "select" as "sel ect" from "order items";
create trigger "trg order items" after insert on "order items"
begin
    insert into audit (what) values ('order item ' || new."id");
end;

-- A few rows, so data views have something to show.
insert into parent (id, code, region, number) values (1, 'P1', 'north', 1), (2, 'P2', 'south', 7);
insert into child_actions (id, a_cascade, a_set_null, by_code) values (1, 1, 2, 'P1');
insert into child_composite (id, region, number) values (1, 'north', 1);
insert into employee (id, name, boss_id) values (1, 'Ada', null), (2, 'Ben', 1);
insert into generated (id, net) values (1, 100), (2, 250);
insert into warehouse_slot values ('W1', 1, 1, 'front'), ('W1', 2, 5, null);
insert into strict_reading (taken_at, value, raw, anything) values ('2026-01-01', 1.5, x'01', 'text'), ('2026-01-02', 2.0, null, 7);
insert into ticket (title) values ('first'), ('second');
insert into constrained (email, nick, age, score, low, high, first_name, last_name) values ('a@example.test', 'Ann', 30, 50, 1, 2, 'Ann', 'Lee');
insert into indexed (name, email, city, created, amount) values ('Ann', 'a@example.test', 'Kiel', '2026-01-01', 5), ('Bob', 'b@example.test', 'kiel', '2026-01-02', 9);
insert into "order items" ("select", "group by", "quo""te", "it's", "größe", "名前", "comma,name") values ('s', 1, 'q', 'i', 1.5, 'なまえ', 'c');
insert into "顧客" ("名前") values ('山田');
insert into doc_search (title, body) values ('Readme', 'running runs ran');
insert into area values (1, 0, 1, 0, 1);
