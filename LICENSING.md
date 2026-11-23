# Licensing

Ferris IDEA is **source-available**. Its own code and documentation are under the
[PolyForm Shield License 1.0.0](LICENSE.md) (<https://polyformproject.org/licenses/shield/1.0.0>).
[`LICENSE.md`](LICENSE.md) is the unmodified text of that licence, followed by the
`Required Notice:` line the licence provides for. Third-party parts keep their own licences.

This page explains the terms in plain language. It is not legal advice and it changes nothing:
where this page and `LICENSE.md` differ, `LICENSE.md` applies. Where a paraphrase could drift, the
licence is quoted. Decision record: [ADR-0032](docs/adr/0032-source-available-licence.md).

Ferris IDEA is not open-source software in the sense of the Open Source Definition, because the
licence excludes one purpose: providing a product that competes with Ferris IDEA or with another
product the licensor provides using it.

The licensor is **Damien Frigewski**, a private person, who offers these terms. The choice of
this licence is his. `LICENSE.md` is the unmodified text of the PolyForm Shield License 1.0.0;
the only line added is the `Required Notice:` line that names him.

## The rule

The whole limit is one sentence of the licence ("Noncompete"):

> Any purpose is a permitted purpose, except for providing any product that competes with the
> software or any product the licensor or any of its affiliates provides using the software.

And the licence says what competing means ("Competition"):

> Goods and services compete even when they provide functionality through different kinds of
> interfaces or for different technical platforms. Applications can compete with services,
> libraries with plugins, frameworks with development tools, and so on, even if they're written in
> different programming languages or for different computer architectures. Goods and services
> compete even when provided free of charge. If you market a product as a practical substitute for
> the software or another product, it definitely competes.

A product, in the licence's definition, "can be a good or service, or a combination of them".

## What you may do without asking

For every permitted purpose the licence lets you use the software, make "changes and new works
based on the software" and "distribute copies of the software". So, as long as you are not
providing a product that competes in the sense above:

| You want to | Allowed? |
|---|---|
| Read the source and build it | Yes |
| Use Ferris IDEA privately, for study, research or a hobby project | Yes |
| Use Ferris IDEA at your job, as a freelancer or anywhere in a company, to write software, commercial software included | Yes, as long as that software is not a product that competes with Ferris IDEA or with another product the licensor provides using it (see "What needs the licensor's consent") |
| Change it for your own use or your team's use | Yes |
| Pass copies on to others, changed or unchanged | Yes, with the terms and the notice line (next paragraph); see below for where passing on a copy ends and providing a competing product begins |

The licence makes no claim on the code, databases and documents you work on with Ferris IDEA.

**Notice duty.** In the licence's words: "You must ensure that anyone who gets a copy of any part
of the software from you also gets a copy of these terms or the URL for them above, as well as
copies of any plain-text lines beginning with `Required Notice:` that the licensor provided with
the software." Those lines are in [`LICENSE.md`](LICENSE.md) and [`NOTICE`](NOTICE).

The licence does "not allow you to sublicense or transfer any of your licenses to anyone else".
Whoever gets a copy from you needs their own licence under the same terms.

The licence also contains a patent licence from the licensor. It ends if you make "any written
claim that the software infringes or contributes to infringement of any patent".

## What needs the licensor's consent

Providing a product that competes with Ferris IDEA, or with another product the licensor or any of
its affiliates provides using Ferris IDEA. Payment is not the test: a product competes "even when
provided free of charge".

How the project reads the terms. This is the licensor's own reading, not legal advice; the
licence text decides:

- Offering Ferris IDEA, or a renamed or modified build of it, to others as a development environment
  or code editor of your own is providing a product that competes — whether you charge for it,
  bundle it or give it away.
- Offering a hosted or cloud development environment built on Ferris IDEA is providing a product that
  competes.
- Using Ferris IDEA's code in another development tool (a plugin, a database client, an editor
  component) can be providing a product that competes: "libraries with plugins, frameworks with
  development tools". Ask first.
- Using Ferris IDEA as your tool to build a product that itself competes with Ferris IDEA is use for
  the excluded purpose.
- Using Ferris IDEA inside your company to build products that do not compete with Ferris IDEA, or with
  another product the licensor provides using it, is a permitted purpose, whatever those products
  earn.
- Passing on a copy and providing a product: the licence's sentence on distribution ("The licensor
  grants you an additional copyright license to distribute copies of the software") names no
  purpose of its own; the limit is the excluded purpose. The project reads that as follows: giving
  a copy to a colleague, mirroring the repository or packaging Ferris IDEA under its own name with
  the terms and the notice line is passing on copies; presenting a copy or a build to others as
  your own product, or as a substitute for Ferris IDEA, is providing a product that competes. Where
  your case lies between the two, ask.

Two special cases are in the licence itself ("New Products", "Discontinued Products"); read them
there if your product comes close to Ferris IDEA's scope.

On violations the licence says: "The first time you are notified in writing that you have violated
any of these terms, or done anything with the software not covered by your licenses, your licenses
can nonetheless continue if you come into full compliance with these terms, and take practical
steps to correct past violations, within 32 days of receiving notice. Otherwise, all your licenses
end immediately."

## Asking the licensor for another licence

Anything these terms do not allow can be agreed separately with the licensor, for example a
licence to provide a product built on Ferris IDEA. The licence leaves that open: its terms do not
"prevent the licensor from granting licenses to anyone else".

Licensor: Damien Frigewski

Contact: by e-mail to Damien Frigewski, <dfrigewski@gmail.com>

Say who you are, what you want to provide and which parts of Ferris IDEA it uses. Consent counts only
when the licensor gives it in writing.

## Third-party parts

The licensor can set terms only for the licensor's own work. These parts keep their own licences,
and you may use them under those licences:

- Keymaps, themes, colour schemes and icons from `JetBrains/intellij-community`: Apache License
  2.0. Files and sources: [`docs/legal/PROVENANCE.md`](docs/legal/PROVENANCE.md).
- The font JetBrains Mono: SIL Open Font License 1.1.
- The built-in English (United States) spelling dictionary: SCOWL and the licences of its sources,
  all permissive; their text is `crates/languages/dictionaries/README_en_US.txt`.
- Rust dependencies and the native libraries compiled into the binaries: mostly MIT, Apache-2.0
  and BSD-style licences, some MPL-2.0. Full list with licence texts:
  [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).
- A third-party Rust crate kept in a folder under `vendor/` with changes of Ferris IDEA: its own
  licence, whose text is in that folder; the folder's `IDECRAFT.md` names the version it was taken
  from and every change.

Licence texts: [`LICENSES/`](LICENSES/README.md). Upstream notices: [`NOTICE`](NOTICE).

## Name and logo

The licence grants no rights in the name Ferris IDEA or its logo. See
[`TRADEMARKS.md`](TRADEMARKS.md).

## AI-assisted development

Large parts of Ferris IDEA were written with AI coding agents working under the project owner's
direction. How far copyright protects such output is not settled and
differs between countries. Where a part should turn out not to be protected by copyright, a
copyright licence cannot restrict that part. The project offers the work as a whole under the
terms above and makes no statement about individual files.

## No warranty

In the licence's words: "As far as the law allows, the software comes as is, without any warranty
or condition, and the licensor will not be liable to you for any damages arising out of these terms
or the use or nature of the software, under any kind of legal claim." Mandatory law that cannot be
excluded stays in force.

Ferris IDEA is alpha software. Keep backups of the projects and databases you open with it.
