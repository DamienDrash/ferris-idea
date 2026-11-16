//! SPEC-0103/AK-7, SPEC-0115/AK-9 and AK-10, SPEC-0134/AK-9 and AK-10: the four sample projects of F-040 in
//! `tests/fixtures/frameworks/` — a Spring Boot, a Jakarta EE, a Quarkus and a Micronaut
//! application, sources only — give the endpoints and beans their frameworks' documentation
//! describes and exactly the problems they are written to have. The rows are the ones the
//! reference listed for the Spring, Jakarta EE and Quarkus samples (OBS-0037, EP1 to EP4), the
//! weak warnings of the Spring sample the ones it reported (FW1).

use std::path::{Path, PathBuf};

use idecraft_languages::inspections::Level;
use idecraft_languages::spring::SpringModel;

/// The endpoint rows as `METHODS path handler file:line` (1-based line; several methods joined
/// with `|`, none for a mapping without a method, a client row marked ` (client)`) and the bean
/// model of a sample.
fn sample(name: &str) -> (Vec<String>, SpringModel) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/frameworks")
        .join(name);
    let files: Vec<PathBuf> = idecraft_core::Workspace::open(&root)
        .unwrap()
        .files()
        .unwrap();
    let endpoints = idecraft_languages::endpoints::endpoints(&root, &files)
        .into_iter()
        .map(|e| {
            format!(
                "{} {}:{}",
                e.row_text(),
                e.file.file_name().unwrap().to_string_lossy(),
                e.line + 1
            )
        })
        .collect();
    (endpoints, idecraft_languages::spring::model(&root, &files))
}

/// The problems of severity `level` as `file:line message` (1-based line), sorted.
fn problems_of(m: &SpringModel, level: Level) -> Vec<String> {
    let mut out: Vec<String> = m
        .problems()
        .into_iter()
        .filter(|p| p.severity == level)
        .map(|p| {
            format!(
                "{}:{} {}",
                p.file.file_name().unwrap().to_string_lossy(),
                p.line + 1,
                p.message
            )
        })
        .collect();
    out.sort();
    out
}

/// The warnings: wiring and configuration mistakes.
fn problems(m: &SpringModel) -> Vec<String> {
    problems_of(m, Level::Warning)
}

/// The weak warnings (SPEC-0115 v4 behaviour 11): the 1-based lines of the annotations of
/// injected fields, as `file:line`, after checking that each carries the recommendation.
fn field_injections(m: &SpringModel) -> Vec<String> {
    let suffix = format!(" {}", idecraft_languages::spring::FIELD_INJECTION);
    problems_of(m, Level::Weak)
        .into_iter()
        .map(|p| {
            p.strip_suffix(&suffix)
                .unwrap_or_else(|| panic!("another weak warning: {p}"))
                .to_owned()
        })
        .collect()
}

/// The beans as `name: Type (kind)`, sorted.
fn beans(m: &SpringModel) -> Vec<String> {
    let mut out: Vec<String> = m
        .beans
        .iter()
        .map(|b| format!("{}: {} ({})", b.name, b.type_name, b.kind))
        .collect();
    out.sort();
    out
}

/// The names of the beans the injection point of type `ty` in `file` at 1-based `line` gets.
fn injected<'a>(m: &'a SpringModel, file: &str, line: usize, ty: &str) -> Vec<&'a str> {
    let point = m
        .injections
        .iter()
        .find(|i| i.file.ends_with(file) && i.line + 1 == line && i.type_name == ty)
        .unwrap_or_else(|| panic!("no injection point {ty} at {file}:{line}"));
    point
        .candidates
        .iter()
        .map(|&c| m.beans[c].name.as_str())
        .collect()
}

