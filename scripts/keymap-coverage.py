#!/usr/bin/env python3
"""How much of the reference's macOS keymap IDECraft covers (SPEC-0028 v4 behaviour 9).

Compares, without a window, the macOS keymap of the bundled upstream keymap files
(`crates/ui/keymaps/`, intellij-community, Apache-2.0, pinned in `crates/ui/keymaps/README.md`)
with the keymap IDECraft bundles (those files plus `crates/ui/keymaps/macos-deviations.json`) and
with the action ids IDECraft's sources carry out, and writes
`docs/research/KEYMAP_COVERAGE.md`. Reads only files of this repository; the output depends on
nothing else (no date, no commit, sorted rows).

Usage: python3 -I scripts/keymap-coverage.py            writes the document
       python3 -I scripts/keymap-coverage.py --check    fails when the document is out of date
"""
import json
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

ROOT = pathlib.Path(__file__).resolve().parent.parent
KEYMAPS = ROOT / "crates" / "ui" / "keymaps"
OUT = ROOT / "docs" / "research" / "KEYMAP_COVERAGE.md"
MODIFIERS = ["control", "alt", "shift", "meta", "altGraph"]

# Where an action id has to appear as a string literal, outside test modules and comments, to
# count as carried out: the workbench and editor views with their menus and popups, the editor
# commands and the tool window ids.
DISPATCH_DIRS = ["crates/workbench/src"]
DISPATCH_FILES = ["crates/editor/src/commands.rs", "crates/ui/src/tool_windows.rs"]

# Reference actions IDECraft carries out under another id or with a fixed key:
# reference id -> (what carries it out, the keymap id whose shortcuts count or None, fixed keys).
MAPPED = {
    "EditorDelete": ("`$Delete`", "$Delete", []),
    "Exit": ("Quit of the application menu, fixed key (`menu_bar.rs`)", None, ["meta Q"]),
    "ToggleFindInSelection": ("`ToggleInSelection` (SPEC-0065)", "ToggleInSelection", []),
}

# Actions that leave a chord to another action bound to it (`yields_to` in
# `crates/ui/src/keymap.rs`, SPEC-0071 v10 behaviour 16): on such a chord the action neither runs
# nor shows the key, so the chord does not count as one of its shortcuts.
YIELDS = {"GotoTest": "ReopenClosedTab"}

# Chords macOS itself takes with its default settings (general knowledge; the user can change
# them in System Settings). A reference shortcut on one of them may never reach an application.
MACOS_SYSTEM = {
    "control SPACE": "select the previous input source",
    "control alt SPACE": "select the next input source",
    "meta SPACE": "Spotlight",
    "alt meta SPACE": "Finder search window",
    "control meta SPACE": "emoji and symbols",
    "meta TAB": "switch applications",
    "shift meta TAB": "switch applications backwards",
    "meta BACK_QUOTE": "next window of the application",
    "shift meta BACK_QUOTE": "previous window of the application",
    "meta H": "hide the application",
    "alt meta H": "hide the other applications",
    "meta M": "minimise the window",
    "meta Q": "quit the application",
    "control meta Q": "lock the screen",
    "shift meta Q": "log out",
    "alt meta ESCAPE": "force quit",
    "alt meta D": "show or hide the Dock",
    "control meta F": "full screen",
    "control UP": "Mission Control",
    "control DOWN": "application windows",
    "control LEFT": "move a space left",
    "control RIGHT": "move a space right",
    "shift meta 3": "screenshot",
    "shift meta 4": "screenshot of an area",
    "shift meta 5": "screenshot tools",
    "shift meta SLASH": "Help menu",
    "F11": "show the desktop",
}

AREAS = ["editor", "navigation", "refactoring", "VCS", "run/debug", "tool windows", "database", "other"]

