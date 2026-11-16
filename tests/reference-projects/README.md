# Reference projects

Own, synthetic mini-projects (one domain: a small inventory). Each exercises
typical IDE features: navigation, rename, extract, run, test, debug, SQL.

| Project | Language | Exercises |
|---|---|---|
| `python-basic` | Python 3 | classes, imports, a test, a deliberate rename target (`Item.qty`) |
| `sql-basic` | SQL (PostgreSQL / SQLite) | schema, foreign keys, view, script with comments and literals |
| `typescript-basic` | TypeScript | module imports, interface, generic function |
| `java-basic` | Java 21 | package, record, stream pipeline |
| `spring-basic` | Java 21, Spring Boot 3.5 | bean injection, REST endpoint, JDBC against the reference PostgreSQL |
| `react-basic` | TypeScript, React 19 | components, props, generic types (run `npm install` first) |
| `php-basic` | PHP 8.2+ | namespaces, readonly properties, variadics |
| `http-requests` | HTTP request file | requests against `spring-basic` |
| `flask-basic` | Python, Flask | web endpoint reading the reference SQLite database |
| `angular-basic` | TypeScript, Angular 20 | a standalone component with an external template, for the Angular language server (`js.sh install-angular` of the reference environment) |
| `sqlite-catalogue` | SQL (SQLite) | one object per feature of the SQLite catalogue (affinities, generated columns, constraints, every foreign key action, index kinds, views, triggers, FTS5, R-Tree, names that need quoting) and files that cannot be opened; `sqlite-catalogue/build.sh <new folder>` builds the databases and an IDECraft project around them |

Reference databases for `sql-basic`: `db.sh` of the reference environment (PostgreSQL, MySQL, SQLite;
MySQL uses `sql-basic/mysql/schema.sql`; `seed-movements.*.sql` adds 10 000 rows).