#[test]
fn spring_shop() {
    let (endpoints, m) = sample("spring-shop");
    assert_eq!(
        endpoints,
        [
            "POST /api/orders OrderController.place OrderController.java:47",
            "GET /api/orders/health OrderController.health OrderController.java:53",
            "GET /api/orders/{id} OrderController.one OrderController.java:41",
            "GET /api/products ProductController.all ProductController.java:32",
            "GET /api/products ProductController.byName ProductController.java:42",
            // The path `/` keeps its slash (EP2), two methods are one row (EP1), a mapping without
            // a method shows none (EP3).
            "POST /api/products/ ProductController.create ProductController.java:47",
            "POST|PUT /api/products/bulk ProductController.bulk ProductController.java:77",
            "/api/products/ping ProductController.ping ProductController.java:82",
            "GET /api/products/search ProductController.search ProductController.java:72",
            "PUT /api/products/update/{id} ProductController.update ProductController.java:53",
            "DELETE /api/products/{id} ProductController.delete ProductController.java:67",
            "GET /api/products/{id} ProductController.one ProductController.java:37",
            "PUT /api/products/{id} ProductController.update ProductController.java:53",
            "PATCH /api/products/{id}/price ProductController.reprice ProductController.java:60",
            "POST /orders OrderController.place OrderController.java:47",
            "GET /orders/health OrderController.health OrderController.java:53",
            "GET /orders/{id} OrderController.one OrderController.java:41",
        ]
    );
    assert_eq!(m.frameworks, ["Spring"]);
    assert_eq!(
        beans(&m),
        [
            "appConfig: AppConfig (Configuration)",
            "checkoutUseCase: CheckoutUseCase (Service)",
            "clock: Clock (Bean)",
            "fakeGateway: PaymentGateway (Bean)",
            "fakes: Fakes (TestConfiguration)",
            "flatTax: TaxCalculator (Bean)",
            "logNotifier: LogNotifier (Component)",
            "mail: MailNotifier (Component)",
            "noTax: TaxCalculator (Bean)",
            "orderController: OrderController (Controller)",
            "orderRepository: OrderRepository (Repository)",
            "paypalGateway: PaypalGateway (Component)",
            "probeController: ProbeController (RestController)",
            "productController: ProductController (RestController)",
            "productRepository: ProductRepository (Repository)",
            "productService: ProductService (Service)",
            "receipts: ReceiptPrinter (Bean)",
            "shopApplication: ShopApplication (SpringBootApplication)",
            "shopProperties: ShopProperties (ConfigurationProperties)",
            "stripeGateway: StripeGateway (Component)",
            "workers: ExecutorService (Bean)",
        ]
    );
    // Constructor injection without @Autowired, the Spring Data repository, the configuration
    // class, @Primary, @Qualifier, the two profiles.
    let checkout = "CheckoutUseCase.java";
    assert_eq!(
        injected(&m, checkout, 16, "OrderRepository"),
        ["orderRepository"]
    );
    assert_eq!(
        injected(&m, checkout, 16, "ShopProperties"),
        ["shopProperties"]
    );
    assert_eq!(injected(&m, checkout, 16, "Notifier"), ["mail"]);
    assert_eq!(
        injected(&m, "OrderController.java", 32, "PaymentGateway"),
        ["paypalGateway"]
    );
    assert_eq!(
        injected(&m, "ProductService.java", 46, "TaxCalculator"),
        ["flatTax", "noTax"]
    );
    let keys: Vec<(&str, usize)> = m
        .properties
        .iter()
        .filter(|p| p.file.ends_with("application.yml"))
        .map(|p| (p.key.as_str(), p.line + 1))
        .collect();
    assert_eq!(
        keys,
        [
            ("shop.currency", 2),
            ("shop.maxItems", 3),
            ("shop.payment.providers[0].name", 6),
            ("shop.payment.providers[0].url", 7),
            ("shop.payment.providers[1].name", 8),
            ("shop.payment.providers[1].url", 9),
            ("spring.jpa.open-in-view", 12),
            ("spring.config.activate.on-profile", 17),
            ("shop.currency", 19),
        ]
    );
    assert_eq!(
        problems(&m),
        [
            "OrderController.java:27 2 beans of type PaymentGateway (paypalGateway, stripeGateway); mark one @Primary or add @Qualifier",
            "OrderController.java:36 No bean of type AuditLog found",
            "ProductService.java:32 Property 'shop.missing-key' is not defined in the application configuration",
        ]
    );
    // The five `@Autowired` fields of the controller, at the annotation (FW1); constructor
    // injection and the test class are not reported.
    assert_eq!(
        field_injections(&m),
        [
            "OrderController.java:22",
            "OrderController.java:26",
            "OrderController.java:30",
            "OrderController.java:35",
            "OrderController.java:38",
        ]
    );
}

