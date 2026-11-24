//! The text of JVM values (`SPEC-0091` v16 behaviour 42, `SPEC-0132` v4 behaviour 4a): one form
//! `Type@id` for objects of java-debug and kotlin-debug-adapter, the size of collections after
//! it, the value of boxed numbers, and no `this` row in a frame without a receiver. Everything
//! here works on texts the adapters sent; the session reads fields for the sizes
//! kotlin-debug-adapter does not give, and never evaluates for them.

use std::ops::Range;

use serde_json::Value;

use crate::Variable;

/// kotlin-debug-adapter's type text for a value whose type it does not know (`null`).
const UNKNOWN_TYPE: &str = "Unknown type";
/// How kotlin-debug-adapter starts the text of an object.
const INSTANCE: &str = "instance of ";
/// Packages whose collection classes get a size read from their fields.
const LIBRARY: [&str; 2] = ["java.util.", "kotlin.collections."];
/// Classes that are always empty.
const EMPTY: [&str; 6] = [
    "kotlin.collections.EmptyList",
    "kotlin.collections.EmptySet",
    "kotlin.collections.EmptyMap",
    "java.util.Collections$EmptyList",
    "java.util.Collections$EmptySet",
    "java.util.Collections$EmptyMap",
];
/// Classes that always hold one element.
const SINGLE: [&str; 3] = [
    "java.util.Collections$SingletonList",
    "java.util.Collections$SingletonSet",
    "java.util.Collections$SingletonMap",
];
/// Classes whose one field `value` is what the user means.
const BOXED: [&str; 8] = [
    "java.lang.Integer",
    "java.lang.Long",
    "java.lang.Short",
    "java.lang.Byte",
    "java.lang.Double",
    "java.lang.Float",
    "java.lang.Boolean",
    "java.lang.Character",
];

/// Whether `v` is the `this` an adapter sends for a frame without a receiver (a Kotlin top-level
/// function, a static method): `null`, no children, no known type.
pub fn receiverless_this(v: &Variable) -> bool {
    v.name == "this"
        && v.value == "null"
        && v.reference == 0
        && v.kind.as_deref().is_none_or(|k| k == UNKNOWN_TYPE)
}

/// The type text to show: kotlin-debug-adapter's "Unknown type" is none.
pub fn known_type(kind: Option<String>) -> Option<String> {
    kind.filter(|k| k != UNKNOWN_TYPE)
}

/// The child counts (`indexedVariables` plus `namedVariables`) of the variables of a `variables`
/// answer, in their order; of an `evaluate` answer when `body` has no variables.
pub fn counts(body: &Value) -> Vec<u64> {
    let count = |v: &Value| {
        v["indexedVariables"].as_u64().unwrap_or(0) + v["namedVariables"].as_u64().unwrap_or(0)
    };
    match body["variables"].as_array() {
        Some(vars) => vars.iter().map(count).collect(),
        None => vec![count(body)],
    }
}

/// An object as kotlin-debug-adapter writes it: `instance of java.util.ArrayList(id=566)`,
/// `instance of int[3] (id=570)`, `instance of java.lang.Class(reflected class=int, id=588)`.
struct Instance<'a> {
    /// The class with its package, an array with its length: `java.util.ArrayList`, `int[3]`.
    class: &'a str,
    /// The debugger's object id.
    id: &'a str,
}

impl<'a> Instance<'a> {
    fn parse(value: &'a str) -> Option<Self> {
        let rest = value.strip_prefix(INSTANCE)?.strip_suffix(')')?;
        let open = rest.rfind('(')?;
        let id = rest[open + 1..].rsplit("id=").next()?;
        let class = rest[..open].trim_end();
        (!class.is_empty() && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
            .then_some(Self { class, id })
    }

    /// The class without its package; a nested class keeps its `$`, an array its brackets.
    fn simple(&self) -> &'a str {
        let name_end = self.class.find('[').unwrap_or(self.class.len());
        match self.class[..name_end].rfind('.') {
            Some(dot) => &self.class[dot + 1..],
            None => self.class,
        }
    }