# (pattern, area, owning specification with a hint), first match wins. The owner is assigned by
# the id's pattern, not by reading each action's behaviour.
RULES = [
    (r"^(Console\.|DatabaseView\.|Jdbc\.|Sql\.)", "database", "SPEC-0093 (SQL console)"),
    (r"^(Diff\.|DirDiffMenu\.|NextDiff$|PreviousDiff$|Compare)", "VCS", "SPEC-0084 (diff viewer)"),
    (r"^(ChangesView\.|Vcs\.ShowMessageHistory$|Vcs\.ToggleAmendCommitMode$|Vcs\.MoveChangedLinesToChangelist$|CheckinProject$|CheckinFiles$)", "VCS", "SPEC-0063 (Commit tool window)"),
    (r"^(ShelveChanges\.|ShelvedChanges\.)", "VCS", "SPEC-0106 (stash; shelves have no specification yet)"),
    (r"^Git\..*Branch", "VCS", "SPEC-0068 (branch popup)"),
    (r"^(Git\.|VcsHistory\.|Vcs\.Log|Annotate$)", "VCS", "SPEC-0086 (Git log tool window)"),
    (r"^(Vcs\.|LocalHistory)", "VCS", "SPEC-0109 (push, pull and the VCS operations popup)"),
    (r"(Breakpoint)", "run/debug", "SPEC-0127 (breakpoints)"),
    (r"^(Rerun|SwitchCoverage$|RunCoverage$|Coverage)", "run/debug", "SPEC-0088 (test results)"),
    (r"^(Compile|MakeModule$|CompileDirty$|CompileProject$)", "run/debug", "SPEC-0097 (build, run and debug of Java)"),
    (r"^(XDebugger\.|Debugger\.|Step|ForceStep|SmartStep|Resume$|Pause$|EvaluateExpression$|QuickEvaluateExpression$|RunToCursor$|ForceRunToCursor$|ShowExecutionPoint$|Hotswap$|Debug$|DebugClass$|ChooseDebugConfiguration$)", "run/debug", "SPEC-0091 (Debug tool window)"),
    (r"^(Run$|RunClass$|ChooseRunConfiguration$|RunAnything$|Stop$|StopBackgroundProcesses$|SendEOF$|editRunConfigurations$)", "run/debug", "SPEC-0087 (Run tool window)"),
    (r"^ActivateBookmarksToolWindow$", "tool windows", "SPEC-0331 (bookmarks)"),
    (r"^(Activate.*ToolWindow$|.*\.ToolWindow$)", "tool windows", "SPEC-0061 (window chrome); the tool window itself has no specification yet"),
    (r"^(HideActiveWindow$|HideAllWindows$|HideSideWindows$|JumpToLastWindow$|ResizeToolWindow|MaximizeToolWindow$|RestoreDefaultLayout$|CloseActiveTab$|ShowContent$|ViewToolButtons$)", "tool windows", "SPEC-0167 (tool window layout)"),
    (r"^(ServiceView\.)", "tool windows", "SPEC-0119 (Services tool window)"),
    (r"^(TodoView)", "tool windows", "SPEC-0334 (TODO tool window)"),
    (r"^(ChangeSignature$|ChangeTypeSignature$)", "refactoring", "SPEC-0113 (Change Signature)"),
    (r"^(CopyElement$|Move$)", "refactoring", "SPEC-0114 (move and copy of files)"),
    (r"^(RenameElement$|RenameFile$|Introduce|Extract|Inline$|SafeDelete$|Refactorings\.|MembersPullUp$|MemberPushDown$|InvertBoolean$)", "refactoring", "SPEC-0095 (refactoring shortcuts)"),
    (r"^(ToggleBookmark|GotoBookmark|ShowBookmarks$|ShowTypeBookmarks$|GotoNextBookmark$|GotoPreviousBookmark$)", "navigation", "SPEC-0331 (bookmarks)"),
    (r"^(SearchEverywhere|TextSearchAction$|GotoClass$|GotoFile$|GotoSymbol$|GotoAction$)", "navigation", "SPEC-0067 (Search Everywhere popup)"),
    (r"^(UsageView\.|UsageFiltering\.|UsageGrouping\.|FindUsages|ShowUsages$|HighlightUsagesInFile$|ShowSettingsAndFindUsages$|GotoNextElementUnderCaretUsage$|GotoPrevElementUnderCaretUsage$)", "navigation", "SPEC-0293 (Find tool window), SPEC-0073 (Find Usages)"),
    (r"^(Find|Replace|ToggleFindInSelection$|ShowSearchHistory$|ShowFilterPopup$|NextOccurence$|PreviousOccurence$)", "navigation", "SPEC-0065 (find bar), SPEC-0066 (Find in Files)"),
    (r"^(GotoLine$|RecentFiles$|RecentChangedFiles$|RecentLocations$|Switcher)", "navigation", "SPEC-0069 (Go to Line, Recent Files)"),
    (r"^(GotoImplementation$|GotoTypeDeclaration$|GotoSuperMethod$|QuickImplementations$)", "navigation", "SPEC-0081 (Go to Implementation and Type)"),
    (r"^(TypeHierarchy$|MethodHierarchy$|CallHierarchy$)", "navigation", "SPEC-0082 (hierarchies)"),
    (r"^(FileStructurePopup$)", "navigation", "SPEC-0079 (File Structure)"),
    (r"^(GotoNextError$|GotoPreviousError$|ShowErrorDescription$)", "navigation", "SPEC-0078 (problem navigation)"),
    (r"^(Goto|Back$|Forward$|JumpToLastChange$|MethodUp$|MethodDown$|ViewSource$|EditSource|SwitchHeaderSource$)", "navigation", "SPEC-0071 (Go to Declaration and navigation)"),
    (r"^(SelectIn$|ShowFilePath$|ShowNavBar$|NextTab$|PreviousTab$|NextEditorTab$|PreviousEditorTab$|NextSplitter$|PrevSplitter$|CloseContent$|CloseAllEditors$|OpenInRightSplit$|ReopenClosedTab$|MaximizeEditorInSplit$)", "navigation", "SPEC-0059 (project tree and tabs)"),
    (r"^(Collapse|Expand(?!LiveTemplate)|FullyExpandTreeNode$)", "editor", "SPEC-0083 (folding in the editor); tree keys: SPEC-0275"),
    (r"^(CodeCompletion$|SmartTypeCompletion$|ClassNameCompletion$|HippieCompletion$|HippieBackwardCompletion$|EditorChooseLookupItem|DumpLookupElementWeights$|NextInlineCompletion|PrevInlineCompletion)", "editor", "SPEC-0072 (code completion)"),
    (r"^(InsertLiveTemplate$|SurroundWithLiveTemplate$|ExpandLiveTemplate|NextTemplateVariable$|PreviousTemplateVariable$|SelectVirtualTemplateElement$)", "editor", "no specification yet (live templates)"),
    (r"^(QuickJavaDoc$|ExternalJavaDoc$|ToggleRenderedDocPresentation$|EditorContextInfo$|ExpressionTypeInfo$)", "editor", "SPEC-0077 (quick documentation)"),
    (r"^(ParameterInfo$|NextParameter$|PrevParameter$|MethodOverloadSwitch)", "editor", "SPEC-0080 (parameter info)"),
    (r"^(ShowIntentionActions$|CodeInspection\.|RunInspection$|QuickActionPopup$|CodeFloatingToolbar\.)", "editor", "SPEC-0076 (context actions)"),
    (r"^(ReformatCode$|ShowReformatFileDialog$|AutoIndentLines$|OptimizeImports$|ReformatWithPrettierAction$)", "editor", "SPEC-0075 (Reformat Code)"),
    (r"^(Generate$|OverrideMethods$|ImplementMethods$|DelegateMethods$|SurroundWith$|Unwrap$|MoveStatement|MoveLine|MoveElement|CommentBy|EditorCompleteStatement$|BraceOrQuoteOut$|FillParagraph$)", "editor", "SPEC-0056 (editor actions), SPEC-0122 (Java and Kotlin editor)"),
    (r"^(Editor|\$|SelectNextOccurrence$|SelectAllOccurrences$|UnselectPreviousOccurrence$|FindWordAtCaret$|PasteMultiple$|CopyPaths$|CopyReference$|CutCopyPasteGroup)", "editor", "SPEC-0057 (editor commands), SPEC-0058 (editor view)"),
    (r"^(FileChooser\.)", "other", "no specification yet (IDECraft uses the system's file dialog)"),
    (r"^(ShowSettings$|ShowProjectStructureSettings$)", "other", "SPEC-0105 (settings dialog)"),
    (r"^(NewElement|NewScratchFile$|SaveAll$|SaveAs$|Synchronize$|ForceRefresh$|ExportToTextFile$|\$?Print$)", "other", "SPEC-0059 (project tree and tabs)"),
    (r"^(NextWindow$|PreviousWindow$|NextProjectWindow$|PreviousProjectWindow$|ToggleFullScreen$|MinimizeCurrentWindow$|Exit$|ZoomInIdeAction$|ZoomOutIdeAction$|ResetIdeScaleAction$|ManageRecentProjects$|EmojiAndSymbols$)", "other", "SPEC-0124 (menu bar), SPEC-0061 (window chrome)"),
    (r"^(List-|Table-|Tree-)", "other", "SPEC-0275 (context menus and trees)"),
]
FALLBACK = ("other", "no owning specification yet")


