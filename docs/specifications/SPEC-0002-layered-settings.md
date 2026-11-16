# SPEC-0002: Layered settings

- **Version:** 1 · **Date:** 2026-10-30
- **Sources:** GEN (layered configuration as used by many tools: defaults < user < project)
- **Feature matrix rows:** F-002

## Purpose
Settings come from built-in defaults, the user and the workspace; the most specific wins.

## Behaviour
1. Layers in ascending precedence: defaults, user, workspace. [GEN]
2. Settings are TOML tables addressed by dotted keys (`editor.tab_size`). [GEN]
3. A higher layer overrides a key without discarding sibling keys from lower layers; nested tables merge, other values replace. [GEN]
4. Replacing a layer drops all values it previously contributed. [GEN]
5. For each key the IDE can tell which layer supplied the effective value. [GEN]
6. Unknown keys yield "not set"; a value of the wrong type yields an error naming the key. Invalid TOML yields an error naming the layer. [GEN]

## Acceptance criteria
- **AK-1** User `tab_size` overrides the default while other default editor keys remain.
- **AK-2** Workspace beats user regardless of the order in which layers were loaded; origin reports the workspace.
- **AK-3** Replacing a layer with an empty one restores the lower value.
- **AK-4** Missing key → not set; wrong type → typed error.
- **AK-5** Invalid TOML → error with layer.