    fn array_length(&self) -> Option<u64> {
        let open = self.class.rfind('[')?;
        self.class[open + 1..].strip_suffix(']')?.parse().ok()
    }
}

/// The value in Ferris IDEA's form, from what one answer says without further requests:
/// kotlin-debug-adapter's objects as `Type@id`, with the size of the classes that are always
/// empty or hold one element; java-debug's ` size=N` as ` size = N`; else the adapter's child
/// count `count` as the size when it is above zero and the value is an object, not an array.
/// Other values (numbers, strings, `null`, texts of other forms) are returned as they are.
pub fn present(value: &str, count: u64) -> String {
    if let Some(instance) = Instance::parse(value) {
        let mut text = format!("{}@{}", instance.simple(), instance.id);
        let fixed = if EMPTY.contains(&instance.class) {
            Some(0)
        } else if SINGLE.contains(&instance.class) {
            Some(1)
        } else {
            (count > 0 && instance.array_length().is_none()).then_some(count)
        };
        if let Some(n) = fixed {
            text.push_str(&size_text(n));
        }
        return text;
    }
    let Some(id) = id_range(value) else {
        return value.to_owned();
    };
    let (object, rest) = value.split_at(id.end);
    // java-debug: `ArrayList@13 size=1`.
    if let Some(n) = rest.strip_prefix(" size=") {
        let digits = n.bytes().take_while(u8::is_ascii_digit).count();
        if let Ok(size) = n[..digits].parse::<u64>() {
            return format!("{object}{}{}", size_text(size), &n[digits..]);
        }
    }
    if rest.is_empty() && count > 0 && !object.contains('[') {
        return format!("{object}{}", size_text(count));
    }
    value.to_owned()
}

/// ` size = N`, the summary after a collection's type and id.
pub fn size_text(size: u64) -> String {
    format!(" size = {size}")
}

/// What the fields of an object are read for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// The number of elements of a collection or map of `java.util` or `kotlin.collections`.
    Size,
    /// The value of a boxed number, boolean or character.
    Boxed,
}

/// Whether the fields of the object behind the adapter's text `value` have to be read for its
/// summary: only for kotlin-debug-adapter's objects, only boxed values and classes of the two
/// library packages, not for arrays and not for the classes [`present`] knows the size of.
pub fn needs(value: &str) -> Option<Need> {
    let instance = Instance::parse(value)?;
    if BOXED.contains(&instance.class) {
        return Some(Need::Boxed);
    }
    let library = LIBRARY.iter().any(|p| instance.class.starts_with(p));
    let known = EMPTY.contains(&instance.class) || SINGLE.contains(&instance.class);
    (library && !known && instance.array_length().is_none()).then_some(Need::Size)
}

/// What an object's fields say about its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// This many elements.
    Known(u64),
    /// A set kept in the map behind this reference: the map's fields have the size.
    InMap(i64),
    /// The fields do not say.
    Unknown,
}

/// The size of a collection from its `fields` as the adapter sent them (not yet in Ferris IDEA's
/// form): a field `size`; else the length of the array in a field `a`; else `head`, `tail` and
/// the length of `elements` (a ring buffer); else the `java.util` map in a field `map` or `m`.
pub fn size_of_fields(fields: &[Variable]) -> Size {
    let field = |name: &str| fields.iter().find(|f| f.name == name);
    let number = |name: &str| field(name).and_then(|f| f.value.parse::<u64>().ok());
    let length = |name: &str| {
        field(name).and_then(|f| Instance::parse(&f.value).and_then(|i| i.array_length()))
    };
    if let Some(size) = number("size") {
        return Size::Known(size);
    }
    if let Some(size) = length("a") {
        return Size::Known(size);
    }
    if let (Some(head), Some(tail), Some(room)) =
        (number("head"), number("tail"), length("elements"))
        && room > 0
    {
        return Size::Known((tail + room - head % room) % room);
    }
    ["map", "m"]
        .iter()
        .filter_map(|name| field(name))
        .find(|f| {
            f.reference != 0
                && Instance::parse(&f.value).is_some_and(|i| i.class.starts_with("java.util."))
        })
        .map_or(Size::Unknown, |f| Size::InMap(f.reference))
}

