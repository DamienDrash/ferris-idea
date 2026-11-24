# SPEC-0134: Jakarta EE (CDI), Quarkus and Micronaut beans

- **Version:** 6 · **Date:** 2026-11-16 (v6: completion of keys in `@ConfigProperty` and
  `@Property` names, behaviour 8, headless pre-check of F-040, OBS-0073; v5 2026-11-14: configuration keys lead to their lines and back,
  resolved values and gutter marks for the three frameworks, behaviour 7, after OBS-0037 findings
  KS2 to KS4 for Spring; v4 2026-11-13: field injection in Micronaut projects and why not in
  Jakarta EE and Quarkus projects, behaviour 6, after OBS-0037 finding FW1; v3: producers,
  qualifiers, framework-provided beans and
  configuration forms the frameworks document, behaviour 5, found by running `spring_beans` on four
  sample projects; v2 2026-11-10: frameworks also from the build files, hand test Nr. 61 window
  run; v1 2026-11-02)
- **Sources:** GEN (IDEs with Jakarta EE, Quarkus and Micronaut support know the beans of such a
  project, show which bean an injection point gets, warn about unsatisfied and ambiguous
  dependencies and about configuration keys defined nowhere; F-040), DOC (Jakarta CDI, MicroProfile
  Config and Micronaut reference documentation, read for the rules, not quoted: scope annotations
  (`@ApplicationScoped`, `@RequestScoped`, `@SessionScoped`, `@ConversationScoped`, `@Dependent`,
  `@Singleton`) and EJB session beans (`@Stateless`, `@Stateful`) make a class a bean, `@Produces`
  methods declare beans, `@Named` names and qualifies, an `@Alternative` with `@Priority` is
  selected among several; `@ConfigProperty(name, defaultValue)`; Micronaut's `@Singleton`,
  `@Prototype`, `@Context`, `@Controller`, `@Factory` classes whose `@Bean`/`@Singleton`/`@Prototype`
  methods declare beans, `@Primary`, `@Property(name)`; v3: CDI producer fields, qualifier
  annotations declared with `@Qualifier` and the `@Default` qualifier, MicroProfile Config
  `Optional` values, MicroProfile REST Client `@RegisterRestClient`, Quarkus `@ConfigMapping`,
  `%profile.` keys, `@DefaultBean`, `@IfBuildProfile`/`@UnlessBuildProfile`; Micronaut
  `@ConfigurationProperties`, `@Client`, `@Requires`, `@Secondary`, repository interfaces of
  Micronaut Data, property names normalised to kebab case; Jakarta REST `@Produces` for media
  types; v4: the Micronaut guide's sections on constructor and field injection call the
  constructor the preferred and recommended form and field injection the lesser one; the Quarkus
  CDI guides and the Jakarta CDI specification present field injection as a usual form and advise
  only against private injected members; v5: MicroProfile Config and Quarkus configuration
  reference for `%profile.` keys and `quarkus.profile`, SmallRye Config for the prefix of
  `@ConfigMapping`, Micronaut configuration reference for environment-specific files
  `application-{environment}`, `micronaut.environments` and the prefix of
  `@ConfigurationProperties`), OBS (OBS-0037, run of 2026-11-14: findings KS2 to KS4, observed
  for the Spring sample only), STD (Java syntax, tree-sitter grammar), SPEC-0115 (Spring bean model), ADR-0008
- **Feature matrix rows:** F-040

## Purpose
Extend the bean model of SPEC-0115 to the other dependency injection frameworks of the matrix row,
so wiring and configuration mistakes in Jakarta EE, Quarkus and Micronaut projects show while
typing. Ferris IDEA's own model, built in a clean room from the rules above.

