# SPEC-0102: HTTP client environments

- **Version:** 6 · **Date:** 2026-11-16 (v6 after OBS-0061 H1: the list of environments
  opens inside the window, behaviour 3, AK-10 new); 2026-11-16 (v5 after OBS-0061 W2: with several environments and none
  chosen nothing that needs one is sent, the choice is a list and holds per request file, agents
  the same; behaviours 2–4 changed, AK-1 and AK-7 changed, AK-8 and AK-9 new; v4: AK-7, the window test closes AK-3 without a screen; v3: private values masked for agents without the
  `secret-read` right, behaviour 5 — security note from PR 667; v2: where private values may appear, behaviour 5; dynamic
  variables moved to SPEC-0046 v3)
- **Sources:** GEN (request files take variables from named environments in `http-client.env.json`,
  secrets from `http-client.private.env.json` beside it, a `$shared` environment for all; the
  environment is chosen before running; F-079), STD (JSON), SPEC-0046 (request files),
  SPEC-0096 (running requests in the window); v5: OBS-0061 W2 (the reference does not send without
  a chosen environment and offers the choice), decision of the coordinator 2026-11-16 (security
  first)
- **Feature matrix rows:** F-079

## Purpose
Run the same requests against local, test and production servers without editing the request file,
and keep tokens out of it.

## Behaviour
1. **Files:** for a request file, the nearest folder from its own up to the workspace root that has
   `http-client.env.json` or `http-client.private.env.json` provides the environments: each top-level
   key is an environment of variables; strings, numbers and booleans become values, other values
   (nested settings) are skipped; `$shared` variables apply to every environment unless it defines
   them; the private file's values win over the shared file's. A file that cannot be read is
   reported and skipped. [GEN, STD]
2. **Running (v5):** ⌘↩, a run mark and Run All Requests in File (SPEC-0096) use
   - the environment chosen for this request file, while the files still define it;
   - else the only environment, when the files define exactly one;
   - else none. When the files define several and none is chosen, a request that uses a variable
     of an environment is not sent: a name that neither the request file nor a dynamic value
     provides and that an environment or `$shared` defines. The status bar and Output (in red) say
     "Not sent: choose an HTTP environment: local, staging" with the names, and the Services
     window shows "Env: none ▾", from which the list of behaviour 3 opens. A request that uses no
     variable of an environment goes without one. Run All asks once for the file: when any of its
     requests needs an environment, none is sent.

   Without environment files nothing changes. Variables defined in the request file
   (`@name = value`) still win. Until v4 the first environment by name was taken without a
   choice, so a request with a private token could go to an environment nobody chose, perhaps a
   real one. [GEN, OBS-0061 W2, security first]
3. **Choosing (v5):** the Services window shows "Env: name ▾" (or "Env: none ▾") once a request
   file with environments has run or asked for one. A click opens a list of the environments, the
   one in use marked; a click on an entry chooses it for this request file and closes the list,
   and the status bar names it. The choice holds for that file, also after other files were run,
   until the window closes or another is chosen; other request files keep their own. Choosing sends
   nothing. Until v4 a click switched to the next environment. [GEN, OBS-0061 W2]
   **v6 (OBS-0061 H1):** the list opens under the button and lies wholly inside the window: when
   it would reach past the window's left or right edge it is moved in (SPEC-0190 v6 behaviour 16).
   Its title "HTTP environment" and each entry take one line each, with no empty line between
   them. In v5 the list hung from the button's right edge, so with the button near the window's left
   edge it reached past it and the first letters were cut off. [GEN]
4. Agents pass `environment` to `http_requests` and `send_http_request`; variables given in `env` win
   over the environment's; an unknown name is refused with the names the files define. **v5:**
   without `environment` the rule of behaviour 2 holds, with `env` counted as provided: the only
   environment is used; with several, `send_http_request` (also its dry run) for a request that
   uses a variable of an environment, and `http_requests` for a file in which any request does,
   are refused with "several HTTP environments and none given: pass `environment` with one of
   dev, prod"; requests that use none are resolved without one. The agent log masks the private
   values of the environment used, also when it was not named. [ADR-0008, security first]
