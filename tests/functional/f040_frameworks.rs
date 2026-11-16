//! F-040 · Spring, Jakarta EE, Micronaut, Quarkus: bean navigation, configuration help (SPEC-0115,
//! SPEC-0124).

use crate::harness::*;
use serde_json::json;

#[test]
fn spring_beans_and_injection_points() {
    let p = Project::reference("spring-basic");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("spring_beans", json!({}));
    assert_eq!(r["framework"], "Spring");
    let names: Vec<String> = r["model"]["beans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| text(&b["name"]))
        .collect();
    assert!(names.contains(&"stockController".into()), "{r}");
    assert!(names.contains(&"stockRepository".into()), "{r}");
}

#[test]
fn rest_endpoints_are_listed_with_a_request() {
    let p = Project::reference("spring-basic");
    let mut ide = Ide::stdio(&p);
    let r = ide.ok("endpoints", json!({}));
    let e = &r[0];
    assert_eq!(e["method"], "GET");
    assert_eq!(e["path"], "/stock/value");
    assert_eq!(e["handler"], "StockController.stockValue");
    // The port comes from application.properties.
    assert!(text(&e["request"]).contains(":8089/stock/value"), "{r}");
}