def area_and_owner(action):
    for pattern, area, owner in RULES:
        if re.search(pattern, action):
            return area, owner
    return FALLBACK


def keystroke(text, swap):
    """The keystroke in the form `Keystroke::to_text` of `crates/ui/src/keymap.rs` writes."""
    words = text.split()
    mods = set()
    for word in words[:-1]:
        low = word.lower()
        if low in ("pressed", "released", "typed"):
            continue
        low = {"ctrl": "control", "altgraph": "altGraph"}.get(low, low)
        if low not in MODIFIERS:
            raise ValueError(f"invalid keystroke `{text}`")
        mods.add(low)
    if swap:
        mods = {{"control": "meta", "meta": "control"}.get(m, m) for m in mods}
    return " ".join([m for m in MODIFIERS if m in mods] + [words[-1].upper()])


def read_keymap(name, swap):
    """Shortcuts by action id of one file; a later entry of an action replaces the earlier one."""
    actions = {}
    for action in ET.parse(KEYMAPS / name).getroot().findall("action"):
        shortcuts = []
        for s in action:
            if s.tag == "keyboard-shortcut":
                entry = keystroke(s.get("first-keystroke"), swap)
                if s.get("second-keystroke"):
                    entry += ", " + keystroke(s.get("second-keystroke"), swap)
            elif s.tag == "mouse-shortcut":
                words = s.get("keystroke").split()
                if swap:
                    words = [{"control": "meta", "ctrl": "meta", "meta": "control"}.get(w.lower(), w) for w in words]
                entry = "mouse: " + " ".join(words)
            elif s.tag == "keyboard-gesture-shortcut":
                entry = f"gesture: {s.get('modifier')} {s.get('keystroke')}"
            else:
                continue
            shortcuts.append(entry)
        actions[action.get("id")] = shortcuts
    return actions


