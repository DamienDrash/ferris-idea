# SPEC-0115: Spring beans and configuration

- **Version:** 7 · **Date:** 2026-11-16 (v7: completion of configuration keys in the code and
  in the configuration files, behaviour 19, found by the headless pre-check of F-040, OBS-0073;
  v6 2026-11-14: choosers sized to their rows and opened at the
  mark, the tool window's action behaves like its stripe button, behaviours 17 and 18, after the
  window check of v5; v5: configuration keys lead to their defining lines and
  back, resolved values in place of placeholders, gutter marks of beans and injection points,
  filters and a dependency panel in the tool window, behaviours 12 to 16, OBS-0037 findings KS1 to
  KS5; v4 2026-11-13: a weak warning on an injected field, behaviour 11,
  OBS-0037 finding FW1; v3: the bean and configuration forms the frameworks
  document, behaviours 9 and 10, found by running `spring_beans` on four sample projects; v2: only
  the project's own sources are read, behaviour 8)
- **Sources:** GEN (Spring-aware IDEs know a project's beans, show which bean an injection point gets,
  warn about injection points without a bean or with several, check `@Value` keys against the
  configuration files, and list the beans in a tool window; F-040), DOC (Spring Framework reference,
  read for the rules, not quoted: stereotype annotations make a class a bean, its default name is the
  class name with a small first letter unless the first two letters are capitals, `@Bean` methods of
  configuration classes declare beans named after the method, a class with one constructor is
  autowired through it, `@Qualifier` selects by name and `@Primary` wins among several,
  `${key:default}` placeholders; v3, Spring Framework, Spring Boot and Spring Data reference: a
  stereotype works as a meta-annotation of a composed annotation, `@Bean` methods may stand in any
  component class, `@Bean(name = {…})` gives a name and aliases, a repository interface that
  extends a Spring Data repository type is implemented by the framework, classes annotated
  `@ConfigurationProperties` are registered by scanning or `@EnableConfigurationProperties`,
  `@Profile` and the `@ConditionalOn…` annotations register a bean only under a condition,
  `.properties` syntax with `=`, `:` or white space after the key and `\` continuing a line, YAML
  lists flattened with `[index]`, relaxed binding of `@Value` keys written in kebab case, random
  values, system properties and environment variables as property sources; v4, Spring Framework
  reference on constructor-based and setter-based injection: the Spring team advocates the
  constructor for required dependencies, since it allows immutable components and dependencies
  that are never null; v5, Spring Boot reference on externalized configuration: profile-specific
  files `application-{profile}`, documents of a multi-document file activated with
  `spring.config.activate.on-profile`, `spring.profiles.active`, a `.properties` file taking
  precedence over a YAML file in the same location, the prefix of `@ConfigurationProperties`), OBS
  (OBS-0037: the problems the reference lists for
  `OrderController.java` of the Spring sample project, finding FW1; v5, its run of 2026-11-14 on
  `ProductService.java`: findings KS1 to KS5), STD (Java syntax,
  tree-sitter grammar), SPEC-0103 (endpoints), SPEC-0070 (problems in the editor), ADR-0008
- **Feature matrix rows:** F-040

## Purpose
Catch Spring wiring mistakes while typing instead of at application start, and get from where a bean
is used to where it is declared. The reference's Spring support is proprietary; this is Ferris IDEA's
own model, built from the rules above in a clean room.

## Behaviour
1. **Model:** the project's Java files are read with tree-sitter. Beans: classes annotated
   `@Component`, `@Service`, `@Repository`, `@Controller`, `@RestController`, `@Configuration`,
   `@SpringBootApplication`, `@ControllerAdvice`, `@RestControllerAdvice` (name from the annotation's
   value or the class name), and `@Bean` methods of configuration classes (name from the annotation
   or the method, type from the return type). `@Primary` is noted. [DOC, STD]
2. **Injection points:** parameters of the constructor Spring uses (the `@Autowired` one, else the
   only one), `@Autowired`/`@Inject` fields and setters, and parameters of `@Bean` methods;
   `@Value` parameters and containers (`List`, `Set`, `Map`, `Optional`, `ObjectProvider`, …) are not
   bean lookups. A bean fits when its type or one of its project supertypes (`extends`, `implements`,
   transitively) has the injected type's simple name; `@Qualifier`/`@Named` keeps the bean of that
   name; among several, a single `@Primary` wins. [DOC]
3. **Configuration:** keys of `application*.properties` and `application*.yml`/`.yaml` (and
   `bootstrap*`) in the project, YAML flattened with dots by indentation; `${key}` and
   `${key:default}` inside `@Value`. [DOC]
4. **Problems:** an injection point of a type declared in the project with no fitting bean ("No bean
   of type X found", "No bean named 'q' of type X") or with several ("2 beans of type X (a, b); mark
   one @Primary or add @Qualifier"); a `@Value` key without default that no configuration file
   defines. Types not declared in the project (library and auto-configured beans) are not checked.
   The problems show as warnings in open editors, prefixed "Spring:", after opening or saving a Java
   or configuration file. [GEN]
5. **Go to Bean:** `Spring.GotoBean` (Search Everywhere) on an injection point's type opens the bean
   that is injected, on a `@Value` key the property's line; several candidates open the first and
   list all in the status bar. [GEN]
6. **Tool window:** `Spring.ToolWindow` shows the problems first, then the beans grouped by kind with
   name, type and place; Enter or a double click opens the declaration, Escape hides it. [GEN]
7. **MCP:** `spring_beans` returns the model (beans, injection points with candidates, keys, `@Value`
   keys) and the problems. [ADR-0008]
8. **Only the project's own sources (v2; decision of the coordinator of 2026-11-13 under the Project Owner's general authority: the rule the Project Owner chose for Rename, Find in Files, Search Everywhere, Inspect Code and Endpoints):** the model (behaviours 1 to 3), the Spring tool
   window and `spring_beans` read the files that are the project's own by the rule of SPEC-0148 v4
   behaviour 7 (`OwnSources`). A class or a configuration file in a virtual environment, in
   installed dependencies, in build output (a copy of `application.properties` below `build`), in
   a folder the project model excludes or matched by an ignore rule of Git without being tracked
   is not one of the project's beans and keys: it gave beans nobody declared, second candidates
   for an injection point and keys that are not configured. There is no option to read them. [GEN]

9. **Bean forms (v3):** on the sample projects of F-040 the model missed beans the framework
   provides and so reported injection points as having no bean, named a bean after
   `destroyMethod`, and called two beans of different profiles ambiguous. [DOC]
   - **Composed annotations:** an annotation type the project declares that carries a stereotype
     as a meta-annotation, directly or through another composed annotation, makes the classes it
     annotates beans of that stereotype's kind.
   - **`@Bean` methods** declare beans in every bean class, not only in configuration classes; a
     class annotated `@TestConfiguration` is a configuration class.
   - **Names:** a stereotype's name is its value written alone or as `value = "…"`; a `@Bean`
     method's name is its value or `name = "…"`, the first entry of an array, the others being
     aliases a `@Qualifier` also finds. No other attribute (`destroyMethod`, `initMethod`) is a
     name.
   - **Repository interfaces:** an interface that extends, directly or through other interfaces of
     the project, a type not declared in the project whose name ends in `Repository`
     (`Repository`, `CrudRepository`, `JpaRepository`, …) is a bean of kind `Repository`, unless
     it is annotated `@NoRepositoryBean`. An interface annotated `@FeignClient` is a bean.
   - **Configuration classes:** a class, record or interface annotated `@ConfigurationProperties`
     is a bean of that kind; its prefix is not a bean name and its constructor parameters are
     bound values, not injection points. Whether the application enables it
     (`@ConfigurationPropertiesScan`, `@EnableConfigurationProperties`) is not checked.
   - **Conditions:** a bean whose class or `@Bean` method carries `@Profile` or an annotation
     whose name starts with `Conditional` is conditional. An injection point whose candidates are
     all conditional is not reported as ambiguous (the active profile chooses); all of them stay
     candidates for Go to Bean. A bean marked `@ConditionalOnMissingBean` gives way when another
     bean fits.
   - **Test sources:** a bean declared under `src/test/` is a candidate only for injection points
     under `src/test/`; the application does not see it.
   - **No lookup:** an array type and a primitive type are not bean lookups.
10. **Configuration keys (v3):** [DOC]
   - **`.properties`:** the key ends at `=`, `:` or white space; a line ending in an odd number of
     `\` continues on the next line, which is no key.
   - **YAML:** only keys that carry a value are properties (`shop.currency`, not `shop`); the
     items of a list carry their index (`shop.providers[0].url`); every document of a
     multi-document file is read; the lines of a block scalar are text.
   - **Relaxed binding:** a `@Value` key written in lower case with hyphens also finds the same
     key written in camel case or with underscores (`shop.max-items` finds `shop.maxItems`). A
     key written in camel case finds only itself.
   - **Profiles:** a key defined only in a profile's file (`application-dev.yml`) or document
     counts as defined.
   - **Keys from the environment:** `random.*`, the JVM's system properties (`user.home`,
     `user.dir`, `user.name`, `java.home`, `java.version`, `java.io.tmpdir`, `os.name`, `os.arch`,
     `os.version`, `file.encoding`, `file.separator`, `line.separator`, `path.separator`) and names
     in the form of environment variables (capitals, digits and `_`) are not reported as
     undefined.

11. **Field injection (v4):** a field annotated `@Autowired` gets a weak warning at the
   annotation, "Field injection: constructor injection is recommended", prefixed "Spring:" like
   the other findings. The reference reported five such warnings in the controller of the sample
   project, one at each `@Autowired` of a field, and Ferris IDEA none. [OBS-0037 FW1; DOC for the
   recommendation; the message is Ferris IDEA's own]
   - **Which fields:** every `@Autowired` field of a class in the project's own sources, whatever
     its type (a project type, a library type, a container) and whatever else it carries
     (`@Qualifier`, `required = false`). Not a constructor, a setter or a `@Bean` method, not a
     `@Value` field, and not a field annotated only with `@Inject` or `@Resource`.
   - **Where not:** under `src/test/`, where Spring's test support injects into the fields of a
     test class, and in a project whose findings carry another framework's label (SPEC-0134
     behaviour 6 says which of them have the rule).
   - **Severity:** a weak warning in the editor; the wiring and configuration problems of
     behaviour 4 stay warnings. The Spring tool window lists the weak warnings after the
     warnings, muted. `spring_beans` gives every problem a `severity`: `warning` or `weak`.
   - **No quick fix:** turning the field into a constructor parameter is not offered yet.

12. **Configuration keys lead to their lines and back (v5):** in the reference a key inside
   `${…}` is a link, and Go to Declaration on it opens the line that defines it; Ferris IDEA
   answered that there is no declaration. [OBS-0037 KS2; DOC for the rules; GEN for the reverse
   direction, Find Usages and the documentation, which the run did not compare]
   - **Where a key stands in the code:** inside a `${…}` placeholder of `@Value`, with or without
     a default; as the prefix of `@ConfigurationProperties` (`"shop"`, `prefix = "shop"`,
     `value = "shop"`). SPEC-0134 behaviour 7 adds the forms of the other frameworks.
   - **Go to Declaration** (`GotoDeclaration`, ⌘B, ⌘-click) with the caret in such a key opens
     the line of the configuration file that defines it, the caret on the key. The defining lines
     are those of behaviours 3 and 10: `application*` and `bootstrap*` files ending in
     `.properties`, `.yml` or `.yaml`, files of a profile, every document of a multi-document
     YAML file, and relaxed binding (a key written in lower-case kebab form also finds its
     camel-case or underscore spelling). One defining line is opened; several open a chooser
     whose rows give file, line, profile and value (masked as in behaviour 13). A prefix leads to
     every key below it. A key nothing defines says so in the status bar; the language server is
     not asked for a place the model knows.
   - **The reverse:** Go to Declaration with the caret in a key of a configuration file — in a
     YAML file the last part of a key that carries a value — opens the place in the code that
     reads it; several open a chooser. The places are the keys of the forms above that the line
     defines, and the prefixes it lies below. A key nothing reads says so in the status bar.
   - **Find Usages** (`FindUsages`, `FindUsagesInFile`, `ShowUsages`) on a key, in the code or in
     a configuration file, lists the places in the code that read it, in the Find tool window or
     the usages popup like the usages of a symbol; the first defining line is the declaration.
   - **Quick documentation** (`QuickJavaDoc`) on a key shows the key, the value the application
     gets (behaviour 13) or that the default profile has none, each defining line with its
     profile and value, and the number of places that read it.
   - **The text the model read:** the model is built from the files on disk. A place whose text
     in the editor is no longer the key the model read there is not treated as a key.
13. **The resolved value in place of the placeholder (v5):** the reference shows a placeholder
   whose key resolves folded as its value, and as written again under the caret. [OBS-0037 KS3;
   DOC for the precedence]
   - **What is shown:** in a Java editor, each `${…}` placeholder of a `@Value` whose key has a
     value is drawn as that value, in the colours of folded text; the text is not changed. With
     the sample project `@Value("${shop.name}")` reads `@Value("Corner Shop")`. A placeholder that a
     caret or a selection touches is drawn as written, so a click on the value opens it.
   - **Which value:** the definition of a profile the configuration itself switches on
     (`spring.profiles.active` of the default profile; several profiles separated by commas),
     the last such definition; else the definition of the default profile, a `.properties` file
     before a YAML file, the last of them. A profile is the file's (`application-dev.yml` →
     `dev`), the document's (`spring.config.activate.on-profile`, or `spring.profiles` as older
     versions write it), or the key's (SPEC-0134). A key only a profile defines that nothing
     switches on has no value and stays as written, and so does a key nothing defines, also when
     the placeholder carries a default. A value is the scalar as written, quotes of YAML removed
     and continued lines of a `.properties` file joined; a block scalar has none.
   - **Secrets:** a key whose name contains `password`, `passwd`, `secret`, `token`, `key` or
     `credential` — whatever the case, with or without `-` and `_` — never shows its value: the
     placeholder is drawn as `•••`, and the chooser, the quick documentation and the MCP tools
     give `•••` for every value of that key. The rule errs on the safe side: `shop.keyboard` is
     masked too. There is no setting that shows these values. [security first, CLAUDE.md rule 12;
     IMP-0409]
   - **Setting:** *Show configuration values in place of ${…} placeholders* on the Editor page,
     `config_values` under `[editor]` and in `set_ide_settings`, on by default. Off: every
     placeholder is drawn as written; navigation and marks stay.
   - **When:** with the bean model, after opening or saving a Java or configuration file. An
     edit moves the values after it and drops the one it touches until the next save.
14. **Gutter marks (v5):** the reference marks the line of a bean class and the line of the
   constructor that takes beans, and a click leads to the other side of the wiring. [OBS-0037
   KS4; glyphs and tooltips are Ferris IDEA's own]
   - **Bean:** `●` on the line of the name of every bean class and of every method or field
     that declares a bean (behaviours 1 and 9). A click leads to the injection points the bean
     fits.
   - **Injection point:** `→` on the line of the injected type of every injection point
     (behaviour 2) whose type the project declares or for which a bean of the project fits. A
     click leads to the beans that fit: after `@Qualifier` and `@Primary`, and without the beans
     of the test sources for an injection point of the main sources. Several injection points on
     one line share the mark and its targets.
   - **Both** on one line (a `@Bean` method with parameters): one mark `◉` with the targets of
     both.
   - **Click:** one target is opened; several open a chooser (a bean's row: name, type, file and
     line; an injection point's row: its class, file and line); none shows the tooltip's text in
     the status bar. The tooltip says what the mark stands for: the bean and in how many places
     it is injected, or the injected type and the bean it gets.
   - **Lane:** the marks stand in a lane of their own left of the marks of SPEC-0320, so a line
     can carry both. They follow the model: an edit of a line drops its mark until the next save.
15. **Tool window: filters, test beans, dependencies (v5):** the reference's bean window has a
   search field, filters by module and by kind of bean, and a panel with a graph of the beans
   and documentation; it listed 16 beans where Ferris IDEA counted 21. [OBS-0037 KS5; GEN]
   - **Main sources by default:** the list holds the beans of the application. A bean declared
     under `src/test/` is left out unless *Tests* is switched on, and is then marked as a test
     bean. The header counts the beans listed, of how many, and names the number in tests that
     are not listed ("18 beans · 3 in tests not listed"). The sample project's 21 beans are 18 of
     the application and 3 of its test sources.
   - **Text filter:** text typed while the window has the focus narrows the list to the beans
     whose name, type or kind contains it, whatever the case; Backspace removes a character,
     Escape first clears the text and then hides the window.
   - **Kind filter:** *Kind* shows the beans of one kind (`Service`, `Bean`, `Repository`, …) or
     of all; a click takes the next kind the project has.
   - **Problems** are listed while neither text nor kind narrows the list.
   - **Dependencies panel:** beside the list, for the selected bean: its name and type, kind and
     place (and `primary`, `conditional`, `fallback`, `test` where they apply), under *Needs* the
     beans its injection points get (`→`; a type of the project without a bean as `✕`), under
     *Needed by* the beans with an injection point it fits (`←`). The injection points of a
     class are its constructor, fields and setters; those of a `@Bean` method are its parameters
     and belong to the bean it declares, not to its class. A click on a row opens that bean, or
     the injection point without a bean.
16. **MCP (v5):** `spring_beans` marks a test bean (`test`), counts `main_beans` and
   `test_beans`, names the class of an injection point (`owner`) and the bean a producer
   parameter belongs to (`producer`), gives a property its column, key range and `profile`, and
   marks a prefix; with `path` it returns the gutter `marks` and the `values` shown for that
   file, with `bean` the `dependencies` and `dependents` of that bean. `config_key` returns for
   one key — by name, or by a place in a Java or configuration file — its `definitions`,
   `usages`, `value`, `value_profile` and `secret`. No tool returns the value of a key that looks
   like a secret: the model serializes no values, and `config_key` and `values` mask them.
   [ADR-0008]

17. **Choosers of places (v6):** in the window check of v5 the choosers were as wide as every
   popup, and a row that began with the path from the project folder was cut off before the
   file name and line (`src/main/java/com/example/shop/config/ShopPropertie`); the chooser of a
   gutter mark opened at the top of the editor, far from the mark. [window check of 2026-11-14;
   GEN]
   - **Rows:** every row of the choosers of behaviours 12 and 14 starts with the file name and
     the 1-based line (`ShopProperties.java:5`), then what stands there — the key as written, the
     profile and value of a defining line, a bean's name and type, the class of an injection
     point — and last, dimmed, the file's folder below the project folder.
   - **Width:** the popup is as wide as its longest row with its folder and its title, at least
     as wide as the other popups and at most four fifths of the width of the editor it opens
     over. The rows are not measured: the width is estimated per character, rather too wide than
     too narrow.
   - **Place:** the chooser of a gutter mark opens at the mark: under its line at the editor's
     left edge, or over the line when there is no room under it, never higher than the room.
     The chooser of Go to Declaration opens at the caret in the same way.
18. **Opening the tool window (v6):** the action `Spring.ToolWindow` — from the menu bar, Find
   Action or a shortcut — behaves like the stripe button of any tool window: the window of its
   placement gives way at once, a shown Spring window without the focus takes the focus, a
   focused one hides. The row of the More tool windows list shows the window, or hides it when
   it is shown; a window that is marked as shown but was never painted since is shown, not
   hidden. [GEN; SPEC-0061 v10 behaviour 53, SPEC-0167 behaviour 2]
   - **Reported and not reproduced:** in the window check of v5 the menu item was disabled and
     the row of the More list opened nothing. The same steps in the window self-test, with
     another window at the bottom and with a chooser open, drew the window before and after
     this change. The menu bar disables every action item while the focus is on an element
     that is not drawn; what put the focus there in the checked window is not known. The rule
     above removes the one state that explains both findings — a window marked as shown that
     nobody sees — and AK-19 keeps the two ways of opening under test.

19. **Completion of configuration keys (v7):** the headless pre-check of F-040 (OBS-0073) found
   no completion at all where a key is typed: `complete_code` inside `${` of a `@Value` returned
   nothing, and in `application.properties` it failed for want of a language server. IDEs with
   Spring support complete keys in both places. [GEN; OBS-0073 part 1; the reference's lists were
   not observed]
   - **In the code:** with the caret inside a `${…}` placeholder of a Java string, before its
     `:` and `}`, the list holds the keys the configuration files of behaviours 3 and 10 define,
     once each (relaxed spellings count as one; a `%profile.` key as its key), sorted, those
     starting with the text typed after `${` (case ignored). Beside each: the value the
     application gets (behaviour 13, masked as `•••` for a key that looks like a secret) and the
     file, or the profiles that alone define it ("only in dev"). The item replaces the whole key
     around the caret. SPEC-0134 behaviour 8 adds the other frameworks' forms.
   - **In a `.properties` file:** with the caret in the key part of a line (before any `=`, `:`
     or white space after the key; not in a comment), first the keys the code reads (behaviour
     12's places, no prefixes, no keys of the environment) that the file does not define —
     those no file defines first, each with "read in File.java:line" — then the keys other
     configuration files define. The typed text narrows them; at the end of the line the item
     inserts `key=`; a typed `%dev.` stays before the key.
   - **In a YAML file:** at a key of a map line (not in a list or after the `:`), the same keys
     below the maps the line stands in, written as the rest of the key (`max-items` under
     `shop:`), followed by `: ` at the end of the line; keys the file already defines are left
     out.
   - **No language server** is asked at such a place; elsewhere the language server answers as
     before. The list opens with the completion action; inside a string of the code it does
     not open by itself (SPEC-0072 behaviour 8), in a `.properties` file it does.
   - **MCP:** `complete_code` returns the same items (`kind` `property`). [ADR-0008]

## Acceptance criteria
- **AK-1** Beans from stereotypes and `@Bean` methods, injection by type and supertype, library
  types unchecked, containers and `@Value` parameters left out, missing/ambiguous/qualified/`@Primary`,
  properties and YAML keys, Go to Bean targets (unit tests `spring::tests`, `spring_view::tests`).
- **AK-2** The reference project `spring-basic` gives its three beans, resolves the repository in the
  controller and has no problems (`crates/languages/tests/it/spring.rs`).
- **AK-3** MCP: beans and both kinds of problems (`crates/mcp/tests/it/protocol.rs`: `spring_beans_tool`).
- **AK-4** Window self-test: the two warnings in the editor, Go to Bean from an interface to its
  implementation, the tool window with the problem and the beans.
- **AK-5** Hand test (round 4): `spring-basic` and a project with an interface with two
  implementations, next to the reference.
- **AK-6 (v2)** MCP in a temporary project with `@Service` classes in an own file, in
  `.venv/lib/python3.12/site-packages/x.java`, `node_modules/y.java` and a file an ignore rule
  matches: `spring_beans` lists only the bean of the own file
  (`crates/mcp/tests/it/protocol.rs::spring_beans_and_reference_index_read_the_projects_own_sources`).

- **AK-7 (v3)** Behaviour 9: a composed stereotype, `@Bean` in a component class, names and
  aliases, a Spring Data repository interface with and without `@NoRepositoryBean`, a
  `@ConfigurationProperties` class, profiles and `@ConditionalOnMissingBean`, beans of the test
  sources, arrays (unit tests `spring::tests::{composed_stereotypes_and_bean_names,
  framework_provided_beans, conditional_and_test_beans}`).
- **AK-8 (v3)** Behaviour 10: `.properties` separators and continuation lines, YAML values, lists,
  documents and block scalars, relaxed binding in one direction only, profile files, keys from the
  environment (unit tests `spring::tests::{configuration_file_keys, relaxed_and_external_keys}`).
- **AK-10 (v4)** Behaviour 11: `@Autowired` fields alone, with a qualifier, with
  `required = false`, of a container and of a library type are reported at the annotation with
  severity weak; a constructor, a setter, a `@Value` field, an `@Inject` field and a field of a
  test class are not; a problem carries its severity as JSON (unit test
  `spring::tests::field_injection_is_a_weak_warning`). The sample project `spring-shop`: the five
  weak warnings of `OrderController.java` at lines 22, 26, 30, 35 and 38, where the reference has
  them, and no other (`crates/languages/tests/it/frameworks.rs`: `spring_shop`); MCP: `severity`
  of the problems (`crates/mcp/tests/it/protocol.rs`: `frameworks_sample_project_spring`).
- **AK-19 (v6)** Behaviour 18, window self-test (`spring_steps`): with Problems shown at the
  bottom, the action of View › Tool Windows › Spring draws the Spring tool window in its place
  (painted since it was shown, at least 200 by 80, Problems hidden) and hides it again; the row
  of the More tool windows list does the same while a chooser is open, and hides the shown
  window. The steps call what the menu item and the row call; a native click on them is a step
  of the window check.
- **AK-20 (v6)** Behaviour 17: the width a popup needs for its rows, never narrower than the
  usual popup, never wider than its room (`choice_view::tests::the_popup_is_sized_to_its_rows`);
  rows start with file and line, the folder is the dimmed detail
  (`workbench_view::workbench_frameworks::tests::decorations_of_a_file`); window self-test: the
  chooser of a mark with two targets has rows starting `CardGatewayImplementation.java:3`, the
  folder `spr/payment/gateways/internal` as detail, is wider than the usual popup and stands at
  the mark's line.
- **AK-11 (v5)** Behaviour 12, to the definition: a `@Value` key leads to its line in a
  `.properties` file, a YAML file, a profile's file and a profile's document; two definitions
  are two rows with their profiles; relaxed binding finds the camel-case spelling; a prefix
  leads to every key below it; an undefined key has none; the quick documentation names value,
  defining lines and usages (unit tests `spring_nav::tests::{keys_lead_to_their_defining_lines,
  key_documentation}`).
- **AK-12 (v5)** Behaviour 12, the reverse and Find Usages: a key of a configuration file leads
  to the fields that read it and to the prefix it lies below, the value's text is no key, and the
  usages from the code are the same places (unit test
  `spring_nav::tests::definitions_lead_back_to_their_usages`).
- **AK-13 (v5)** Behaviour 13: the values of the default profile, none for a key of a profile
  nothing switches on, for an undefined key and for a default; the value of a profile switched
  on with `spring.profiles.active`; `•••` for keys that look like secrets, in the folds, the
  key information, its JSON and the documentation (unit test
  `spring_nav::tests::placeholders_show_resolved_values_and_mask_secrets`); values of
  `.properties` and YAML files, documents and profiles (`spring::tests::configuration_values`);
  the value replaces the placeholder's columns in the drawn line, a caret or selection that
  touches it shows it as written, an edit moves and drops it
  (`value_fold_model::tests::{the_value_replaces_the_placeholder_in_the_display,
  carets_open_a_placeholder_and_edits_move_it}`); the setting off gives marks and no values
  (`workbench_view::workbench_frameworks::tests::decorations_of_a_file`).
- **AK-14 (v5)** Behaviour 14: marks on bean classes, `@Bean` methods and injection points with
  targets both ways, two injection points of one line in one mark, a test bean no target of a
  main injection point, a type without a bean a mark without target (unit test
  `spring_nav::tests::gutter_marks_lead_both_ways`); marks of one line merge
  (`decl_marks_model::tests::marks_of_one_line_merge`).
- **AK-15 (v5)** Behaviour 15: test beans left out by default and marked when shown, the text
  filter by name, type and kind, the kind filter, problems only while nothing narrows, the panel
  with what a bean needs and what needs it (unit tests `spring_view::tests::{
  rows_put_problems_first_and_group_beans, filters_narrow_the_beans,
  details_name_dependencies_and_dependents}`, `spring_nav::tests::dependencies_and_dependents`).
- **AK-16 (v5)** Behaviour 16 on the sample project: `config_key` by name and by place, from the
  code and from the configuration file, a masked secret; `spring_beans` with `path` and `bean`;
  18 main and 3 test beans (`crates/mcp/tests/it/protocol.rs`:
  `config_key_and_marks_of_the_spring_sample`, `frameworks_sample_project_spring`).
- **AK-17 (v5)** The sample project `spring-shop`: the eight keys of `ProductService.java` with
  their defining lines and values (`shop.currency` twice, value `EUR`), the three values shown,
  the usages of `shop.name`, the marks of `ProductService.java`, `OrderController.java` and
  `AppConfig.java`, and the three test beans among 21
  (`crates/languages/tests/it/frameworks.rs`: `spring_shop_keys_marks_and_test_beans`).
- **AK-18 (v5)** Window self-test (`spring_steps`): ⌘B on a `@Value` key opens the line of
  `application.properties`, ⌘B there leads back to the Java file, the editor shows the value in
  place of the placeholder and `•••` for a secret and the placeholder under the caret as
  written, the marks of the bean and of the injection point are set with their targets, the tool
  window narrows by typed text, shows the dependencies of the selected bean, and Escape clears
  the text. The click on a mark is not part of the self-test (its targets are, AK-14); it is a
  step of the window check.
- **AK-21 (v7)** Behaviour 19: the keys in a placeholder with value, profile-only key and masked
  secret and the range they replace, nothing after the default's colon or outside a string; in
  a `.properties` file the keys read and not defined first, `=` and a `%dev.` prefix, nothing in
  a value or comment; in a YAML file the rest of the key below its maps (unit tests
  `config_completion::tests::{keys_in_a_placeholder, keys_in_a_properties_file,
  keys_in_a_yaml_file}`); MCP `complete_code` in a `.properties` file without a language server,
  the secret's value absent (`crates/mcp/tests/it/protocol.rs`:
  `configuration_keys_complete_and_quarkus_starts_in_dev_mode`).
- **AK-9 (v3)** The sample project `crates/languages/tests/fixtures/frameworks/spring-shop`: 21
  beans, the three problems it is written to have (two `PaymentGateway` beans, no `AuditLog`
  bean, the key `shop.missing-key`) and no other warning
  (`crates/languages/tests/it/frameworks.rs`: `spring_shop`); MCP `spring_beans` on it
  (`crates/mcp/tests/it/protocol.rs`: `frameworks_sample_project_spring`).

## Out of scope
Kotlin sources, XML bean definitions, component scan packages, evaluating conditions and profile
expressions (which profile is active, whether two profiles exclude each other), generics in bean
matching, beans from libraries and auto-configuration,
`@ConfigurationProperties` binding (which keys a class reads), constructors generated by Lombok,
`@Resource`, `@Autowired(required = false)`, qualifier annotations of the project's own in Spring
(SPEC-0134 reads them), a quick fix for field injection (behaviour 11).
Since v5 also: a diagram of the bean graph and a documentation pane in the tool window, a
filter by module, rename of configuration keys, which field of a
`@ConfigurationProperties` class binds which key (the prefix leads to all keys below it), keys
used elsewhere than in the forms of behaviour 12 (`@ConditionalOnProperty`, `Environment`
lookups, placeholders inside the configuration files, Kotlin sources), profiles switched on
outside the configuration files (run configurations, environment variables, command lines),
`spring.config.import` and profile groups, drawing keys as links.
Since v7 also: the keys libraries declare in their configuration metadata
(`META-INF/spring-configuration-metadata.json`, `server.port`, `spring.datasource.url`) — neither
completed nor checked, so a misspelt key in a configuration file is not reported; values (an
enum's constants, `true`/`false`); keys of `@ConfigurationProperties` classes.

## Known deviations from the reference
Since v7 the completion of keys (behaviour 19) offers only the project's own keys: the keys of the
framework and its libraries, which the reference completes from their metadata, are missing, and
unknown keys in a configuration file are not reported. Not compared side by side (OBS-0073 part 2
plans it).
The model is read from the files on disk, so problems, marks and shown values follow saving, not
every keystroke; types are matched by simple name.
Compared side by side on `ProductService.java` of the Spring sample project (OBS-0037, run of
2026-11-14), since v5:
- **Better, kept:** Ferris IDEA reports the `@Value` key without a default that no file defines;
  the reference did not (KS1, IMP-0408). Values of keys that look like secrets are never shown
  (IMP-0409); whether the reference shows them was not observed.
- **Which value:** for `shop.currency` the reference showed `USD`, the value of the document of
  the profile `prod`; Ferris IDEA shows `EUR`, the value of the default profile, because nothing in
  the configuration switches `prod` on. Whether a profile was set as active in the reference was
  not checked. A deviation on purpose: the value shown is the one the application gets when it
  is started as the files say.
- **No link drawing:** a key is not underlined or coloured as a link; Go to Declaration and
  ⌘-click work on it.
- **Mark of an injection point:** the reference marks the line of the constructor; Ferris IDEA
  marks the line of each injected type, which is the constructor's line when its parameters
  stand on it, and each field's line for field injection. Whether the reference marks fields
  was not observed.
- **Tool window:** no diagram of the bean graph — the panel lists what the selected bean needs
  and what needs it, one level, as text — and no documentation pane; no filter by module (one
  model per project). Ferris IDEA's problem list in the window has no counterpart in the
  reference. The reference listed 16 beans; Ferris IDEA lists 18 for the main sources and 3 more
  with *Tests*. Which two the reference does not list was not compared.
- **Find Usages** on a key lists places without the kind and context columns of symbol usages.
  Rerun is answered by the model while the caret is still on the key; after the caret moved it
  asks the language server for the old place, which knows no key.
- **Folding:** the shown value is not a fold of SPEC-0083: the fold actions do not open or close
  it; the caret does.
Earlier comparison, unchanged: Compared side by
side for one file of the Spring sample project (OBS-0037): the two wiring problems agree, and
behaviour 11 adds the field injection warnings the reference had. The reference showed the two
wiring problems as errors, Ferris IDEA shows them as warnings (behaviour 4, unchanged). Whether the
reference reports field injection in test classes, and its unused-field warning (a general Java
inspection, not part of this model), were not compared. Find Usages and the quick documentation
on a key, the way back from a configuration file and the prefixes were not compared (behaviour
12 follows GEN and the frameworks' documentation there).
Since v2 only the project's own sources are read (behaviour 8); the reference was not compared for
this.
Since v3 a `@ConfigurationProperties` class counts as a bean without checking that the application
enables it, and candidates that are all conditional are never called ambiguous, even where their
conditions could hold together: fewer false warnings at the price of a missed one. Behaviours 9 and
10 follow the frameworks' documentation; the reference was not compared for them.