#[test]
fn jakarta_library() {
    let (endpoints, m) = sample("jakarta-library");
    assert_eq!(
        endpoints,
        [
            "GET /api/books BookResource.list BookResource.java:61",
            "POST /api/books BookResource.add BookResource.java:73",
            "DELETE /api/books/{id} BookResource.remove BookResource.java:88",
            "GET /api/books/{id} BookResource.get BookResource.java:66",
            "PUT /api/books/{id} BookResource.replace BookResource.java:81",
            "GET /api/health HealthResource.health HealthResource.java:11",
            "HEAD /api/health/live HealthResource.live HealthResource.java:17",
        ]
    );
    assert_eq!(m.frameworks, ["Jakarta EE"]);
    assert_eq!(
        beans(&m),
        [
            "bookResource: BookResource (Path)",
            "catalog: Catalog (ApplicationScoped)",
            "expressCourier: ExpressCourier (ApplicationScoped)",
            "healthResource: HealthResource (Path)",
            "libraryName: String (Bean)",
            "mail: MailSender (ApplicationScoped)",
            "searchClient: SearchClient (Bean)",
            "searchProducer: SearchProducer (ApplicationScoped)",
            "sms: SmsSender (Stateless)",
            "standardCourier: StandardCourier (ApplicationScoped)",
        ]
    );
    let resource = "BookResource.java";
    assert_eq!(injected(&m, resource, 38, "Courier"), ["expressCourier"]);
    assert_eq!(injected(&m, resource, 42, "Courier"), ["standardCourier"]);
    assert_eq!(injected(&m, resource, 51, "Sender"), ["sms"]);
    assert_eq!(injected(&m, resource, 59, "SearchClient"), ["searchClient"]);
    assert_eq!(
        injected(&m, "Catalog.java", 15, "SearchClient"),
        ["searchClient"]
    );
    assert_eq!(
        problems(&m),
        [
            "BookResource.java:46 2 beans of type Sender (mail, sms); mark one @Alternative @Priority or add @Named",
            "BookResource.java:55 No bean of type Indexer found",
        ]
    );
    // CDI names field injection as a usual form: no recommendation (SPEC-0134 v4 behaviour 6).
    assert!(
        !m.field_injections.is_empty(),
        "the sample injects into fields"
    );
    assert_eq!(field_injections(&m), [""; 0]);
}

#[test]
fn quarkus_greeting() {
    let (endpoints, m) = sample("quarkus-greeting");
    assert_eq!(
        endpoints,
        [
            "GET /hello GreetingResource.hello GreetingResource.java:57",
            "POST /hello/farewell GreetingResource.farewell GreetingResource.java:70",
            "GET /hello/{name} GreetingResource.greet GreetingResource.java:63",
            // The REST client interface, as the reference lists it (EP4).
            "GET /v3.1/name/{name} CountryClient.byName (client) CountryClient.java:13",
        ]
    );
    assert_eq!(m.frameworks, ["Quarkus", "Jakarta EE"]);
    assert_eq!(
        beans(&m),
        [
            "countryClient: CountryClient (RegisterRestClient)",
            "greetingConfig: GreetingConfig (ConfigMapping)",
            "greetingResource: GreetingResource (Path)",
            "greetingService: GreetingService (ApplicationScoped)",
            "lowerFormatter: LowerFormatter (Singleton)",
            "upperFormatter: UpperFormatter (ApplicationScoped)",
        ]
    );
    assert_eq!(
        injected(&m, "GreetingService.java", 11, "UpperFormatter"),
        ["upperFormatter"]
    );
    assert_eq!(
        problems(&m),
        [
            "GreetingResource.java:34 Property 'greeting.missing' is not defined in the application configuration",
            "GreetingResource.java:51 2 beans of type Formatter (lowerFormatter, upperFormatter); mark one @Alternative @Priority or add @Named",
            "GreetingResource.java:55 No bean of type Translator found",
        ]
    );
    // Quarkus documents field injection as a usual form: no recommendation.
    assert!(
        !m.field_injections.is_empty(),
        "the sample injects into fields"
    );
    assert_eq!(field_injections(&m), [""; 0]);
}

