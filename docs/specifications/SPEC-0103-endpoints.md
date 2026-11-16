# SPEC-0103: Endpoints of web frameworks

- **Version:** 5 · **Date:** 2026-11-13 (v5: the rows can be searched — the Endpoints tab of
  Search Everywhere and `query` of the MCP tool, behaviour 8; v4: one row per mapping with all its methods, the slash
  at the end of a path, no method shown for a mapping without one, REST client interfaces as rows
  of their own kind, behaviour 7, after the comparison of OBS-0037, findings EP1 to EP4; v3: the
  forms of the mapping annotations the frameworks document, behaviour 6, found by running the tool
  on four sample projects; v2: only the project's
  own sources are scanned, decision of the Project Owner of 2026-11-12; v1: 2026-11-01)
- **Sources:** GEN (IDEs list the HTTP endpoints a project declares in an "Endpoints" tool window —
  method, path, handler — navigate to them and generate HTTP requests for them; F-040), DOC (the
  frameworks' own documentation of their annotations and decorators: Spring MVC `@RequestMapping`,
  `@GetMapping` … with `value`/`path`/`method`; JAX-RS `@Path`, `@GET` …; Micronaut `@Controller`,
  `@Get` …; NestJS `@Controller`, `@Get` …; FastAPI `APIRouter(prefix=…)`, `@router.get`; Flask
  `Blueprint(url_prefix=…)`, `@app.route(methods=[…])`; Express `app.get`/`router.post`; Spring Boot
  `server.port`; v3: Spring MVC arrays of paths and of request methods, Jakarta REST
  `@ApplicationPath`, Micronaut `uri`/`uris` and `@Head`/`@Options`/`@Trace`, the client interfaces of
  MicroProfile REST Client, Micronaut and Spring Cloud OpenFeign, Quarkus `quarkus.http.port`; v4:
  Spring Framework 6 no longer matches a path with a slash at its end to a mapping without, FastAPI
  and Flask treat the two as different routes, Micronaut's `@Client` takes an address, a service
  name or a path and has a `path` attribute, `@FeignClient` has `path`), OBS (OBS-0037: the rows of
  the reference's list for the Spring, Jakarta EE and Quarkus sample projects, findings EP1 to
  EP4), SPEC-0096 (requests from `.http` files). Clean room: no internals of the reference.
- **Feature matrix rows:** F-040, F-036

## Purpose
See which HTTP routes a service offers, jump to their code and try them, without reading every
controller.

## Behaviour
1. **Finding:** the project files (not under `src/test/` or `node_modules/`, at most 1 MiB) are scanned
   line by line: Java and Kotlin (Spring, JAX-RS, Micronaut), TypeScript with `@nestjs` (NestJS),
   Python (FastAPI, Flask) and JavaScript/TypeScript using `express`. Class-level mappings and router
   prefixes of the same file are joined with the method's path; commented lines are skipped. The handler is
   `Class.method` or the function name. Sorted by path and methods. A mapping without a method and
   one with several: behaviour 7 (until v3 `ANY`, and one endpoint per method). [DOC]
2. **Window:** the Endpoints tool window (`ActivateEndpointsToolWindow`, Search Everywhere; bottom)
   lists methods, path, handler, framework and `file:line`; it scans when opened and with the refresh
   button. ↑/↓ select, Enter or a double click opens the declaration, Esc hides. [GEN]
3. **Generate Request** adds `### handler` and `METHOD http://localhost:port/path` (the first method
   of the row, GET for a row without a method; nothing for a client row, behaviour 7) to
   `.idecraft/generated-requests.http` and opens it on the new request, ready for ⌘↩. The port is
   `server.port` (or `micronaut.server.port`) from `src/main/resources/application.properties`/`.yml`,
   else the framework's default: 8080 (Spring, JAX-RS, Micronaut), 8000 (FastAPI), 5000 (Flask), 3000
   (Express, NestJS). The generated file and the test runners' report files are listed in the
   generated `.idecraft/.gitignore`. [GEN]
4. Agents get the same list with a ready request per endpoint from `endpoints`; its fields:
   behaviour 7. [ADR-0008]