/// The value of a boxed number, boolean or character from its `fields`: the field `value`, when
/// it is a plain value.
pub fn boxed_of_fields(fields: &[Variable]) -> Option<String> {
    fields
        .iter()
        .find(|f| f.name == "value" && f.reference == 0 && !f.value.starts_with(INSTANCE))
        .map(|f| f.value.clone())
        .filter(|v| !v.is_empty())
}

/// The bytes of the `@id` part of a value in the form `Type@id…`, for drawing it dimmed; `None`
/// for values without one (numbers, strings, `null`).
pub fn id_range(value: &str) -> Option<Range<usize>> {
    let object = value.split(" \"").next().unwrap_or(value);
    if object.starts_with(['"', '\'']) {
        return None;
    }
    let at = object.rfind('@')?;
    let digits = object[at + 1..]
        .bytes()
        .take_while(u8::is_ascii_hexdigit)
        .count();
    let class = &object[..at];
    (digits > 0 && !class.is_empty() && !class.contains(' ')).then_some(at..at + 1 + digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(name: &str, value: &str, kind: Option<&str>, reference: i64) -> Variable {
        Variable {
            name: name.into(),
            value: value.into(),
            kind: kind.map(str::to_owned),
            reference,
            evaluate_name: None,
        }
    }

    #[test]
    fn this_without_a_receiver_is_left_out() {
        // kotlin-debug-adapter in a top-level `main`.
        assert!(receiverless_this(&var(
            "this",
            "null",
            Some("Unknown type"),
            0
        )));
        assert!(receiverless_this(&var("this", "null", None, 0)));
        // A member function's object stays.
        assert!(!receiverless_this(&var(
            "this",
            "instance of demo.Dog(id=568)",
            Some("demo.Dog"),
            568
        )));
        // Another local that is null stays, without the adapter's placeholder type.
        let nothing = var("nothing", "null", Some("Unknown type"), 0);
        assert!(!receiverless_this(&nothing));
        assert_eq!(known_type(nothing.kind), None);
        assert_eq!(
            known_type(Some("demo.Dog".into())).as_deref(),
            Some("demo.Dog")
        );
    }

    #[test]
    fn kotlin_objects_read_type_and_id() {
        assert_eq!(present("instance of demo.Dog(id=568)", 0), "Dog@568");
        assert_eq!(present("instance of int[3] (id=570)", 0), "int[3]@570");
        assert_eq!(
            present("instance of java.lang.String[2] (id=576)", 0),
            "String[2]@576"
        );
        assert_eq!(
            present("instance of java.util.HashMap$Node[4] (id=587)", 0),
            "HashMap$Node[4]@587"
        );
        assert_eq!(
            present(
                "instance of java.lang.Class(reflected class=int, id=588)",
                0
            ),
            "Class@588"
        );
        // Not this form: left alone.
        for other in [
            "\"Rex\"",
            "42",
            "null",
            "instance of",
            "<Foo object at 0x10>",
        ] {
            assert_eq!(present(other, 0), other);
        }
        assert_eq!(id_range("Dog@568"), Some(3..7));
        assert_eq!(id_range("ArrayList@566 size = 1"), Some(9..13));
        assert_eq!(id_range("Item@1f \"a@b\""), Some(4..7));
        assert_eq!(id_range("\"a@1\""), None);
        assert_eq!(id_range("42"), None);
    }

    #[test]
    fn sizes_from_the_adapters_text_and_counts() {
        // java-debug writes the size itself.
        assert_eq!(present("ArrayList@13 size=1", 1), "ArrayList@13 size = 1");
        assert_eq!(present("HashMap@20 size=0", 0), "HashMap@20 size = 0");
        // Only a count: the size of an object, never of an array.
        assert_eq!(present("Stack@7", 3), "Stack@7 size = 3");
        assert_eq!(present("String[2]@12", 2), "String[2]@12");
        assert_eq!(present("Hot$Counter@14", 0), "Hot$Counter@14");
        // An object's own text is not searched for a size.
        let text = "Item@4 \"Item[ size=3]\"";
        assert_eq!(present(text, 0), text);
        assert_eq!(
            counts(&serde_json::json!({"variables": [
                {"name": "a", "indexedVariables": 2, "namedVariables": 1},
                {"name": "b"},
            ]})),
            [3, 0]
        );
        assert_eq!(
            counts(&serde_json::json!({"result": "x", "indexedVariables": 4})),
            [4]
        );
    }

    #[test]
    fn sizes_from_fields_only_for_library_collections() {
        assert_eq!(
            needs("instance of java.util.ArrayList(id=566)"),
            Some(Need::Size)
        );
        assert_eq!(
            needs("instance of kotlin.collections.ArrayDeque(id=5)"),
            Some(Need::Size)
        );
        // The program's own classes, arrays and plain values are never asked.
        for none in [
            "instance of demo.Dog(id=568)",
            "instance of demo.util.Bag(id=9)",
            "instance of int[3] (id=570)",
            "instance of java.lang.String[2] (id=576)",
            "ArrayList@13 size=1",
            "\"Rex\"",
            "null",
        ] {
            assert_eq!(needs(none), None, "{none}");
        }
        // Known without asking.
        let empty = "instance of kotlin.collections.EmptyList(id=573)";
        assert_eq!(needs(empty), None);
        assert_eq!(present(empty, 0), "EmptyList@573 size = 0");
        let one = "instance of java.util.Collections$SingletonList(id=577)";
        assert_eq!(needs(one), None);
        assert_eq!(present(one, 0), "Collections$SingletonList@577 size = 1");

        let list = [
            var("DEFAULT_CAPACITY", "10", None, 0),
            var(
                "elementData",
                "instance of java.lang.Object[10] (id=581)",
                None,
                581,
            ),
            var("size", "1", None, 0),
        ];
        assert_eq!(size_of_fields(&list), Size::Known(1));
        let as_list = [var(
            "a",
            "instance of java.lang.Integer[2] (id=595)",
            None,
            595,
        )];
        assert_eq!(size_of_fields(&as_list), Size::Known(2));
        let deque = |head: &str, tail: &str| {
            [
                var(
                    "elements",
                    "instance of java.lang.Object[17] (id=596)",
                    None,
                    596,
                ),
                var("head", head, None, 0),
                var("tail", tail, None, 0),
            ]
        };
        assert_eq!(size_of_fields(&deque("0", "0")), Size::Known(0));
        assert_eq!(size_of_fields(&deque("2", "5")), Size::Known(3));
        assert_eq!(size_of_fields(&deque("15", "1")), Size::Known(3));
        let set = [
            var(
                "map",
                "instance of java.util.LinkedHashMap(id=593)",
                None,
                593,
            ),
            var("PRESENT", "instance of java.lang.Object(id=594)", None, 0),
        ];
        assert_eq!(size_of_fields(&set), Size::InMap(593));
        // A field `map` of another kind says nothing.
        let other = [var("map", "instance of demo.Atlas(id=3)", None, 3)];
        assert_eq!(size_of_fields(&other), Size::Unknown);
        assert_eq!(size_of_fields(&[]), Size::Unknown);
    }

    #[test]
    fn boxed_values_show_the_value() {
        assert_eq!(
            needs("instance of java.lang.Integer(id=571)"),
            Some(Need::Boxed)
        );
        assert_eq!(
            needs("instance of java.lang.Boolean(id=3)"),
            Some(Need::Boxed)
        );
        assert_eq!(needs("instance of java.lang.Object(id=3)"), None);
        let fields = [
            var("MIN_VALUE", "-2147483648", None, 0),
            var("digits", "instance of char[36] (id=589)", None, 589),
            var("value", "300", None, 0),
        ];
        assert_eq!(boxed_of_fields(&fields).as_deref(), Some("300"));
        assert_eq!(boxed_of_fields(&fields[..2]), None);
    }
}