#[test]
fn micronaut_notes() {
    let (endpoints, m) = sample("micronaut-notes");
    assert_eq!(
        endpoints,
        [
            // The declarative client: its address is no path.
            "GET /check/{word} SpellClient.check (client) SpellClient.java:10",
            "GET /notes NoteController.list NoteController.java:69",
            "POST /notes NoteController.add NoteController.java:89",
            "GET /notes/count NoteController.count NoteController.java:84",
            "GET /notes/export NoteController.export NoteController.java:79",
            "DELETE /notes/{id} NoteController.remove NoteController.java:106",
            "GET /notes/{id} NoteController.one NoteController.java:74",
            "HEAD /notes/{id} NoteController.exists NoteController.java:110",
            "PATCH /notes/{id} NoteController.change NoteController.java:100",
            "PUT /notes/{id} NoteController.replace NoteController.java:95",
        ]
    );
    assert_eq!(m.frameworks, ["Micronaut"]);
    assert_eq!(
        beans(&m),
        [
            "clock: Clock (Bean)",
            "clockFactory: ClockFactory (Factory)",
            "csvExporter: CsvExporter (Prototype)",
            "json: JsonExporter (Singleton)",
            "noteController: NoteController (Controller)",
            "noteService: NoteService (Singleton)",
            "notesConfiguration: NotesConfiguration (ConfigurationProperties)",
            "spellClient: SpellClient (Client)",
            "stamp: Stamp (Bean)",
        ]
    );
    assert_eq!(
        injected(&m, "NoteController.java", 51, "Exporter"),
        ["json"]
    );
    assert_eq!(
        injected(&m, "NoteController.java", 65, "NoteService"),
        ["noteService"]
    );
    assert_eq!(
        problems(&m),
        [
            "NoteController.java:39 Property 'notes.missing' is not defined in the application configuration",
            "NoteController.java:47 2 beans of type Exporter (csvExporter, json); mark one @Primary or add @Named",
            "NoteController.java:55 No bean of type Archive found",
        ]
    );
    // Micronaut's guide recommends the constructor: every `@Inject` field (SPEC-0134 v4
    // behaviour 6).
    assert_eq!(
        field_injections(&m),
        [
            "NoteController.java:46",
            "NoteController.java:49",
            "NoteController.java:54",
            "NoteController.java:58",
            "NoteController.java:61",
        ]
    );
}

