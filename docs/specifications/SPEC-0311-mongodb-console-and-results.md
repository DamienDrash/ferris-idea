# SPEC-0311: MongoDB console, completion, result views and editing

- **Version:** 2 · **Date:** 2026-11-05 (v2: pages are read with skip/limit)
- **Sources:** GEN (MongoDB tools offer a shell-like console with `db.collection.find(...)`, show
  documents as a tree, a table or JSON, and edit one document at a time), STD (MongoDB Extended JSON
  v2 relaxed and canonical forms; JSON RFC 8259), DOC (MongoDB manual: CRUD methods, query and update
  operators, aggregation stages, `listCollections`, `listIndexes`, `estimatedDocumentCount` — read,
  not quoted), ADR-0022, SPEC-0012, SPEC-0021, SPEC-0310
- **Feature matrix rows:** F-053, F-054, F-055, F-060

## Purpose
The user types MongoDB queries in a console, gets completion for collections, methods, operators and
field names, sees the documents as a tree, grid or JSON, edits them safely and exports them.

## Behaviour
1. **Console language.** One command per run (the command at the caret; a selection is one command).
   Accepted forms, with `db` the console's current database:
   `show dbs` · `show databases` · `show collections` · `use <db>` ·
   `db.<coll>.find(<filter>?, <projection>?)` with chained `.sort(<spec>)`, `.limit(n)`, `.skip(n)` ·
   `db.<coll>.findOne(<filter>?, <projection>?)` · `db.<coll>.countDocuments(<filter>?)` ·
   `db.<coll>.estimatedDocumentCount()` · `db.<coll>.distinct("<field>", <filter>?)` ·
   `db.<coll>.aggregate([<stage>, …])` · `db.<coll>.insertOne(<doc>)` · `insertMany([<doc>, …])` ·
   `updateOne(<filter>, <update>)` · `updateMany(…)` · `replaceOne(<filter>, <doc>)` ·
   `deleteOne(<filter>)` · `deleteMany(<filter>)` · `createIndex(<keys>, <options>?)` ·
   `dropIndex("<name>")` · `drop()` · `db.getCollection("<name>").<method>(…)` for names that are not
   identifiers · `db.createCollection("<name>")` · `db.runCommand(<doc>)`. A trailing `;` is allowed.
   [GEN, DOC]
2. **Arguments** are relaxed JSON: unquoted keys (`$gt`, `a.b`), single- or double-quoted strings,
   trailing commas, `//` and `/* */` comments, `true/false/null`, integers (32-bit when they fit,
   else 64-bit), doubles, and the shell constructors `ObjectId("…")`, `ISODate("…")`,
   `new Date("…")`, `NumberLong(n)`, `NumberInt(n)`, `NumberDecimal("…")`, `UUID("…")`,
   `/regex/flags`; canonical Extended JSON (`{"$oid": …}`, `{"$date": …}`) is accepted too. [STD]
3. **Not a script engine.** Variables, functions, loops, `printjson`, `load`, `eval` and any other
   method are refused with an error at its position that lists the accepted forms; nothing is sent.
   [ADR-0022]
4. **Errors** carry a character offset into the console text: parse errors point at the offending
   character; server errors carry the server's code and message. [SPEC-0007]
5. **Results.** `find` and `aggregate` return pages of 50 documents (the console's page size,
   1–1000); page *n* is read again with skip/limit (`aggregate`: `$skip`/`$limit` stages appended,
   not for `$out`/`$merge`), one document more than the page tells whether more exist.
   `findOne` gives one document or "no document"; counts and `distinct` give a value; writes give
   matched/modified/inserted/deleted counts and inserted ids; `runCommand` gives its reply document.
   [GEN]
6. **Three views of one result**, switched without re-running:
   - **Tree:** one node per document (`_id` as its label), fields as children, arrays with indexes;
     each value shows its BSON type (string, int32, int64, double, decimal128, bool, date,
     objectId, null, array, object, binary, regex, timestamp, other).
   - **Grid:** columns are the top-level fields of the page in first-seen order with `_id` first;
     missing fields are empty, nested values are compact relaxed Extended JSON.
   - **JSON:** relaxed Extended JSON, pretty printed, one document after another in an array.
   [GEN, STD]