5. **Only the project's own sources (v2; decision of the Project Owner of 2026-11-12: Endpoints
   follows the rule Rename, Find in Files and Search Everywhere got):** the files scanned are those
   that are the project's own by the rule of SPEC-0148 v4 behaviour 7 (`OwnSources`) — not virtual
   environments (the routes of a framework's own examples in `.venv`), installed dependencies, build
   output and caches, version control and IDE data, folders the project model excludes, anything
   behind a symbolic link, anything the ignore rules of Git match unless Git tracks it. The rule of
   behaviour 1 on `src/test/` and `node_modules/` holds in addition. The window and `endpoints` use
   the same list. [GEN; decision]

6. **Forms of the mapping annotations (v3):** the scanner read one annotation per line with one
   path and one method. On the four sample projects of F-040 that gave wrong rows: a path taken
   from `produces` or `params`, a mapping over several lines read as a method named after its
   attribute, the prefix of a Spring `@Controller` class lost, the second path and the second
   method of an array missing, and the interfaces of HTTP clients listed as routes of the
   application. The forms the frameworks document are read as follows. [DOC]
   - **Where an annotation ends:** an annotation whose arguments run over several lines is read as
     one, at the line it starts on; several annotations on one line, and annotations on the line of
     the declaration (`@GetMapping("/x") public String x()`), are each read. The endpoint's line is
     the line of the annotation that gives the HTTP method.
   - **Paths:** the attribute `path` or `value` (Spring), `value`, `uri` or `uris` (Micronaut), or
     the value written without a name. An array (`{"/a", "/b"}`, in Kotlin `["/a", "/b"]`) gives
     one endpoint per path. Every other attribute (`produces`, `consumes`, `params`, `headers`,
     `name`) is never a path; a mapping without a path maps the class path itself.
   - **Methods:** `@RequestMapping(method = …)` takes one `RequestMethod`, an array of them or
     statically imported names (v3: one endpoint per method, `ANY` without the attribute; since v4
     one row, behaviour 7).
     Micronaut's `@Head`, `@Options` and `@Trace` are read like `@Get`.
   - **Class level:** the paths of the class-level mapping are prefixes; with several, every
     endpoint of the class is listed under each. Spring's stereotype `@Controller` carries a bean
     name and no path, so in a file that imports Spring the prefix comes from `@RequestMapping`
     alone.
   - **Application path:** when the project declares exactly one `@ApplicationPath("…")` (Jakarta
     REST), its value leads the path of every JAX-RS endpoint, as the running application serves
     them. With none or several the paths stay as the resources write them.
   - **Clients are no endpoints of the application:** the methods of an interface annotated
     `@RegisterRestClient` (MicroProfile REST Client), `@Client` (Micronaut) or `@FeignClient`
     (Spring Cloud OpenFeign) describe a service the application calls (v3: not listed; since v4
     listed as rows of kind client, behaviour 7).
   - **Port:** `quarkus.http.port` is read like `server.port` (behaviour 3).

7. **Rows as the reference lists them (v4):** next to the reference the paths of the Spring,
   Jakarta EE and Quarkus sample projects were the same; four things differed. [OBS-0037]
   - **One row per mapping and path (EP1):** a mapping with several HTTP methods
     (`@RequestMapping(method = {POST, PUT})`, Flask `methods=["GET", "POST"]`) is one row that
     names them all in the mapping's order, each once, joined with `|` (`POST|PUT`). The reference
     showed such a mapping in one row; IDECraft showed two. Several paths still give one row each
     (behaviour 6), as on both sides. Rows are sorted by path, then by their methods. [OBS-0037
     EP1]
   - **The slash at the end (EP2):** when the handler's own path ends in `/`, the shown path does
     too (`/api/products/` for `@PostMapping("/")` below `/api/products`) — for the frameworks
     that tell the two paths apart: Spring, FastAPI and Flask. A slash at the end of a class or
     router prefix alone does not show, and JAX-RS, Micronaut, NestJS and Express, which serve both
     spellings from one mapping, keep the path without. The generated request uses the shown
     path. [OBS-0037 EP2 for Spring; DOC for the others]
   - **No method (EP3):** a mapping that names no method (`@RequestMapping("/ping")`, Express
     `app.all`) takes every method. The window shows nothing in the method column, as the
     reference does, instead of `ANY`. [OBS-0037 EP3]
   - **Client interfaces (EP4):** the methods of an interface annotated `@RegisterRestClient`,
     Micronaut's `@Client` or `@FeignClient` are listed again, as rows of kind client: the
     reference lists the REST client of the Quarkus sample, and since v3 IDECraft left it out. A
     client row has the path the interface writes — its class-level `@Path`, the `path` of
     `@FeignClient` or of `@Client`, or a `@Client` value that starts with `/`; an address or a
     service name is no path — without the application path of behaviour 6, since another service
     serves it. The window marks the row with the word "client" after the handler; Enter opens the
     declaration as for every row. There is no request for it: the scan does not know the other
     service's address, so Generate Request does nothing on a client row. Spring's HTTP interface
     clients (`@HttpExchange`) are not read; the model does not know them. [OBS-0037 EP4; DOC]
   - **MCP:** `endpoints` returns one entry per row. `methods` is the list of the row's methods,
     empty for a mapping without a method. `method` stays as one text for readers of the earlier
     shape: the methods joined with `|`, and `ANY` for the empty list — an agent gets the fact
     in words where the window shows a blank. `kind` is `server` or `client`. `request` uses the
     first method (GET for the empty list) and is null for a client row. [ADR-0008; decision of
     this version]

8. **Search (v5):** the rows of this specification are also what the Endpoints tab of Search
   Everywhere searches (SPEC-0067 v9 behaviour 20: by path, by a leading HTTP method and by
   handler; Enter opens the declaration as in behaviour 2). The tab reads them with the same scan
   as the window (behaviours 1 and 5) each time the popup opens, so a framework or a form of
   mapping added here is found there without further work. `endpoints` takes an optional `query`
   with the matching and ranking of that tab and returns the matching rows, best first, each
   with the fields of behaviour 7; without `query`, or with an empty one, it returns every row
   in the order of behaviour 7 as before. [SPEC-0067; ADR-0008]

## Acceptance criteria
- **AK-1** Spring (class prefix, `value =`, `RequestMethod`, commented mapping), JAX-RS, Micronaut in
  Kotlin, FastAPI with router prefix, Flask with two methods, Express, NestJS; ports from configuration
  and defaults (unit tests `endpoints::tests`); the generated request (unit test
  `endpoints_view::tests::request_for_an_endpoint`).
- **AK-2** MCP: a Spring controller with a configured port (`crates/mcp/tests/it/protocol.rs`:
  `endpoints_tool`).
- **AK-3** In a real window (`idecraft ui --selftest`): the window lists `GET /ep/ping EpApi.ping` and
  `POST /ep EpApi.add`; Generate Request opens the request.
- **AK-4** By hand: a Spring Boot and a FastAPI project next to the reference.
- **AK-5 (v2)** In a project with a route in `.venv/lib/python3.12/site-packages/x.py`, in
  `node_modules/y.js`, in a file ignored by `.gitignore`, in `src/test/` and in an own file: only the
  own file's route is listed (unit test `endpoints::tests::only_the_projects_own_sources_are_scanned`;
  MCP `crates/mcp/tests/it/protocol.rs`: `endpoints_lists_the_projects_own_sources`).

