#!/bin/sh
# Generates a large synthetic Java source tree (no build scripts, no foreign code) for indexing
# measurements. Deterministic: same arguments, same files.
#
#   gen-java-large.sh [target_dir] [packages] [classes_per_package]
#   default: ~/ReferenceEnv/projects/java-large, 50 packages x 100 classes = 5000 files
# MAVEN=1 also writes a pom.xml (no plugins; Guava and JUnit from Maven Central) so that
# opening the project includes a Maven import and library indexing.
set -eu
TARGET="${1:-$HOME/ReferenceEnv/projects/java-large}"
PACKAGES="${2:-50}"
CLASSES="${3:-100}"
[ ! -e "$TARGET" ] || { echo "$TARGET exists, remove it first" >&2; exit 1; }
awk -v root="$TARGET/src/main/java/org/ferrisidea/sample" -v np="$PACKAGES" -v nc="$CLASSES" 'BEGIN {
  for (p = 0; p < np; p++) {
    dir = sprintf("%s/p%02d", root, p)
    system("mkdir -p \"" dir "\"")
    for (c = 0; c < nc; c++) {
      f = sprintf("%s/Item%03d.java", dir, c)
      # each class refers to one class in the previous package, so references cross packages
      dep = sprintf("org.ferrisidea.sample.p%02d.Item%03d", (p + np - 1) % np, (c * 7) % nc)
      printf "package org.ferrisidea.sample.p%02d;\n\n", p > f
      printf "import java.util.ArrayList;\nimport java.util.List;\nimport java.util.Map;\nimport java.util.TreeMap;\n\n" > f
      printf "/** Synthetic class %d in package %d. */\npublic final class Item%03d {\n", c, p, c > f
      printf "    private final List<String> names = new ArrayList<>();\n" > f
      printf "    private final Map<String, Integer> counts = new TreeMap<>();\n" > f
      printf "    private %s next;\n\n", dep > f
      for (m = 0; m < 8; m++) {
        printf "    public int compute%d(int value) {\n", m > f
        printf "        int sum = value;\n" > f
        printf "        for (int i = 0; i < %d; i++) {\n", m + 3 > f
        printf "            sum += i * %d;\n", c + m + 1 > f
        printf "            names.add(\"n\" + i);\n" > f
        printf "            counts.merge(\"k\" + (i %% 5), 1, Integer::sum);\n        }\n" > f
        printf "        return next == null ? sum : sum + next.compute%d(value - 1);\n    }\n\n", m > f
      }
      printf "    public void link(%s other) {\n        this.next = other;\n    }\n}\n", dep > f
      close(f)
    }
  }
}'
if [ "${MAVEN:-0}" = 1 ]; then
  cat > "$TARGET/pom.xml" <<'POM'
<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>org.ferrisidea.sample</groupId>
  <artifactId>java-large</artifactId>
  <version>0.0.1</version>
  <properties>
    <maven.compiler.release>17</maven.compiler.release>
    <project.build.sourceEncoding>UTF-8</project.build.sourceEncoding>
  </properties>
  <dependencies>
    <dependency>
      <groupId>com.google.guava</groupId>
      <artifactId>guava</artifactId>
      <version>33.3.1-jre</version>
    </dependency>
    <dependency>
      <groupId>org.junit.jupiter</groupId>
      <artifactId>junit-jupiter</artifactId>
      <version>5.11.3</version>
      <scope>test</scope>
    </dependency>
  </dependencies>
</project>
POM
fi
echo "$TARGET: $(find "$TARGET" -name '*.java' | wc -l | tr -d ' ') files"