/// The gutter marks of the file named `file` as `line kind: targets` (1-based line).
fn marks(m: &SpringModel, file: &str) -> Vec<String> {
    let rel = m
        .beans
        .iter()
        .map(|b| &b.file)
        .chain(m.injections.iter().map(|i| &i.file))
        .find(|f| f.ends_with(file))
        .unwrap_or_else(|| panic!("no bean or injection point in {file}"));
    m.gutter_marks(rel)
        .into_iter()
        .map(|k| {
            format!(
                "{} {:?}: {}",
                k.line + 1,
                k.kind,
                k.targets
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })
        .collect()
}

/// The key the file named `file` reads as `key`: its defining lines as `file:line` with the
/// profile in brackets, and the value shown.
fn key(m: &SpringModel, file: &str, key: &str) -> (Vec<String>, Option<String>) {
    let v = m
        .values
        .iter()
        .find(|v| v.file.ends_with(file) && v.key == key)
        .unwrap_or_else(|| panic!("{file} does not read {key}"));
    let info = m.key_info_at(&v.file, v.start).unwrap();
    let places = info
        .definitions
        .iter()
        .map(|d| {
            format!(
                "{}:{}{}",
                d.file.file_name().unwrap().to_string_lossy(),
                d.line + 1,
                d.profile
                    .as_ref()
                    .map_or(String::new(), |p| format!(" [{p}]"))
            )
        })
        .collect();
    (places, info.value)
}

fn found(places: &[&str], value: Option<&str>) -> (Vec<String>, Option<String>) {
    (
        places.iter().map(|p| (*p).to_owned()).collect(),
        value.map(str::to_owned),
    )
}

/// SPEC-0115 AK-11 to AK-15 on the sample project: the keys of `ProductService.java` (OBS-0037,
/// run of 2026-11-14) lead to their defining lines, the placeholders show the values of the
/// default profile, the gutter marks lead both ways, and three of the 21 beans are test beans.
#[test]
fn spring_shop_keys_marks_and_test_beans() {
    let (_, m) = sample("spring-shop");
    let service = "ProductService.java";
    assert_eq!(
        key(&m, service, "shop.name"),
        found(&["application.properties:4"], Some("Corner Shop"))
    );
    // Defined twice: a chooser. The profile `prod` is not switched on, so the value is the
    // default profile's (the reference showed the `prod` value, a stated deviation).
    assert_eq!(
        key(&m, service, "shop.currency"),
        found(
            &["application.yml:2", "application.yml:19 [prod]"],
            Some("EUR")
        )
    );
    assert_eq!(
        key(&m, service, "shop.debug-banner"),
        found(&["application-dev.yml:2 [dev]"], None)
    );
    assert_eq!(
        key(&m, service, "shop.max-items"),
        found(&["application.yml:3"], Some("20"))
    );
    assert_eq!(key(&m, service, "shop.missing-key"), found(&[], None));
    assert_eq!(key(&m, service, "shop.timeout"), found(&[], None));
    assert_eq!(key(&m, service, "user.home"), found(&[], None));
    let rel = &m
        .values
        .iter()
        .find(|v| v.file.ends_with(service))
        .unwrap()
        .file;
    let folds: Vec<(usize, String)> = m
        .value_folds(rel)
        .into_iter()
        .map(|f| (f.line + 1, f.text))
        .collect();
    assert_eq!(
        folds,
        [
            (16, "Corner Shop".to_owned()),
            (20, "EUR".to_owned()),
            (28, "20".to_owned())
        ]
    );
    // Back from the configuration file: the field and the prefix of the bound class.
    let name = m.properties.iter().find(|p| p.key == "shop.name").unwrap();
    let mut usages: Vec<String> = m
        .key_info_at(&name.file, name.start)
        .unwrap()
        .usages
        .iter()
        .map(|u| {
            format!(
                "{}:{}",
                u.file.file_name().unwrap().to_string_lossy(),
                u.line + 1
            )
        })
        .collect();
    usages.sort();
    assert_eq!(usages, ["ProductService.java:16", "ShopProperties.java:5"]);
    // The bean leads to where it is injected, the constructor to the beans it gets: one
    // repository, and the two tax calculators of the two profiles (a chooser).
    assert_eq!(
        marks(&m, service),
        [
            "11 Bean: ProductController — ProductController.java:28",
            "46 Injection: productRepository: ProductRepository — ProductRepository.java:9; \
             flatTax: TaxCalculator — AppConfig.java:36; noTax: TaxCalculator — AppConfig.java:42",
        ]
    );
    assert_eq!(
        marks(&m, "OrderController.java"),
        [
            "19 Bean: ",
            "23 Injection: orderRepository: OrderRepository — OrderRepository.java:7",
            "27 Injection: paypalGateway: PaypalGateway — PaypalGateway.java:6; \
             stripeGateway: StripeGateway — StripeGateway.java:6",
            "32 Injection: paypalGateway: PaypalGateway — PaypalGateway.java:6",
            // No bean of type AuditLog: a mark without a target.
            "36 Injection: ",
            "39 Injection: checkoutUseCase: CheckoutUseCase — CheckoutUseCase.java:10",
        ]
    );
    // `@Bean` methods: the clock is injected twice, a method's parameters are injection points.
    assert_eq!(
        marks(&m, "AppConfig.java")[1..5],
        [
            "19 Bean: AppConfig — AppConfig.java:29; CheckoutUseCase — CheckoutUseCase.java:16",
            "24 Bean: ",
            "29 Bean: ",
            "29 Injection: stripeGateway: StripeGateway — StripeGateway.java:6; \
             clock: Clock — AppConfig.java:19",
        ]
    );
    // 21 beans, three of them of the test sources: the application has 18.
    let mut tests: Vec<&str> = m
        .beans
        .iter()
        .filter(|b| b.test)
        .map(|b| b.name.as_str())
        .collect();
    tests.sort();
    assert_eq!(tests, ["fakeGateway", "fakes", "probeController"]);
    assert_eq!(m.beans.iter().filter(|b| !b.test).count(), 18);
}

/// SPEC-0134 AK-11 and AK-12 on the three other sample projects.
#[test]
fn keys_and_marks_of_the_other_samples() {
    let (_, quarkus) = sample("quarkus-greeting");
    let resource = "GreetingResource.java";
    assert_eq!(
        key(&quarkus, resource, "greeting.message"),
        found(&["application.properties:2"], Some("hello"))
    );
    assert_eq!(
        key(&quarkus, resource, "greeting.debug"),
        found(&["application.properties:3 [dev]"], None)
    );
    assert_eq!(
        key(&quarkus, resource, "greeting.missing"),
        found(&[], None)
    );
    assert_eq!(
        marks(&quarkus, resource),
        [
            "15 Bean: ",
            "38 Injection: greetingService: GreetingService — GreetingService.java:6",
            "42 Injection: greetingConfig: GreetingConfig — GreetingConfig.java:7",
            "47 Injection: countryClient: CountryClient — CountryClient.java:11",
            "51 Injection: lowerFormatter: LowerFormatter — LowerFormatter.java:6; \
             upperFormatter: UpperFormatter — UpperFormatter.java:6",
            "55 Injection: ",
        ]
    );
    let (_, micronaut) = sample("micronaut-notes");
    let controller = "NoteController.java";
    assert_eq!(
        key(&micronaut, controller, "notes.page-size"),
        found(&["application.yml:8"], Some("25"))
    );
    assert_eq!(
        key(&micronaut, controller, "notes.owner"),
        found(&["application.yml:9"], Some("me"))
    );
    assert_eq!(
        marks(&micronaut, controller)[..3],
        [
            "22 Bean: ",
            "47 Injection: csvExporter: CsvExporter — CsvExporter.java:7; \
             json: JsonExporter — JsonExporter.java:9",
            "51 Injection: json: JsonExporter — JsonExporter.java:9",
        ]
    );
    let (_, jakarta) = sample("jakarta-library");
    // The producer method of `SearchProducer` is the bean of the last injection point.
    let library = marks(&jakarta, "BookResource.java");
    assert_eq!(
        library[..3],
        [
            "30 Bean: ",
            "33 Injection: catalog: Catalog — Catalog.java:10",
            "38 Injection: expressCourier: ExpressCourier — ExpressCourier.java:7",
        ]
    );
    assert_eq!(
        library.last().map(String::as_str),
        Some("59 Injection: searchClient: SearchClient — SearchProducer.java:12")
    );
    let producer = marks(&jakarta, "SearchProducer.java");
    assert!(
        producer.contains(
            &"12 Bean: Catalog — Catalog.java:15; BookResource — BookResource.java:59".to_owned()
        ),
        "{producer:?}"
    );
}
