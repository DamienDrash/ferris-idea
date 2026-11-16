//! jdtls in the features every server is checked for (`it/servers.rs`), through the language
//! server pool: nine questions to one jdtls instead of one jdtls per question.

use crate::server_case::{Case, check};

#[tokio::test]
async fn jdtls_java() {
    crate::pooled();
    check(&Case {
        name: "java",
        server: "jdtls",
        files: &[],
        file: "Shapes.java",
        text: "public class Shapes {
    /** Adds two numbers. */
    static int add(int a, int b) {
        return a + b;
    }

    static class Square {
        final int side;
        Square(int side) { this.side = side; }
        int area() { return side * side; }
    }

    public static void main(String[] args) {
        Square sq = new Square(2);
        int total = add(sq.area(), 1);
        System.out.println(total);
    }
}
",
        call: (14, 20),
        definition: (2, 15),
        second_argument: (14, 35),
        parameter: "int b",
        variable: (13, 15),
        type_line: Some(6),
        doc: "Adds two numbers",
        function: "add",
        typing: ("int total = add(sq.area(), 1);", "int total = ad", 14, 22),
        top_symbols: &["Shapes"],
        calls: true,
        rename_to: Some("plus"),
    })
    .await;
}