## Behaviour
1. **Frameworks:** the imports of the project's Java files name the frameworks: `io.quarkus.` →
   Quarkus, `io.micronaut.` → Micronaut, `org.springframework.` → Spring, `jakarta./javax.enterprise.`,
   `jakarta./javax.ejb.`, `jakarta.ws.rs.`, `org.eclipse.microprofile.` → Jakarta EE. They are listed
   most specific first; the first labels the findings ("Quarkus: …"), Spring when none is imported
   (SPEC-0115 unchanged). The tool window title names them all. [DOC]
   **Build files (v2):** a Quarkus application usually imports nothing from `io.quarkus.` — its
   code uses CDI, JAX-RS and MicroProfile, the same imports as a plain Jakarta EE application — so
   the imports alone cannot tell the two apart. The build files decide: `pom.xml`,
   `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts` and
   `gradle.properties` anywhere in the project that name the group `io.quarkus` (a dependency such
   as `io.quarkus:quarkus-rest`, the BOM `io.quarkus.platform:quarkus-bom`, also through a property
   such as `quarkus.platform.group-id`, the `quarkus-maven-plugin`, the Gradle plugin
   `io.quarkus`) mark Quarkus; `io.micronaut` marks Micronaut; `org.springframework` marks Spring.
   The name counts only as a whole group: not after a letter, digit, `_` or `.`, and the next character one of `.` `:` `"` `'` `<` `}`
   or a space, a line end or the end. A marked framework joins those from the imports, in the same
   order, so a Quarkus project's tool window reads "Quarkus · Jakarta EE" and its findings "Quarkus: …",
   while a project with only Jakarta EE imports and no such build entry (for example
   `jakarta.platform:jakarta.jakartaee-api`) stays "Jakarta EE". Before v2 the Quarkus project of
   the hand test, whose sources import only `jakarta.` and `org.eclipse.microprofile.`, was labelled
   "Jakarta EE". [DOC (Quarkus and Micronaut project layouts, read for the facts)]
2. **Beans and injection:** besides the Spring stereotypes, the CDI scopes, EJB session beans, JAX-RS
   resources (`@Path`) and the Micronaut scopes and factories make a class a bean; a class's
   `@Named("x")` names it, a path argument (`@Path("/a")`, `@Controller("/a")`) does not. Producer
   methods: `@Produces` in any bean class, `@Bean`/`@Singleton`/`@Prototype` in a factory or Spring
   configuration, named by `@Named` or the method; their parameters are injection points.
   `@Inject` fields, setters and constructors are injection points as in SPEC-0115; `@Named` is a
   qualifier. Among several beans, a single `@Primary` or `@Alternative` with `@Priority` wins. The
   ambiguity message names the framework's way out: "mark one @Alternative @Priority or add @Named"
   (Jakarta EE, Quarkus), "mark one @Primary or add @Named" (Micronaut). [DOC]
3. **Configuration:** `@ConfigProperty(name = "k")` and `@Property(name = "k")` keys are checked
   against `application*.properties/yml` like `@Value` keys; `defaultValue` counts as a default.
   Parameters and fields with these annotations are not bean lookups. Go to Bean on the key opens
   its definition. [DOC]
