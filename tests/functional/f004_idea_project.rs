//! F-004 · Project files of the reference IDE: a folder with IDE metadata is recognised as a project
//! (SPEC-0049 AK-1, AK-2, AK-4).

use crate::harness::*;
use serde_json::json;

#[test]
fn no_idea_folder_gives_null() {
    let p = Project::reference("java-basic");
    let mut ide = Ide::stdio(&p);
    assert_eq!(ide.ok("idea_project", json!({})), json!(null));
}

#[test]
fn idea_folder_gives_modules_and_sdk_without_opening_private_files() {
    let p = Project::reference("java-basic");
    p.write(".idea/misc.xml", r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectRootManager" version="2" languageLevel="JDK_21" project-jdk-name="21" project-jdk-type="JavaSDK">
    <output url="file://$PROJECT_DIR$/out" />
  </component>
</project>
"#)
    .write(".idea/modules.xml", r#"<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectModuleManager">
    <modules>
      <module fileurl="file://$PROJECT_DIR$/java-basic.iml" filepath="$PROJECT_DIR$/java-basic.iml" />
    </modules>
  </component>
</project>
"#)
    .write("java-basic.iml", r#"<?xml version="1.0" encoding="UTF-8"?>
<module type="JAVA_MODULE" version="4">
  <component name="NewModuleRootManager" inherit-compiler-output="true">
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/src/main/java" isTestSource="false" />
    </content>
    <orderEntry type="inheritedJdk" />
    <orderEntry type="sourceFolder" forTests="false" />
  </component>
</module>
"#)
    .write(".idea/workspace.xml", "<not xml");
    let mut ide = Ide::stdio(&p);
    let project = ide.ok("idea_project", json!({}));
    let all = text(&project);
    assert!(all.contains("java-basic"), "{all}");
    assert!(all.contains("JDK_21"), "{all}");
    assert!(all.contains("src/main/java"), "{all}");
    // Of workspace.xml only its run configurations count (SPEC-0049 v3); its text is never
    // shown, broken or not.
    assert!(!all.contains("<not xml"), "{all}");
}