5. **Private values:** Ferris IDEA remembers which variables of an environment come from the private
   file. Their values go to the server the request names and into the resolved request shown to the
   one who runs it; Ferris IDEA writes them to no file: the agent log (SPEC-0053 behaviour 5) shows
   `{{name}}` in their place. The private file itself is never a request body (SPEC-0046 behaviour
   11). **v3 – Private values for agents:** what `http_requests` returns and what the dry run of
   `send_http_request` shows keep every value that comes from `http-client.private.env.json` as
   `{{name}}`, unless the agent has the `secret-read` right (SPEC-0053 v6 behaviour 10). A request
   with such a value lists the names in `masked`; nothing is reported as unresolved for it. Values the
   agent passes in `env` are its own and are shown. Sending (`dry_run=false`) uses the real values
   and needs only `http`. Each masking is logged in the agent log with the names, never the values.
   The private file itself is refused to agents without the right (SPEC-0053 v6 behaviour 10,
   SPEC-0156 v5 behaviour 8). [security first, GEN, IMP-0007]

## Acceptance criteria
- **AK-1** `$shared`, the private file winning, numbers as text, nested values skipped, the nearest
  folder, a broken file reported (unit test `env::tests::shared_private_and_nearest_folder`); choosing
  (v5: AK-8).
- **AK-2** MCP: a named environment with a private token, `env` overriding it, an unknown name refused
  with the defined names (`crates/mcp/tests/it/protocol.rs`: `http_requests_tool`).
- **AK-3** In a real window (`ferris-idea ui --selftest`): a request whose host comes from
  `http-client.env.json` reaches the local server and the Services window shows the environment.
- **AK-4** By hand: switching between two environments next to the reference.
- **AK-5** The variables of the private file are known by name (unit test in `env::tests`); a request
  with a private value in its URL is logged with `{{name}}` in its place
  (`crates/mcp/tests/it/protocol.rs`).
- **AK-6 (v3, behaviour 5)** Which values are private (unit test
  `env::tests::shared_private_and_nearest_folder`); over MCP without the right `{{token}}` and `masked`
  in `http_requests` and the dry run, the value in neither nor in the agent log; with `secret-read`
  the value (`protocol.rs`: `http_requests_tool`,
  `secret_files_and_private_http_values_need_the_secret_read_right`).

- **AK-7 (v4, v5)** In a window of the test harness against a local server
  (`crates/workbench/tests/it/http_client_window.rs`: `requests_go_from_keys_marks_and_run_all`): the
  host and path of the request come from `http-client.env.json` with two environments; the first ⌘↩
  sends nothing (the server sees no request) and Output names both environments; a click on
  "Env: none ▾" (found through the semantic tree) opens the list, a click on `local` chooses it, and
  the next ⌘↩ goes with `local`'s values; the list then chooses `staging` and the next request goes
  with `staging`'s values. Closes AK-3 without a screen.
- **AK-8 (v5, behaviour 2)** Unit test `env::tests::the_environment_is_chosen_or_asked_for`: a
  chosen environment wins while it exists, a choice that is gone counts as none, the only
  environment is used, several with a needed variable (also one of `$shared`) ask with all names,
  several without one and no environment files run without; the window keeps a choice per request
  file (`http_client_window.rs`: a second request file with the same environments asks again).
- **AK-9 (v5, behaviour 4)** MCP (`crates/mcp/tests/it/protocol.rs`:
  `http_tools_ask_for_one_of_several_environments`): with two environments and no `environment`,
  `send_http_request` (dry run and sending) and `http_requests` are refused naming both and nothing
  reaches the local server; a request that needs no environment is sent; with one environment it is
  used without being named, and the agent log shows its private value as `{{name}}`.
- **AK-10 (v6, behaviour 3)** In a window of the test harness
  (`crates/workbench/tests/it/http_client_h1_h3.rs`: `the_environment_list_stays_inside_the_window`):
  with the Services window at the left edge the list's title and entries lie inside the window, in
  order, one line apart.

## Out of scope
SSL and proxy settings from environment files, response handler scripts setting variables, hiding the
private file from agents that read workspace files, a "no environment" entry in the list (a
request that needs no environment goes without one by itself), keeping the choice after the window
closes, warning when the private file is not ignored by version control.

## Known deviations from the reference
The environment is chosen from a list in the Services window instead of a selector in the editor
toolbar. (v5) Without a choice Ferris IDEA says why nothing was sent and offers the list where the
answer is shown; it does not open a list on its own at the editor. Compared next to the reference in
OBS-0061 (W2).