4. **MCP:** `spring_beans` also returns `framework` (the label) and `model.frameworks`. [ADR-0008]
5. **Forms the frameworks document (v3):** on the sample projects of F-040 every resource method
   with `@Produces(MediaType…)` was listed as a bean named after the method or the media type and
   its parameters as injection points, two beans told apart by a qualifier of the project's own
   were called ambiguous, and interfaces the framework implements had "no bean". The rules of
   SPEC-0115 behaviours 9 and 10 hold here too; in addition: [DOC]
   - **`@Produces`:** only the CDI annotation declares a bean: it is written without arguments.
     `@Produces(…)` with arguments, and `@Produces` in a file that imports the Jakarta REST or the
     Micronaut HTTP annotation of that name and not the CDI one, names media types. The
     parameters of a resource method are not injection points.
   - **Producer fields:** a field annotated with CDI's `@Produces` in a bean class declares a bean
     of the field's type, named by `@Named` or the field.
   - **Qualifiers of the project's own:** an annotation type the project declares with
     `@Qualifier` is a qualifier. An injection point with such qualifiers gets the beans that
     carry all of them. In Jakarta EE and Quarkus an injection point without any qualifier does
     not get a bean that carries one, since that bean is no `@Default` bean; in Micronaut and
     Spring it stays a candidate.
   - **Beans the framework implements:** interfaces annotated `@ConfigMapping` (Quarkus),
     `@RegisterRestClient` (MicroProfile REST Client), `@Client` (Micronaut), or with an
     annotation whose name ends in `Repository` (Micronaut Data); classes, records and interfaces
     annotated `@ConfigurationProperties` (Micronaut). The annotation's value (a prefix, an
     address) is not a bean name.
   - **Conditions:** `@Requires` (Micronaut), `@IfBuildProfile`, `@UnlessBuildProfile`,
     `@IfBuildProperty` and `@UnlessBuildProperty` (Quarkus) make a bean conditional; `@DefaultBean`
     (Quarkus) and `@Secondary` (Micronaut) give way when another bean fits.
   - **Configuration:** a `@ConfigProperty` whose type is an `Optional` needs no value; a key
     written `%profile.key` in a configuration file defines `key`; `@ConfigProperty` keys are
     compared exactly, Micronaut's `@Value` and `@Property` keys with the relaxed binding of
     SPEC-0115 behaviour 10.

6. **Field injection (v4):** the weak warning of SPEC-0115 behaviour 11 is given only where the
   framework's own documentation recommends constructor injection. [DOC; OBS-0037 FW1 observed it
   for Spring only]
   - **Micronaut:** its guide recommends the constructor and describes field injection as the
     lesser form. In a project whose findings are labelled "Micronaut", a field annotated
     `@Inject` gets the weak warning at the annotation, prefixed "Micronaut:", with the same
     message; not under `src/test/`, and not a field that carries only `@Value`, `@Property` or
     `@Client`.
   - **Jakarta EE and Quarkus:** no warning. The CDI specification names field, constructor and
     initializer injection side by side, and the Quarkus guides use field injection in their
     examples; their one advice, not to make injected members private, is another rule and is
     not checked.
   - The label decides (behaviour 1): in a project that names several frameworks only the rule
     of the first applies, so a Quarkus project with Spring's compatibility annotations gets no
     warning on `@Autowired`.

