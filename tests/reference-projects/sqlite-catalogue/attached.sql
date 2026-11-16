-- Second database file, attached by the profile's startup script as "archive".
create table archived_ticket (
    id integer primary key,
    title text not null,
    closed_at text not null default current_timestamp
);
create table "archive notes" (
    id integer primary key,
    ticket_id integer references archived_ticket (id) on delete cascade,
    "note text" text
);
create index archived_ticket_title on archived_ticket (title);
create view archived_titles as select title from archived_ticket;
insert into archived_ticket (title) values ('old one');