def reference_keymap():
    """SPEC-0028 behaviours 3 and 3a: the macOS file over `$default` with Control and Meta exchanged."""
    actions = read_keymap("$default.xml", True)
    actions.update(read_keymap("Mac OS X 10.5+.xml", False))
    return {a: s for a, s in actions.items() if s}


def bundled_keymap(reference, deviations):
    ours = {a: list(s) for a, s in reference.items()}
    for d in deviations:
        shortcuts = ours.setdefault(d["action"], [])
        if d["change"] == "add":
            if d["shortcut"] not in shortcuts:
                shortcuts.append(d["shortcut"])
        elif d["change"] == "remove":
            shortcuts.remove(d["shortcut"])
        else:
            raise ValueError(f"unknown change `{d['change']}`")
    return {a: s for a, s in ours.items() if s}


def dispatch_text():
    files = []
    for d in DISPATCH_DIRS:
        files += sorted((ROOT / d).rglob("*.rs"))
    files += [ROOT / f for f in DISPATCH_FILES]
    parts = []
    for f in files:
        text = f.read_text(encoding="utf-8")
        m = re.search(r"^#\[cfg\(test\)\]\nmod \w+ \{", text, re.M)
        if m:
            text = text[: m.start()]
        parts.append("\n".join(l for l in text.splitlines() if not l.lstrip().startswith("//")))
    return "\n".join(parts)


