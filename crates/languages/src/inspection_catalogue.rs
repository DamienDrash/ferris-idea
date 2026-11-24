//! Language, default level and description of every own inspection (`SPEC-0098` v3 behaviour 6,
//! `SPEC-0315` behaviour 4): the data behind `inspections::description` and
//! `inspections::catalogue`. The descriptions are Ferris IDEA's own words.

use crate::inspections::Level;

/// Id, language group, default level and description of one inspection.
pub(crate) struct Meta {
    pub id: &'static str,
    pub language: &'static str,
    pub level: Level,
    pub description: &'static str,
}

const JAVA: &str = "Java";
const KOTLIN: &str = "Kotlin";
const JS: &str = "JavaScript and TypeScript";
const PYTHON: &str = "Python";
const PROPERTIES: &str = "Properties";
const PROSE: &str = "Natural language";
const GENERAL: &str = "General";

const fn meta(
    id: &'static str,
    language: &'static str,
    level: Level,
    description: &'static str,
) -> Meta {
    Meta {
        id,
        language,
        level,
        description,
    }
}

/// Every inspection `inspect` and `inspect_text` can report.
pub(crate) const META: &[Meta] = &[
    meta(
        "java.empty-catch",
        JAVA,
        Level::Warning,
        "Reports a 'catch' block that contains nothing, not even a comment. An exception that is \
         caught and dropped hides failures and makes bugs hard to find; handle it, log it or say \
         in a comment why it can be ignored.\n\nReported:\n    try { load(); } catch (IOException e) { }",
    ),
    meta(
        "kotlin.empty-catch",
        KOTLIN,
        Level::Warning,
        "Reports a 'catch' block that contains nothing, not even a comment. Silently swallowing an \
         exception hides failures; handle it, log it or explain in a comment why it is safe to \
         ignore.\n\nReported:\n    try { load() } catch (e: IOException) { }",
    ),
    meta(
        "python.bare-except",
        PYTHON,
        Level::Weak,
        "Reports an 'except:' clause without an exception type. Such a clause also catches \
         SystemExit and KeyboardInterrupt, so a program may refuse to stop; name the exceptions \
         you expect, or at least 'Exception'.\n\nReported:\n    try:\n        run()\n    except:\n        pass",
    ),
    meta(
        "python.shadows-name",
        PYTHON,
        Level::Weak,
        "Reports a parameter or local variable of a function that has the name of a variable, \
         function, class or import of the module or of an enclosing function. Inside the function \
         the outer name can no longer be read, and a reader may take one for the other; a later \
         edit that removes the local name silently starts using the outer one. Rename one of the \
         two. Names starting with an underscore, 'self', 'cls' and names declared 'global' or \
         'nonlocal' are not reported.\n\nReported:\n    def describe(item):\n        return item.name\n\n    item = load()",
    ),
    meta(
        "python.method-could-be-static",
        PYTHON,
        Level::Weak,
        "Reports a method of a class that never uses 'self'. Such a method does not depend on the \
         instance: it can be a '@staticmethod' or a function of the module, and a reader then \
         sees at once that it touches no state. Methods with a decorator, special methods such \
         as '__len__', placeholder bodies ('pass', '...', a docstring, 'raise \
         NotImplementedError'), methods of classes with a base class or metaclass and of test \
         classes ('Test...') are not reported, because they may be required to take the \
         instance. The quick fix 'Make method static' adds '@staticmethod' and removes 'self'; \
         calls through an instance keep working.\n\nReported:\n    class Shape:\n        def area(self):\n            return 0",
    ),
    meta(
        "python.unused-import",
        PYTHON,
        Level::Info,
        "Finds an import of a Python module whose name nothing in the file reads, also not \
         '__all__', a string annotation or a '# type:' comment. pyright marks the import as not \
         accessed; this inspection gives the place its quick fixes: 'Remove unused import' removes \
         the name, or the statement when none is left, by the rules of Optimize Imports without \
         ruff (imports in a block or with a comment stay), and 'Optimize imports' runs Optimize \
         Imports. By default it shows nothing and only offers the fixes; a package's \
         '__init__.py' and files with 'import *', 'globals()' or 'exec' are left out.\n\nReported:\n    import os\n\n    def f():\n        return 1",
    ),
    meta(
        "python.unused-variable",
        PYTHON,
        Level::Info,
        "Finds 'name = expression' in a function whose name the function never reads, also not \
         in a nested function or by '+='. 'Remove assignment target' keeps the expression as a \
         statement, so a call in it still runs; 'Remove statement' is offered only for a value \
         that cannot do anything (literals, containers of them, a lambda) and removes the line, \
         or leaves 'pass' in an otherwise empty block. Names starting with '_', parameters, \
         'global' and 'nonlocal' names and functions that use 'locals()' are left out. By \
         default it shows nothing and only offers the fixes.\n\nReported:\n    def f():\n        unused = 3\n        return 1",
    ),
    meta(
        "python.unresolved-name",
        PYTHON,
        Level::Info,
        "Finds a name that a function reads and no scope defines: not the function, an \
         enclosing function or class, the module (also further down) and not Python's built-in \
         names. 'Import' adds a 'from module import name' for each module of the project that \
         defines the name at its top level and each standard library module that offers it, or \
         'import name' for a standard library module. 'Create parameter' appends it to the function's parameters when they are plain \
         names only; callers are not changed, so a call that does not pass it fails until it \
         does. With a default value, '*', '/', '*args' or '**kwargs' no parameter is created, \
         because the callers' arguments would move. By default it shows nothing and only offers \
         the fix; pyright reports the name.\n\nReported:\n    def total(price):\n        return price * rate",
    ),
    meta(
        "js.coercing-equality",
        JS,
        Level::Weak,
        "Reports '==' and '!=' in JavaScript and TypeScript, except comparisons with null or \
         undefined. These operators convert the operands before comparing, so '0 == \"\"' is true; \
         the strict operators '===' and '!==' compare without conversion.\n\nReported:\n    if (count == input) { }",
    ),
    meta(
        "any.conflict-marker",
        GENERAL,
        Level::Warning,
        "Reports the marker lines Git leaves in a file when a merge could not be completed. A file \
         with markers usually does not compile and still holds both versions of the changed \
         lines; resolve the conflict and remove the markers. Only files with an opening marker \
         line are checked, so a Markdown heading underline is not reported.\n\nReported:\n    <<<<<<< HEAD\n    size = 1\n    =======\n    size = 2\n    >>>>>>> topic",
    ),
    meta(
        "java.unused-import",
        JAVA,
        Level::Weak,
        "Reports a single-type or static import whose name is used nowhere in the file, also not \
         in a Javadoc link. Unused imports clutter the file and can hide which types a class \
         really depends on. Wildcard imports are left alone.\n\nReported:\n    import java.util.Map;\nAfter the quick fix the line is removed.",
    ),
    meta(
        "java.unused-local",
        JAVA,
        Level::Warning,
        "Reports a local variable whose value is never read later in its block. It is either a \
         leftover or a sign that the code meant to use it. The quick fix removes the declaration \
         only when nothing with a side effect would disappear with it.\n\nReported:\n    int unused = 1;\nAfter the quick fix the declaration is removed.",
    ),
    meta(
        "java.unused-private",
        JAVA,
        Level::Warning,
        "Reports a private field or method that nothing in the file uses. Private members cannot \
         be reached from other files, so such a member is dead code. Annotated members and the \
         serialization methods count as used.\n\nReported:\n    private void helper() { }",
    ),
    meta(
        "java.unused-parameter",
        JAVA,
        Level::Weak,
        "Reports a parameter of a private method or constructor that its body never mentions. The \
         callers pass a value that is thrown away, which often points to a forgotten use. There is \
         no quick fix, since removing the parameter changes every call.\n\nReported:\n    private int twice(int x, int unused) { return 2 * x; }",
    ),
    meta(
        "java.boolean-literal-comparison",
        JAVA,
        Level::Weak,
        "Reports a comparison of a boolean expression with 'true' or 'false'. The comparison adds \
         nothing and reads worse than the expression itself or its negation.\n\nReported:\n    if (ready == true) { }\nAfter the quick fix:\n    if (ready) { }",
    ),
    meta(
        "java.size-zero",
        JAVA,
        Level::Weak,
        "Reports 'size()' or 'length()' compared with zero where the receiver is declared in the \
         file as a String or a collection or map type. 'isEmpty()' says the same more directly and \
         is never slower.\n\nReported:\n    if (names.size() == 0) { }\nAfter the quick fix:\n    if (names.isEmpty()) { }",
    ),
    meta(
        "java.self-assignment",
        JAVA,
        Level::Warning,
        "Reports a variable or field assigned to itself. The statement has no effect and usually \
         means another value was meant, for example a parameter of the same name.\n\nReported:\n    this.name = this.name;\nAfter the quick fix the statement is deleted.",
    ),
    meta(
        "java.empty-if",
        JAVA,
        Level::Warning,
        "Reports an 'if' statement whose body is a lone semicolon. The semicolon ends the \
         statement, so the following block runs whatever the condition is.\n\nReported:\n    if (done);\n    { finish(); }",
    ),
    meta(
        "java.constant-condition",
        JAVA,
        Level::Warning,
        "Reports 'if (true)', 'if (false)' and 'while (false)'. A condition that never changes \
         makes a branch dead or always taken, often after debugging; 'while (true)' as an endless \
         loop is fine.\n\nReported:\n    if (false) { log(); }",
    ),
    meta(
        "java.duplicate-condition",
        JAVA,
        Level::Warning,
        "Reports a condition that occurs twice in one chain of 'if' and 'else if'. The second \
         branch can never run, so one of the conditions is probably wrong.\n\nReported:\n    if (a) { x(); } else if (b) { y(); } else if (a) { z(); }",
    ),
    meta(
        "java.print-stack-trace",
        JAVA,
        Level::Weak,
        "Reports 'printStackTrace()' without arguments. The trace goes to standard error instead \
         of the application's log, where it is easily lost; log the exception instead.\n\nReported:\n    catch (IOException e) { e.printStackTrace(); }",
    ),
    meta(
        "java.boxing-constructor",
        JAVA,
        Level::Warning,
        "Reports 'new Integer(...)' and the constructors of the other seven boxed types. They are \
         deprecated since Java 9 and always create a new object, while 'valueOf' can reuse cached \
         ones.\n\nReported:\n    Integer i = new Integer(5);\nAfter the quick fix:\n    Integer i = Integer.valueOf(5);",
    ),
    meta(
        "java.empty-block",
        JAVA,
        Level::Warning,
        "Reports an empty 'finally' block or an empty 'synchronized' block without a comment. An \
         empty 'finally' does nothing, and an empty 'synchronized' block is rarely what was \
         intended.\n\nReported:\n    try { run(); } finally { }",
    ),
    meta(
        "java.equals-hash-code",
        JAVA,
        Level::Warning,
        "Reports a class that declares 'equals(Object)' without 'hashCode()', or the other way \
         round. Equal objects must have equal hash codes, otherwise hash maps and sets lose or \
         duplicate them.\n\nReported:\n    class Point { public boolean equals(Object o) { ... } }",
    ),
    meta(
        "java.string-equality",
        JAVA,
        Level::Warning,
        "Reports '==' or '!=' between strings. These operators compare object identity, so two \
         strings with the same characters can still be unequal; compare contents with 'equals'.\n\nReported:\n    if (name == \"admin\") { }\nAfter the quick fix:\n    if (\"admin\".equals(name)) { }",
    ),
    meta(
        "java.boxed-equality",
        JAVA,
        Level::Warning,
        "Reports '==' or '!=' between two boxed values of the same type, such as two Integer \
         variables. The operators compare references, which only happens to work for small cached \
         values.\n\nReported:\n    if (a == b) { }   // Integer a, b\nAfter the quick fix:\n    if (Objects.equals(a, b)) { }",
    ),
    meta(
        "java.ignored-result",
        JAVA,
        Level::Warning,
        "Reports a call on an immutable value whose result is dropped, such as 's.trim();'. The \
         value itself does not change, so the call has no effect.\n\nReported:\n    s.trim();\nAfter the quick fix:\n    s = s.trim();",
    ),
    meta(
        "java.equals-incompatible",
        JAVA,
        Level::Warning,
        "Reports 'equals()' between two value types that can never be equal, such as a String and \
         an Integer. The result is always false, which is almost certainly a mistake.\n\nReported:\n    if (text.equals(count)) { }",
    ),
    meta(
        "java.array-to-string",
        JAVA,
        Level::Warning,
        "Reports an array that is printed, concatenated or turned into a string directly. That \
         gives its type and hash code, not its elements; 'Arrays.toString' shows the contents.\n\nReported:\n    System.out.println(values);\nAfter the quick fix:\n    System.out.println(Arrays.toString(values));",
    ),
    meta(
        "java.array-equals",
        JAVA,
        Level::Warning,
        "Reports 'equals()' called on an array. Arrays inherit identity comparison, so two arrays \
         with the same elements are not equal; 'Arrays.equals' compares the contents.\n\nReported:\n    if (a.equals(b)) { }\nAfter the quick fix:\n    if (Arrays.equals(a, b)) { }",
    ),
    meta(
        "java.integer-division",
        JAVA,
        Level::Warning,
        "Reports a division of two integers whose result is used as a floating-point value. The \
         fraction is cut off before the conversion, so 'double d = 1 / 2' is 0.\n\nReported:\n    double avg = sum / n;\nAfter the quick fix:\n    double avg = (double) sum / n;",
    ),
    meta(
        "java.redundant-cast",
        JAVA,
        Level::Warning,
        "Reports a cast of a variable to exactly the type it is declared with in the file. The \
         cast changes nothing and only adds noise.\n\nReported:\n    String s = ...;\n    use((String) s);\nAfter the quick fix:\n    use(s);",
    ),
    meta(
        "java.field-may-be-final",
        JAVA,
        Level::Warning,
        "Reports a private field with an initializer that is never assigned anywhere else in the \
         file. Making it final documents that it does not change and lets the compiler keep it \
         that way.\n\nReported:\n    private int limit = 10;\nAfter the quick fix:\n    private final int limit = 10;",
    ),
    meta(
        "java.null-dereference",
        JAVA,
        Level::Warning,
        "Reports a method call or field access on a local variable that is still null: it was \
         initialised with null (or with a conditional that can be null) and nothing assigned or \
         checked it before. The call throws a NullPointerException at run time.\n\nReported:\n    String s = null;\n    int n = s.length();",
    ),
    meta(
        "kotlin.ignored-result",
        KOTLIN,
        Level::Warning,
        "Reports a call on an immutable value whose result is dropped, such as 's.trim()' as a \
         statement. The value does not change, so the call has no effect.\n\nReported:\n    s.trim()\nAfter the quick fix:\n    s = s.trim()",
    ),
    meta(
        "kotlin.identity-equality",
        KOTLIN,
        Level::Warning,
        "Reports '===' or '!==' between values such as strings or numbers. Identity says whether \
         two references point to the same object, not whether the values are equal; use '==' and \
         '!='.\n\nReported:\n    if (a === b) { }\nAfter the quick fix:\n    if (a == b) { }",
    ),
    meta(
        "kotlin.array-equality",
        KOTLIN,
        Level::Warning,
        "Reports '==' or 'equals()' between arrays. Arrays compare by identity, so two arrays with \
         the same elements are not equal; 'contentEquals' compares the elements.\n\nReported:\n    if (xs == ys) { }\nAfter the quick fix:\n    if (xs.contentEquals(ys)) { }",
    ),
    meta(
        "kotlin.array-to-string",
        KOTLIN,
        Level::Warning,
        "Reports an array that is printed, put into a string template or converted with \
         'toString()'. That shows its type and hash code, not its elements; 'contentToString()' \
         shows the contents.\n\nReported:\n    println(xs)\nAfter the quick fix:\n    println(xs.contentToString())",
    ),
    meta(
        "kotlin.equals-incompatible",
        KOTLIN,
        Level::Warning,
        "Reports 'equals()' between two value types that can never be equal, such as a String and \
         an Int. The result is always false.\n\nReported:\n    if (name.equals(count)) { }",
    ),
    meta(
        "kotlin.integer-division",
        KOTLIN,
        Level::Warning,
        "Reports an integer division that is converted to a floating-point number afterwards. The \
         fraction is already lost when the conversion happens; convert an operand first.\n\nReported:\n    val avg = (sum / n).toDouble()\nAfter the quick fix:\n    val avg = sum.toDouble() / n",
    ),
    meta(
        "kotlin.unused-private",
        KOTLIN,
        Level::Weak,
        "Reports a private class, function or property whose name appears nowhere else in the \
         file. Private declarations cannot be reached from other files, so such a declaration is \
         dead code. Annotated declarations, declarations inside an annotated class, overrides, \
         operators and 'main' are not reported.\n\nReported:\n    private fun helper() { }",
    ),
    meta(
        "kotlin.unused-declaration",
        KOTLIN,
        Level::Warning,
        "Reports Kotlin classes, objects, functions and properties that are not private and that \
         nothing in the project uses, counted over Java and Kotlin files with the project-wide \
         reference index; a property also counts as used through its Java getter and setter. \
         Entry points such as main functions, annotated declarations, overrides, operator \
         functions and the properties of data classes count as used; remove unused code or keep \
         it with @Suppress(\"unused\").\n\nReported:\n    class Helper {\n        fun neverCalled() { }\n    }",
    ),
    meta(
        "kotlin.unused-parameter",
        KOTLIN,
        Level::Weak,
        "Reports a parameter of a private function that its body never mentions. The callers \
         pass a value that is thrown away, which often points to a forgotten use. Functions \
         passed as a reference ('::name') keep their parameters.\n\nReported:\n    private fun area(width: Int, height: Int) = width * width",
    ),
    meta(
        "grazie.repeated-word",
        PROSE,
        Level::Weak,
        "Reports the same word written twice in a row in comments, Markdown or text files. Such \
         doubles are almost always typing slips.\n\nReported:\n    // the the value\nAfter the quick fix:\n    // the value",
    ),
    meta(
        "grazie.article",
        PROSE,
        Level::Weak,
        "Reports 'a' before a word that starts with a vowel sound, or 'an' before a consonant \
         sound. The wrong article reads like a typo.\n\nReported:\n    // a error occurred\nAfter the quick fix:\n    // an error occurred",
    ),
    meta(
        "grazie.typo",
        PROSE,
        Level::Weak,
        "Reports words in comments, Markdown and text files that are not in the dictionary, with \
         suggestions. Words of the project dictionary and names used in the code are not \
         reported; a word can be saved to the project dictionary from the quick fixes.\n\nReported:\n    // recieve the data\nAfter the quick fix:\n    // receive the data",
    ),
    meta(
        crate::spelling_variants::OTHER_VARIANT,
        PROSE,
        Level::Weak,
        "Reports a word in comments, Markdown and text files spelt the way of the other English \
         variant than the dictionary chosen, for example an American spelling with English \
         (United Kingdom). The quick fix writes the chosen spelling. Only applies while one of \
         the two built-in English dictionaries is chosen alone.\n\nReported:\n    // English (United Kingdom) chosen\n    // pick a color\nAfter the quick fix:\n    // pick a colour",
    ),
    meta(
        crate::spelling_variants::MIXED_SPELLING,
        PROSE,
        Level::Weak,
        "Reports the less used of two spellings of one word in the same file, such as \
         'organise' where 'organize' is used more often. The quick fix writes the other \
         spelling, so the text reads alike throughout.\n\nReported:\n    // organize the files, then organise the tests\nAfter the quick fix:\n    // organize the files, then organize the tests",
    ),
    meta(
        "properties.duplicate-key",
        PROPERTIES,
        Level::Error,
        "Reports a key that occurs more than once in a .properties file. Only the last value is \
         used when the file is loaded, so the earlier one is silently lost.\n\nReported:\n    title=Start\n    title=Home",
    ),
    meta(
        "properties.missing-in-bundle",
        PROPERTIES,
        Level::Error,
        "Reports a key that is missing in other files of the same resource bundle, such as a key \
         of Messages.properties that Messages_de.properties lacks. Users of that locale then see \
         the fallback text.\n\nReported:\n    greeting=Hello   (no greeting in Messages_de.properties)",
    ),
    meta(
        "properties.trailing-spaces",
        PROPERTIES,
        Level::Warning,
        "Reports spaces at the end of a property value. They become part of the value, which is \
         rarely intended and invisible in most editors.\n\nReported:\n    name=Alice   \nAfter the quick fix:\n    name=Alice",
    ),
    meta(
        "java.anonymous-to-lambda",
        JAVA,
        Level::Weak,
        "Reports an anonymous class of a functional interface (Runnable, Comparator, the \
         interfaces of java.util.function or an interface of the same file with one abstract \
         method) whose only member is that method and which does not use this or super. A lambda \
         says the same in less code. The quick fix gives the shortest form at once: an \
         expression lambda for a single statement, a method reference where that statement \
         allows one.\n\nReported:\n    Runnable r = new Runnable() {\n        public void run() { work(); }\n    };\nAfter the quick fix:\n    Runnable r = this::work;",
    ),
    meta(
        "java.statement-lambda",
        JAVA,
        Level::Weak,
        "Reports a lambda whose block holds a single return or expression statement and no \
         comment. The expression alone is shorter and reads as a value. Not reported where a \
         Runnable and a Callable overload could tell the two forms apart, such as submit.\n\nReported:\n    names.forEach(n -> { print(n + 1); });\nAfter the quick fix:\n    names.forEach(n -> print(n + 1));",
    ),
    meta(
        "java.lambda-to-method-ref",
        JAVA,
        Level::Weak,
        "Reports a lambda that only passes its parameters, in order, to one method or \
         constructor, so a method reference can stand in its place. The receiver must be a plain \
         name, this or super, or the first parameter with a declared type; a receiver with a \
         call is left alone because a method reference would evaluate it only once. The message \
         names the reference.\n\nReported:\n    names.forEach(n -> System.out.println(n));\nAfter the quick fix:\n    names.forEach(System.out::println);",
    ),
    meta(
        "java.unused-declaration",
        JAVA,
        Level::Warning,
        "Reports classes, methods and fields that are not private and that nothing in the project \
         uses. It is a global inspection: it runs in Inspect Code over the project-wide reference \
         index and not while typing. Entry points such as main methods, annotated members, test \
         classes and methods that may implement a library method count as used; remove unused \
         code or keep it with @SuppressWarnings(\"unused\").\n\nReported:\n    public class Helper {\n        public void neverCalled() { }\n    }",
    ),
];

/// The entry of `id`.
pub(crate) fn of(id: &str) -> Option<&'static Meta> {
    META.iter().find(|m| m.id == id)
}