7. **Keys, values and gutter marks (v5):** SPEC-0115 behaviours 12 to 16 hold for these
   frameworks through the same bean model; nothing is decided per framework except what follows.
   [DOC; OBS-0037 KS2 to KS4 for Spring; the reference was not observed for these frameworks]
   - **Where a key stands in the code:** the `name` of `@ConfigProperty` (Quarkus, MicroProfile)
     and of `@Property` (Micronaut), a `${…}` placeholder of Micronaut's `@Value`, the prefix of
     `@ConfigMapping(prefix = "…")` (Quarkus) and of `@ConfigurationProperties("…")` (Micronaut).
     Go to Declaration, Find Usages and the quick documentation work on each as in SPEC-0115
     behaviour 12, and from a key of the configuration file back to these places.
   - **Matching:** as for the missing-key check (behaviour 5): `@ConfigProperty` keys exactly,
     Micronaut's `@Value` and `@Property` keys and every prefix with relaxed binding.
   - **Profiles:** a key written `%dev.key` is a definition of `key` for the profile `dev`; a
     file `application-test.yml` is of the profile (Micronaut: environment) `test`. Switched on
     by the configuration itself are the profiles of `quarkus.profile` and
     `micronaut.environments` of the default profile. From the line `%dev.key` Go to Declaration
     leads to the places that read `key`.
   - **Values:** only `${…}` placeholders are drawn as their value (Micronaut's `@Value`). The
     key of `@ConfigProperty` or `@Property` is a name, not a placeholder, and stays as written;
     its value is in the quick documentation and in `config_key`. Secrets are masked as in
     SPEC-0115 behaviour 13.
   - **Gutter marks:** on every bean of behaviours 2 and 5 — scope and resource classes,
     producer methods and producer fields, factory methods, interfaces the framework implements
     — and on every injection point (`@Inject` fields, setters and constructors, producer and
     factory parameters), with the candidates the model computes: after `@Named`, the project's
     own qualifiers, `@Alternative @Priority`, `@Primary`, `@DefaultBean` and `@Secondary`.
   - **Tool window and MCP:** the filters, the test beans, the dependencies panel, `config_key`
     and the additions to `spring_beans` are the same for every framework.

8. **Completion of keys (v6):** SPEC-0115 behaviour 19 holds for these frameworks; in addition,
   with the caret inside the string of `name = "…"` of `@ConfigProperty` (Quarkus,
   MicroProfile) or `@Property` (Micronaut), the list holds the defined keys as in a `${…}`
   placeholder, and the item replaces the whole string's key. Micronaut's `@Value("${…}")` is a
   placeholder already. `%profile.` keys and `application-{environment}` files count as in
   behaviour 7. The prefixes of `@ConfigMapping` and `@ConfigurationProperties` are not
   completed. [GEN; OBS-0073 part 1]

## Acceptance criteria
- **AK-1** CDI with Quarkus: scope and `@Path` beans, a named producer, `@Alternative @Priority`
  winning, `@Named` qualifier without bean, `@ConfigProperty` with and without default, frameworks
  "Quarkus", "Jakarta EE" (unit test `spring::tests::cdi_quarkus_beans_producers_and_config_properties`).
- **AK-2** Micronaut: factory beans, ambiguity message with `@Named`, `@Property` key with Go to Bean
  target (`spring::tests::micronaut_factory_beans_and_property_keys`).
- **AK-3** MCP: label, frameworks, candidate and config problem for a Quarkus resource
  (`crates/mcp/tests/it/protocol.rs`: `spring_beans_tool_reads_quarkus_cdi`).
- **AK-4** Window self-test: a Quarkus resource shows "Quarkus: No bean of type Missing found" and
  the undefined `@ConfigProperty` key as warnings (v2: the resource imports only `jakarta.` and
  MicroProfile, Quarkus comes from a `pom.xml`).
- **AK-5** Hand test (round 4): a Quarkus project (`quarkus create app`) and a Micronaut project,
  next to the reference.
- **AK-6 (v2)** A Quarkus project with only CDI and MicroProfile imports and the BOM through
  properties in `pom.xml`: frameworks "Quarkus", "Jakarta EE", ambiguity and missing key labelled
  "Quarkus:"; the same with a Gradle build; a Jakarta EE project with `jakartaee-api` stays
  "Jakarta EE"; `io.quarkusx` is not Quarkus; a Micronaut `pom.xml` adds Micronaut (unit tests
  `spring::tests::{quarkus_is_read_from_the_build_file, build_markers_are_whole_groups}`,
  integration test `crates/languages/tests/it/spring.rs`: `quarkus_project_without_quarkus_imports`).
- **AK-7 (v2)** MCP: `spring_beans` on such a Quarkus project returns framework "Quarkus"
  (`crates/mcp/tests/it/protocol.rs`: `spring_beans_tool_reads_quarkus_from_the_pom`).

- **AK-8 (v3)** Behaviour 5: `@Produces` of CDI, of Jakarta REST and of Micronaut HTTP, a producer
  field, a qualifier of the project's own in Jakarta EE and in Micronaut, the interfaces the
  framework implements, `@DefaultBean`, an `Optional` configuration value, a `%dev.` key (unit
  tests `spring::tests::{cdi_producers_and_own_qualifiers, framework_provided_beans,
  relaxed_and_external_keys}`).
- **AK-10 (v4)** Behaviour 6: an `@Inject` field of a Micronaut project is reported with severity
  weak and a `@Property` field is not; `@Inject` and `@Autowired` fields of a Jakarta EE and of a
  Quarkus project are not (unit test `spring::tests::field_injection_is_a_weak_warning`). The
  sample projects: `micronaut-notes` has the weak warnings at its `@Inject` fields,
  `jakarta-library` and `quarkus-greeting` have none
  (`crates/languages/tests/it/frameworks.rs`).
- **AK-11 (v5)** Behaviour 7, marks: a CDI scope bean and the `@Inject` field of a JAX-RS
  resource, a Micronaut factory method and the constructor of a controller, with targets both
  ways (unit test `spring_nav::tests::gutter_marks_of_cdi_and_micronaut`).
- **AK-12 (v5)** Behaviour 7, keys: `@ConfigProperty` to its line and to the `%dev.` line with
  its profile, no value for a key only a profile defines, from the `%dev.` line back to the
  field and the `@ConfigMapping` prefix, no fold for `@ConfigProperty`; Micronaut's `@Value`
  with relaxed binding and its shown value, `@Property`, the `@ConfigurationProperties` prefix
  (unit test `spring_nav::tests::keys_of_quarkus_and_micronaut`).
- **AK-13 (v5)** The sample projects: the keys of `GreetingResource.java` (Quarkus) and of
  `NoteController.java` (Micronaut) with their defining lines and values; the marks of
  `GreetingResource.java`, `NoteController.java` and `BookResource.java` (Jakarta EE) with the
  beans their injection points get, a producer method as the target and its mark leading back
  (`crates/languages/tests/it/frameworks.rs`: `keys_and_marks_of_the_other_samples`).
- **AK-14 (v6)** Behaviour 8: `@ConfigProperty(name = "…")` and `@Property(name = "…")` complete
  the defined keys, another annotation's `name` does not (unit test
  `config_completion::tests::keys_in_config_property_names`); MCP `complete_code` in a
  `@ConfigProperty` name of a Quarkus project (`crates/mcp/tests/it/protocol.rs`:
  `configuration_keys_complete_and_quarkus_starts_in_dev_mode`).