- **AK-6 (v3; v4: methods in one row, clients as client rows)** The forms of behaviour 6: arrays
  of paths and methods, a mapping without a path beside `produces`/`params`, a mapping over several
  lines, annotation and method on one line, a Spring `@Controller` with two class paths, Micronaut
  `uri`, `uris` and `@Head`, the three kinds of client interfaces, the application path with one
  and with two applications, the Quarkus port
  (unit tests `endpoints::tests::{mapping_forms_of_spring, mapping_forms_of_jaxrs_and_micronaut,
  application_path_and_quarkus_port}`).
- **AK-7 (v3; counts of v4)** The four sample projects
  (`crates/languages/tests/fixtures/frameworks/`): the rows of `spring-shop` (17, test sources
  left out; `POST|PUT /api/products/bulk`, `POST /api/products/`, `/api/products/ping` without a
  method), `jakarta-library` (7, under `/api`), `quarkus-greeting` (3 and the REST client's row)
  and `micronaut-notes` (9 and the declarative client's row)
  (`crates/languages/tests/it/frameworks.rs`); MCP `endpoints` on `spring-shop`
  (`crates/mcp/tests/it/protocol.rs`: `frameworks_sample_project_spring`).
- **AK-8 (v4)** Behaviour 7: several methods in one row and each once, the slash kept for Spring
  and FastAPI and not for Micronaut and JAX-RS, a mapping without a method and Express `all`
  without one, the request of a row with several methods and of one without, a client row of each
  of the three kinds with its path prefix, without application path and without request, `kind`
  and `methods` as JSON (unit tests `endpoints::tests::{rows_methods_slashes_and_clients,
  mapping_forms_of_spring, mapping_forms_of_jaxrs_and_micronaut, application_path_and_quarkus_port}`,
  `endpoints_view::tests::request_for_an_endpoint`); MCP: `methods`, `method`, `kind` and
  `request` of a row with two methods, of one without a method and of a client row
  (`crates/mcp/tests/it/protocol.rs`: `endpoints_rows_methods_and_clients`).

- **AK-9 (v5)** The search over the rows as SPEC-0067 AK-10 (unit tests
  `endpoints::tests::search_*`); MCP: `query` with a path, with a method prefix and with a
  handler, and every row without it (`crates/mcp/tests/it/protocol.rs`: `endpoints_query`).

## Out of scope
Resolving constants in paths, prefixes from other files (`app.use('/api', router)`,
`include_router(prefix=…)`; the one prefix read from another file is `@ApplicationPath`, behaviour
6), context paths from the configuration (`server.servlet.context-path`,
`micronaut.server.context-path`, `quarkus.http.root-path`), mappings inherited from an interface or
a superclass, composed mapping annotations of the project's own, Spring WebFlux router functions,
OpenAPI documents, Spring bean navigation and configuration completion, Ktor routing DSL,
client-side calls in code, Spring's HTTP interface clients (`@HttpExchange`), the address of the
service a client interface calls and a request for it.

## Known deviations from the reference
A line scanner instead of a full framework model, so paths built from constants or other files are
shown as written. Compared side by side for the Spring, Jakarta EE and Quarkus sample projects
(OBS-0037): the paths agree, and behaviour 7 closes the four differences found. Not compared: the
Micronaut sample (the reference's list stayed empty), the row `PATCH /api/products/{id}/price`
(not read off in the reference), how the reference marks a client row and what it generates for
it, the slash at the end outside Spring, FastAPI and Flask projects next to the reference.
IDECraft does not group the rows by module.