def code(shortcuts):
    return "<br>".join(f"`{s}`" for s in shortcuts) if shortcuts else "—"


def pinned_commit():
    readme = (KEYMAPS / "README.md").read_text(encoding="utf-8")
    commits = set(re.findall(r"`([0-9a-f]{40})`", readme))
    if len(commits) != 1:
        raise ValueError("crates/ui/keymaps/README.md must name one pinned commit")
    return commits.pop()


def build():
    deviations = json.loads((KEYMAPS / "macos-deviations.json").read_text(encoding="utf-8"))
    reference = reference_keymap()
    ours = bundled_keymap(reference, deviations)
    sources = dispatch_text()

    def carried(action):
        return f'"{action}"' in sources

    rows = []
    for action in sorted(reference):
        area, owner = area_and_owner(action)
        ref = reference[action]
        if carried(action):
            how, mine = "same id", ours.get(action, [])
            if action in YIELDS:
                mine = [s for s in mine if s not in ours.get(YIELDS[action], [])]
        elif action in MAPPED:
            how, other, fixed = MAPPED[action]
            mine = fixed + (ours.get(other, []) if other else [])
            if other and not carried(other):
                raise ValueError(f"{action} is mapped to {other}, which nothing carries out")
        else:
            how, mine = None, []
        if how is None:
            status = "action missing"
        elif not mine:
            status = "action exists without shortcut"
        elif all(s in mine for s in ref):
            status = "same"
        else:
            status = "different shortcut"
        rows.append({"action": action, "area": area, "owner": owner, "ref": ref, "how": how, "mine": mine, "status": status})

    total = len(rows)
    statuses = ["same", "different shortcut", "action exists without shortcut", "action missing"]
    count = {s: sum(1 for r in rows if r["status"] == s) for s in statuses}
    have = total - count["action missing"]

    def pct(n, of):
        return f"{100 * n / of:.1f} %" if of else "—"

    # Chords of the bundled keymap that the reference keymap gives to other actions.
    by_chord = {}
    for action, shortcuts in reference.items():
        for s in shortcuts:
            by_chord.setdefault(s, []).append(action)
    conflicts, own = [], []
    for d in sorted(deviations, key=lambda d: (d["shortcut"], d["action"])):
        if d["change"] != "add":
            continue
        theirs = sorted(a for a in by_chord.get(d["shortcut"], []) if a != d["action"])
        (conflicts if theirs else own).append((d, theirs))

    def carried_or_mapped(action):
        return carried(action) or action in MAPPED

    out = []
    w = out.append
    w("# Keymap coverage against the open keymap")
    w("")
    w("Generated by `scripts/keymap-coverage.py` (SPEC-0028 v4 behaviour 9); do not edit by hand.")
    w("`python3 -I scripts/keymap-coverage.py --check` fails when it is out of date.")
    w("")
    w("**Reference:** the macOS keymap of the open keymap files of `JetBrains/intellij-community`")
    w(f"(Apache-2.0) at commit `{pinned_commit()}`, bundled unchanged in `crates/ui/keymaps/`:")
    w("`Mac OS X 10.5+.xml` over `$default.xml`, inherited shortcuts with Control and Meta exchanged")
    w("(SPEC-0028 behaviours 3 and 3a). **IDECraft:** the bundled macOS keymap, which is these files plus")
    w("the shortcuts listed in `crates/ui/keymaps/macos-deviations.json`.")
    w("")
    w("## What is measured, and what is not")
    w("")
    w("- An action counts as present when its id is a string literal in the sources that carry out actions")
    w("  (`crates/workbench/src`, `crates/editor/src/commands.rs`, `crates/ui/src/tool_windows.rs`; test")
    w("  modules and comments left out), or when it is one of the mapped actions below. This is evidence")
    w("  from the source text, not from running the action: it says neither that the action does all the")
    w("  reference's action does, nor in which focus it works.")
    w("- **same** means every shortcut the reference keymap gives the action is bound in IDECraft's keymap;")
    w("  additional shortcuts of IDECraft are listed under the deviations. Mouse shortcuts and gestures are")
    w("  compared as keymap entries only.")
    w("- A chord an action yields to another action of IDECraft's keymap (`GotoTest` on the chord of")
    w("  `ReopenClosedTab`, SPEC-0071 v10) does not count as its shortcut: the action exists and the")
    w("  chord does not run it.")
    w("- The reference also binds keys in the declarations of its actions and plugins, outside the keymap")
    w("  files. Those are not part of this comparison, and nothing here was checked against a running")
    w("  reference IDE.")
    w("- Keys that popups, trees and dialogs handle themselves, without the keymap, are not measured.")
    w("")
    w("## Summary")
    w("")
    w(f"The reference's macOS keymap binds **{total}** actions.")
    w("")
    w("| Result | Actions | Share of all | Share of the present ones |")
    w("|---|---:|---:|---:|")
    for s in statuses[:3]:
        w(f"| {s} | {count[s]} | {pct(count[s], total)} | {pct(count[s], have)} |")
    w(f"| action missing | {count['action missing']} | {pct(count['action missing'], total)} | — |")
    w(f"| **action present** | **{have}** | **{pct(have, total)}** | 100.0 % |")
    w("")
    w(f"Chords on which IDECraft's keymap binds an action the reference keymap does not bind there: {len(conflicts)}")
    w(f"(table below). Shortcuts of IDECraft on chords the reference keymap leaves free: {len(own)}.")
    w("")
    w("| Area | Bound in the reference | same | different shortcut | exists without shortcut | missing | present |")
    w("|---|---:|---:|---:|---:|---:|---:|")
    for area in AREAS:
        part = [r for r in rows if r["area"] == area]
        c = [sum(1 for r in part if r["status"] == s) for s in statuses]
        w(f"| {area} | {len(part)} | {c[0]} | {c[1]} | {c[2]} | {c[3]} | {pct(len(part) - c[3], len(part))} |")
    w("")
    w("The area is assigned by the pattern of the action id (`RULES` in the script).")
    w("")
    w("## Mapped actions")
    w("")
    w("Reference actions IDECraft carries out under another id or with a fixed key.")
    w("")
    w("| Reference action | Carried out by | Reference shortcuts | IDECraft shortcuts | Result |")
    w("|---|---|---|---|---|")
    for r in rows:
        if r["action"] in MAPPED:
            w(f"| `{r['action']}` | {r['how']} | {code(r['ref'])} | {code(r['mine'])} | {r['status']} |")
    w("")
    w("## Deviations of the bundled macOS keymap")
    w("")
    w("Every shortcut by which the bundled keymap differs from the upstream files")
    w("(`crates/ui/keymaps/macos-deviations.json`; the test")
    w("`keymap::tests::bundled_macos_keymap_differs_from_upstream_only_as_listed` fails on any other difference).")
    w("")
    w("| Action | Change | Shortcut | Decided by | Reason |")
    w("|---|---|---|---|---|")
    for d in deviations:
        w(f"| `{d['action']}` | {d['change']} | `{d['shortcut']}` | {d['decided_by']} | {d['reason']} |")
    w("")
    w("## Conflicts: the same chord, another action")
    w("")
    w("Chords on which IDECraft's keymap binds an action that the reference keymap does not bind there,")
    w("while the reference keymap binds other actions to it.")
    w("")
    w("| Chord | IDECraft's action | Reference's actions on the chord | Effect in IDECraft |")
    w("|---|---|---|---|")
    for d, theirs in conflicts:
        present = [a for a in theirs if carried_or_mapped(a)]
        names = ", ".join(f"`{a}`" + (" (present)" if a in present else " (missing)") for a in theirs)
        yielding = [a for a in present if YIELDS.get(a) == d["action"]]
        if yielding and len(yielding) == len(present):
            effect = "IDECraft's action runs; " + ", ".join(f"`{a}`" for a in yielding) + " yields the chord and is reached through its menu and Find Action"
        elif not present:
            effect = "only IDECraft's action can run"
        elif MAPPED.get(present[0], (None, None))[1] == d["action"] and len(present) == 1:
            effect = "the same command under IDECraft's id"
        else:
            effect = "shared: the keymap offers both, the focused part decides"
        w(f"| `{d['shortcut']}` | `{d['action']}` | {names} | {effect} |")
    if not conflicts:
        w("| — | — | — | — |")
    w("")
    w("## Shortcuts of IDECraft on chords the reference keymap leaves free")
    w("")
    w("Free in the keymap files; the reference's plugins may still use them.")
    w("")
    w("| Chord | IDECraft's action | Decided by |")
    w("|---|---|---|")
    for d, _ in own:
        w(f"| `{d['shortcut']}` | `{d['action']}` | {d['decided_by']} |")
    w("")
    w("## Reference shortcuts on chords macOS takes itself")
    w("")
    w("With macOS's default settings these chords may not reach an application. They are kept as the")
    w("reference has them (the user can free them in System Settings); where IDECraft adds a second key,")
    w("the deviations above say so.")
    w("")
    w("| Chord | macOS uses it for | Reference actions | Present in IDECraft |")
    w("|---|---|---|---|")
    for chord in sorted(MACOS_SYSTEM):
        actions = sorted(by_chord.get(chord, []))
        if actions:
            present = [a for a in actions if carried_or_mapped(a)]
            w(f"| `{chord}` | {MACOS_SYSTEM[chord]} | {', '.join(f'`{a}`' for a in actions)} | {', '.join(f'`{a}`' for a in present) or '—'} |")
    w("")
    w("## Missing actions by area")
    w("")
    w("Bound in the reference's macOS keymap, no action in IDECraft. The owner is the existing")
    w("specification that would take the action, assigned by the id's pattern; nothing is implied about")
    w("whether or when it is built.")
    for area in AREAS:
        part = [r for r in rows if r["area"] == area and r["status"] == "action missing"]
        w("")
        w(f"### {area} ({len(part)})")
        w("")
        if not part:
            w("None.")
            continue
        w("| Action | Reference shortcuts | Would be owned by |")
        w("|---|---|---|")
        for r in part:
            w(f"| `{r['action']}` | {code(r['ref'])} | {r['owner']} |")
    w("")
    w("## Every action the reference's macOS keymap binds")
    w("")
    w("| Action | Area | Reference shortcuts | IDECraft action | IDECraft shortcuts | Result |")
    w("|---|---|---|---|---|---|")
    for r in rows:
        w(f"| `{r['action']}` | {r['area']} | {code(r['ref'])} | {r['how'] or '—'} | {code(r['mine'])} | {r['status']} |")
    return "\n".join(out) + "\n"


def main():
    text = build()
    if sys.argv[1:] == ["--check"]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT.relative_to(ROOT)} is out of date: run python3 -I scripts/keymap-coverage.py")
            return 1
        print(f"{OUT.relative_to(ROOT)} is up to date")
        return 0
    if sys.argv[1:]:
        print(__doc__)
        return 2
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