- **AK-9 (v3)** The sample projects `jakarta-library`, `quarkus-greeting` and `micronaut-notes`
  (`crates/languages/tests/fixtures/frameworks/`): each gives its beans and the problems it is
  written to have — one ambiguous injection point, one type without a bean and, for Quarkus and
  Micronaut, one undefined key — and no other
  (`crates/languages/tests/it/frameworks.rs`: `jakarta_library`, `quarkus_greeting`,
  `micronaut_notes`).

## Out of scope
CDI interceptors, decorators, events and stereotypes declared by the project, `@Any`, qualifier
members (`@Pay(method = CARD)`), `Instance<T>`, Quarkus build steps and Dev UI, evaluating
Micronaut `@Requires` conditions and build profiles, the keys the frameworks and extensions
declare (`quarkus.http.port`, `micronaut.server.port`) for completion and checking (v6), `@EachProperty`, `beans.xml` discovery modes,
which profile is active, the keys a `@ConfigMapping` interface or a `@ConfigurationProperties`
class reads.

## Known deviations from the reference
Types are matched by simple name; only types declared in the project are checked (as SPEC-0115).
A key defined only for one profile (`%dev.key`) counts as defined, as a key of a profile's file does
in SPEC-0115. Behaviour 5 follows the frameworks' documentation; the reference was not compared for
it. Behaviour 6 follows the frameworks' documentation too: what the reference reports on injected
fields of a Micronaut, a Jakarta EE or a Quarkus project was not observed. Beans and problems of
these three frameworks are not compared side by side; their endpoints are (SPEC-0103, OBS-0037).
Behaviour 7 (v5) carries what the reference showed for the Spring sample over to these
frameworks; what the reference does for their keys, values and gutter was not observed. The
deviations SPEC-0115 states for v5 hold here too: the value of the default profile unless the
configuration switches one on, no diagram of the bean graph, marks on the line of the injected
type, no drawing of keys as links. A profile chosen when the application is started (`-D`,
environment variables, the Quarkus dev mode's `dev`) is not known and never switched on.
