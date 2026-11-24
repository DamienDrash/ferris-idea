# README structure

**Status:** v3 · 2026-11-14 · the README is the product's front page. v1 (2026-10-31) derived the
order from the ArtCraft family (analysis §4).
v2 followed the project owner's brief of 2026-11-14: carry all the information a reader may want,
say openly what the idea was, what the goal is now, how far the project is and what comes next,
and present Ferris IDEA as an IDE AI harness. v3 works in two critical reviews of v2: the harness
before the story, each number once, real captures instead of asserted value, and corrected facts.

## Two readers

- **The reader with 30 seconds** gets the first screen: head block, hero image, three sentences,
  the status callout, the highlights and "Use It, or Not Yet". After that they know what it is,
  what an agent gets from it, which stage it is at, and whether it is for them.
- **The reader with 30 minutes** gets the rest in the order of their questions: what can an agent
  do with it and what protects me (harness), what can it do (features), why does it exist (story),
  can I rely on it (how it is built, status), how do I get it (platforms, build), where do I read
  more.

About a third of the source is visible; the rest sits in `<details>` blocks whose summaries say
what is inside.

## Sections

| # | Section | Content | Rule |
|---|---|---|---|
| 1 | Head block (centred) | symbol in a `<picture>` (72 px), H1, tagline, secondary line, ≤ 3 static badges that link to their sections, link row | link row only to things that exist |
| 2 | Hero image | main window, dark and light in one `<picture>` | real screenshot only; a slot comment until it exists |
| 3 | Intro | three sentences: the IDE, "what we call an IDE AI harness" with three verbs an agent performs, who wrote it and what it targets; short independence line below | "source-available"; the reference only as a target; the category is "our term" |
| 4 | Status callout | `> [!WARNING]`: stage, no downloads, verified platform, rows compared side by side, no security review | no percentage against the reference on the first screen |
| 5 | Highlights | about eight bullets, each a capability and a status word | |
| 6 | Use It, or Not Yet | who should try it, who should not yet | in the reader's terms |
| 7 | What an Agent Can Do with It | definition in two paragraphs; permission-alert image; Mermaid diagram; Try It in Five Minutes (build, connect, client-neutral configuration, ways in, five steps with captures, more examples in details); How It Differs; What an Agent Can Use (details); What Keeps You in Control; What the Model Does Not Cover under `> [!CAUTION]`; What Leaves Your Machine | real output only, edits to it named in the caption; known gaps named as gaps; each limit with its planned fix; no tool counts of other products |
| 8 | Features | status legend with what Works does not mean; summary table; database image; one `<details>` per area with *Feature · Status · Notes* | one status word per row, never "Works to Experimental" |
| 9 | Remote Development | status table; `> [!NOTE]` on the unaudited composition; what the code is written to enforce, as short paragraphs in details | never "secure", "quantum-safe", "audited", "enforces" as an absolute |
| 10 | The Story | The Question · What Changed · The Goal Now · Timeline (details) · What Comes Next (five lines) · Questions You Will Ask | "clean-room" only qualified; planned items marked; the two obvious objections answered from the record |
| 11 | How It Is Built | who writes, reviews, decides; separated roles and their limit; how the reference is observed; open material; traceability as a rule; layers of checking; CI and what a green run does not prove; Rust facts (details) | rules written as rules |
| 12 | Development Status | stage paragraph; numbers table with two columns; how each number was measured (details); the gap list | the home of every number, including the feature-presence score |
| 13 | Platforms and Requirements | targets; verified machine; language tools | |
| 14 | Build from Source | requirements, numbered path, notes, what was verified for the page | no clone URL before the repository is public |
| 15 | Configuration and Files | table *File · Place · Holds* and how to remove everything (details) | |
| 16 | Documentation | map (details); Getting Help; Versioning | links resolve |
| 17 | FAQ | each question a `<details>` | no claim that is not made above |
| 18 | Who Decides | owner, agents, the planned export without development history | |
| 19 | Contributing | two sentences | |
| 20 | License | summary in the licence's own terms ("licensor", competing product "even when free of charge"), no-warranty sentence, name and logo, third-party notices | says it is a summary; never "resale", never "author" for the licensor |
| 21 | Independent Project Notice | text from BRAND_GUIDELINES.md §6 | mandatory |
| 22 | Acknowledgements | open platform material, fonts, palette, libraries, protocols | plain text, no logos |

Headings: Title Case for every H2 and H3 of the README (CONTENT_STYLE_GUIDE §2). No punctuation in
headings that changes the anchor GitHub derives.

## Slots

Until an image or an output exists, its place holds an HTML comment and nothing else:

- `<!-- SCREENSHOT (missing, …): … -->` for an image that does not exist yet. Planned are seven
  files, three of them visible without opening anything (hero dark and light, permission alert,
  database). In the repository since 2026-11-14: hero dark, database, editor with completion,
  commit. Missing: hero light, permission alert, remote project.
- `<!-- CAPTURE X (console|measurement): … -->` for real output or a measured value.
- `<!-- OWNER MUST CONFIRM …: … -->` for a statement only the project owner can make.

A slot is never filled with a mock-up or with invented output. At publication, a slot without
content is removed together with the sentence that leads to it.

Capture rules: themes Ferris IDEA Dark and Ferris IDEA Light only, never the dark theme taken from the
open platform files; window 1440×900 exported at 2×; the same sample project in every image; no
real host names, user names or paths; images in `docs/images/` under 1 MB each.

## Rendering rules for GitHub

- Works in light and dark mode: images through `<picture>` with a `prefers-color-scheme` source, or
  one image that reads on both. The wordmark and the horizontal logo are for dark surfaces only
  and are not used in the README.
- Only assets from `docs/design/assets/` and, once they exist, `docs/images/`. No external image
  hosts except static shields.io badges.
- Every image has alt text; every screenshot has a caption with what is shown and the version.
- Feature tables have at most three columns; the numbers table has two.
- Callouts: one `[!WARNING]` at the top, one `[!CAUTION]` above the limits, `[!NOTE]` where a
  reader must know that something is untested or unaudited. No second box that repeats the first.
- The diagram is Mermaid in the Markdown source, top to bottom, with quoted labels and no colours
  of its own.
- No emoji. Status is always a word.

## Checklist before merging a README change

- [ ] Every "Works" item runs on current `main` (tests or command).
- [ ] No planned feature written in present tense.
- [ ] Every number appears once, matches `main` and names its kind in the measurement details.
- [ ] The agent section lists its limits, and each is still true; it claims no more than
      SPEC-0053 and SPEC-0012 state. A limit that is fixed is removed there, in SECURITY.md and
      in ARCHITECTURE.md together.
- [ ] No "open source", no "replace", no comparison stated as achieved, no superlative, no
      absolute security statement.
- [ ] Links resolve (`scripts/check-docs.sh`).
- [ ] Images have alt text and a caption; no slot was filled with a mock-up or invented output.
- [ ] Independence notice present.