7. **Editing** a document of a `find` result over a single collection (not `aggregate`): changing a
   top-level or nested value, adding or removing a field, or editing the whole document as JSON
   produces one `updateOne({_id: …}, {$set/$unset: …})` or `replaceOne({_id: …}, …)` that is shown
   before it runs. Documents without `_id` cannot be edited. A document changed by someone else since
   it was read (compared field by field on the edited paths) is not overwritten: the update filters on
   the old values and reports "changed since it was read" when it matches nothing. Deleting asks with
   the number of documents. [GEN, IMP-0291]
8. **Completion** in the console: after `db.` the collections of the current database and
   `getCollection`; after `db.<coll>.` the methods of behaviour 1; inside a filter or projection object
   the field paths sampled for that collection (SPEC-0310 behaviour 5) and, after `$`, query operators
   (`$eq $ne $gt $gte $lt $lte $in $nin $exists $type $regex $and $or $nor $not $elemMatch $size
   $all`); inside an update `$set $unset $inc $push $pull $addToSet $rename $mul $min $max
   $currentDate`; inside `aggregate([ {` the stages (`$match $project $group $sort $limit $skip
   $unwind $lookup $count $addFields $set $unset $facet $replaceRoot $sample $out $merge`). Candidates
   are filtered by the typed prefix, case-insensitive. [GEN, DOC]
9. **Classification for rights and read-only** (SPEC-0012): read-data — `find`, `findOne`, counts,
   `distinct`, `aggregate` without `$out`/`$merge`, `show`, `use`; read-schema — `show collections`,
   listing indexes; modify-data — inserts, updates, replaces, deletes, `aggregate` with `$out` or
   `$merge`; modify-schema — `createIndex`, `dropIndex`, `drop`, `createCollection`, and every
   `runCommand` except a short list of read commands (`ping`, `buildInfo`, `serverStatus`,
   `listCollections`, `listIndexes`, `dbStats`, `collStats`, `count`, `distinct`, `find`,
   `hello`, `isMaster`). `deleteMany({})` and `updateMany({}, …)` with an empty filter ask for
   confirmation naming the collection and the count. [SPEC-0012, IMP-0062]
10. **Export** of a `find`/`aggregate` result streams all pages, not only the loaded one
    (SPEC-0310 behaviour 7). [SPEC-0021]

## Acceptance criteria
- **AK-1** Parser: every form of behaviour 1 parses, chained cursor methods, `getCollection`,
  trailing `;`; relaxed JSON of behaviour 2 incl. constructors, comments and trailing commas; a
  variable, a loop and an unknown method are refused with offsets (unit tests `mongo_shell::tests`).
- **AK-2** Classification of behaviour 9 incl. `$out`, `runCommand` allow list, empty-filter
  confirmation (unit tests).
- **AK-3** Views: tree with types, grid column union with `_id` first, JSON relaxed (unit tests
  `view::tests`).
- **AK-4** Edit plans: `$set`/`$unset` from changed paths with the old values in the filter;
  replace; no `_id` refused (unit tests `edit::tests`).
- **AK-5** Completion contexts of behaviour 8 (unit tests `completion::tests`).
- **AK-6** Live (MongoDB container): insert, find with sort/limit and paging, aggregate, count,
  distinct, updateOne by edit plan, a concurrent change detected, deleteMany, createIndex, and
  read-only refusal (`crates/nosql/tests/live.rs`).

## Out of scope
JavaScript execution; transactions across commands; schema validation editing; GridFS; explain
plans for MongoDB (later version of this spec).

## Known deviations from the reference
The console is a defined subset of the shell instead of a script engine, so it is safe to hand to
agents and every refusal says what is accepted (IMP-0292). Edits never overwrite a document changed
since it was read (IMP-0291).
