#!/usr/bin/env python3
"""Ferris IDEA brand artwork from one grid definition (2026-11-22).

The symbol is a crab of 28 x 19 cells (the left half below, mirrored) with a cyan text cursor in
its centre, drawn only from square cells. Every SVG master is generated from it, so symbol, app
icon, logo and social preview never drift apart:

    python3 -I scripts/brand-assets.py <output folder>

PNG renderings, the macOS .icns (iconutil) and the Windows .ico are made from these SVGs; the
concept came from an image the project owner chose on 2026-11-22, redrawn here on the grid.
"""
import sys, os
OUT = sys.argv[1]
LEFT = [  # 14 columns of the left half; the right half is its mirror. '#' body, 'e' eye.
 "##..##........",
 "##..##........",
 "######........",
 "####..........",
 "####....####..",
 "..#....###....",
 "..###..###....",
 "....#..##.....",
 "....#####..e..",
 "......###.....",
 "......##......",
 "......###.....",
 ".......##.....",
 ".......##.....",
 "...######.....",
 "..##...#####..",
 "..#..##.##....",
 "..#..#..##....",
 "..#..#..##....",
]
W, H = 28, len(LEFT)
ROWS = [l + l[::-1] for l in LEFT]
CURSOR = (13.5, 10, 1, 3)  # x, y, w, h in grid units: the cyan text cursor in the centre

def runs(row):
    out, x = [], 0
    while x < len(row):
        if row[x] != '.':
            s = x
            while x < len(row) and row[x] != '.': x += 1
            out.append((s, x - s))
        else: x += 1
    return out

def path(ox, oy, u):
    d = []
    for y, row in enumerate(ROWS):
        for x, n in runs(row):
            d.append(f"M{ox+x*u:g} {oy+y*u:g}h{n*u:g}v{u:g}h{-n*u:g}z")
    return "".join(d)

def cursor(ox, oy, u):
    x, y, w, h = CURSOR
    return f'<rect x="{ox+x*u:g}" y="{oy+y*u:g}" width="{w*u:g}" height="{h*u:g}"'

def crab(ox, oy, u, fg, acc):
    return f'<path d="{path(ox, oy, u)}" fill="{fg}"/>{cursor(ox, oy, u)} fill="{acc}"/>'

def write(name, svg):
    open(os.path.join(OUT, name), "w").write(svg + "\n")


# The wordmark "Ferris IDEA" as glyph outlines (from the project owner's logo files of 2026-11-22):
# coordinates of a 1120x360 artboard, cap height 91 units, baseline at y 226, x from 363 to 1032.
WORD_FERRIS = '<path d="M150 0H417V596H1037V818H417V1265H1104V1490H150Z" transform="translate(363.000 226.000) scale(0.06103516 -0.06103516)"/><path d="M632 -23C881 -23 1055 96 1109 277L870 326C835 231 749 181 635 181C468 181 355 287 348 486H1126V568C1126 960 888 1132 615 1132C295 1132 89 896 89 552C89 202 295 -23 632 -23ZM349 663C361 812 457 928 616 928C771 928 858 823 873 663Z" transform="translate(436.488 226.000) scale(0.06103516 -0.06103516)"/><path d="M138 0H398V658C398 800 505 900 650 900C696 900 751 893 773 887V1126C748 1130 707 1133 678 1133C550 1133 442 1060 402 931H390V1118H138Z" transform="translate(506.375 226.000) scale(0.06103516 -0.06103516)"/><path d="M138 0H398V658C398 800 505 900 650 900C696 900 751 893 773 887V1126C748 1130 707 1133 678 1133C550 1133 442 1060 402 931H390V1118H138Z" transform="translate(555.996 226.000) scale(0.06103516 -0.06103516)"/><path d="M138 0H398V1118H138ZM268 1276C351 1276 420 1340 420 1419C420 1498 351 1561 268 1561C185 1561 117 1498 117 1419C117 1340 185 1276 268 1276Z" transform="translate(606.840 226.000) scale(0.06103516 -0.06103516)"/><path d="M561 -23C843 -23 1041 123 1041 335C1041 492 940 592 728 637L538 677C424 702 379 743 379 808C379 884 463 938 573 938C693 938 757 876 783 798L1015 844C970 1017 823 1132 570 1132C302 1132 119 999 119 792C119 628 222 525 433 479L624 438C726 416 775 375 775 310C775 234 694 176 566 176C439 176 361 229 331 340L88 294C127 91 304 -23 561 -23Z" transform="translate(640.348 226.000) scale(0.06103516 -0.06103516)"/>'
WORD_IDEA = '<path d="M417 1490H150V0H417Z" transform="translate(743.012 226.000) scale(0.06103516 -0.06103516)"/><path d="M654 0C1108 0 1375 281 1375 747C1375 1210 1108 1490 664 1490H150V0ZM417 231V1259H648C953 1259 1112 1088 1112 747C1112 403 953 231 641 231Z" transform="translate(777.617 226.000) scale(0.06103516 -0.06103516)"/><path d="M150 0H1122V225H417V641H1066V863H417V1265H1118V1490H150Z" transform="translate(867.887 226.000) scale(0.06103516 -0.06103516)"/><path d="M50 0H342L464 369H1021L1148 0H1441L909 1490H570ZM535 584 598 774C639 907 684 1074 737 1282C792 1076 839 911 883 774L948 584Z" transform="translate(943.570 226.000) scale(0.06103516 -0.06103516)"/>'
def wordmark(x, baseline, cap, fg, acc):
    k = cap / 91.0
    return (f'<g transform="translate({x - 363*k:.3f} {baseline - 226*k:.3f}) scale({k:.5f})">'
            f'<g fill="{fg}" aria-label="Ferris">{WORD_FERRIS}</g><g fill="{acc}" aria-label="IDEA">{WORD_IDEA}</g></g>')
DARK, LIGHT, FG, CYAN, BLUE, EDGE = "#2E3440", "#ECEFF4", "#ECEFF4", "#88C0D0", "#5E81AC", "#3B4252"
# Symbol on a 32-unit square: the 28x19 crab centred (2 units left/right, 6.5 above/below).
write("ferris-idea-symbol.svg", f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32">{crab(2, 6.5, 1, FG, CYAN)}</svg>')
write("ferris-idea-symbol-light.svg", f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32">{crab(2, 6.5, 1, DARK, BLUE)}</svg>')
# App icon 512: the old frame (rounded square, same radius and edge), crab 12 units per cell.
u = 12; ox = 256 - W*u/2; oy = 256 - H*u/2
write("ferris-idea-app-icon.svg", f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512" fill="none"><rect x="32" y="32" width="448" height="448" rx="100" fill="{DARK}" stroke="{EDGE}" stroke-width="4"/>{crab(ox, oy, u, FG, CYAN)}</svg>')
# Logo: symbol + wordmark ("Ferris" foreground, "IDEA" accent), dark and light.
def logo(bg, fg, acc):
    u = 8; ox = 40; oy = (240 - H*u)/2
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 960 240" width="960" height="240">'
            f'<rect width="960" height="240" fill="{bg}"/>{crab(ox, oy, u, fg, acc)}'
            + wordmark(300, 160, 80, fg, acc) + '</svg>')
write("ferris-idea-logo.svg", logo(DARK, FG, CYAN))
write("ferris-idea-logo-light.svg", logo(LIGHT, DARK, BLUE))
# Social preview 1280x640: tagline left, crab right over a faint editor frame; stage without a fixed version.
u = 16; ox = 1240 - W*u - 40; oy = 320 - H*u/2 + 20
grid = "".join(f'<path d="M{x} 0V640" stroke="{EDGE}" stroke-opacity="0.35"/>' for x in range(0, 1281, 40)) + \
       "".join(f'<path d="M0 {y}H1280" stroke="{EDGE}" stroke-opacity="0.35"/>' for y in range(0, 641, 40))
font = "Inter, 'Helvetica Neue', Arial, sans-serif"
write("ferris-idea-social-preview.svg",
  f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1280 640" width="1280" height="640">'
  f'<rect width="1280" height="640" fill="{DARK}"/>{grid}'
  f'<rect x="700" y="96" width="536" height="310" rx="12" fill="none" stroke="#4C566A" stroke-width="3"/>'
  f'<path d="M700 146H1236M796 146V406" stroke="#4C566A" stroke-width="3"/>'
  f'<rect x="724" y="114" width="18" height="14" fill="#4C566A"/><rect x="752" y="114" width="18" height="14" fill="#4C566A"/><rect x="780" y="114" width="18" height="14" fill="#4C566A"/>'
  f'{crab(ox, oy, u, FG, CYAN)}'
  f'<text x="64" y="206" font-family="{font}" font-size="76" font-weight="700" fill="{FG}">Your IDE.</text>'
  f'<text x="64" y="300" font-family="{font}" font-size="76" font-weight="700" fill="{FG}">Your database.</text>'
  f'<text x="64" y="394" font-family="{font}" font-size="76" font-weight="700" fill="{CYAN}">Your model.</text>'
  f'<text x="68" y="470" font-family="\'JetBrains Mono\', Menlo, monospace" font-size="24" fill="#D8DEE9">Stable · Rust-native · Built-in SQL client</text>'
  + wordmark(64, 590, 24, FG, CYAN) + '</svg>')
# The startup animation's masters (SPEC-0355 behaviour 23; crates/ui/src/brand.rs): plain absolute
# path data (M H V C Z, no transforms), which the animation reads and draws itself.
#   symbol-dark   32x32: background rect, crab path (one rectangle per run), cursor path
#   wordmark      76.5x32: "Ferris" path, "IDEA" path; ink 2 units from either edge, cap height
#                 10 (y 11 to the baseline at 21), spacing as in the logo
#   horizontal    109x32: crab path, cursor path, the wordmark's paths in a group translated by
#                 32.5, so the wordmark starts 4.5 units after the crab (the logo's proportions)
# File names stay those of the Ferris IDEA masters until rebranding step 2.
import re
def crab_abs(ox, oy):
    return "".join(f"M{ox+x:g} {oy+y:g}H{ox+x+n:g}V{oy+y+1:g}H{ox+x:g}Z"
                   for y, row in enumerate(ROWS) for x, n in runs(row))
def cursor_abs(ox, oy):
    x, y, w, h = CURSOR
    return f"M{ox+x:g} {oy+y:g}H{ox+x+w:g}V{oy+y+h:g}H{ox+x:g}Z"
GLYPH = re.compile(r'<path d="([^"]+)" transform="translate\(([\d.]+) ([\d.]+)\) scale\(([\d.]+) -[\d.]+\)"/>')
def glyphs(word):  # [(command, [(artboard x, artboard y), ...]), ...] per glyph, transforms applied
    out = []
    for d, tx, ty, k in GLYPH.findall(word):
        tx, ty, k = float(tx), float(ty), float(k)
        toks = re.findall(r'[A-Za-z]|-?[\d.]+', d)
        cmds, i = [], 0
        while i < len(toks):
            c = toks[i]; i += 1
            n = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "Z": 0}[c]
            while True:
                v = [float(t) for t in toks[i:i+n]]; i += n
                if c == "H": cmds.append((c, [(tx + v[0]*k, None)]))
                elif c == "V": cmds.append((c, [(None, ty - v[0]*k)]))
                else: cmds.append((c, [(tx + v[j]*k, ty - v[j+1]*k) for j in range(0, n, 2)]))
                if n == 0 or i >= len(toks) or toks[i].isalpha(): break
                if c == "M": c, n = "L", 2
        out.append(cmds)
    return out
CAP_UNITS = 1490 * 0.06103516          # the capital's height on the artboard ("F", "I", "D", "E")
M_WORD = 10 / CAP_UNITS                # cap height 10 units
FERRIS, IDEA = glyphs(WORD_FERRIS), glyphs(WORD_IDEA)
INK_X0 = min(x for g in FERRIS + IDEA for _, pts in g for x, _ in pts if x is not None)
def word_d(gs):
    d = []
    for g in gs:
        for c, pts in g:
            if c == "Z": d.append("Z"); continue
            xy = []
            for x, y in pts:
                if x is not None: xy.append(f"{round(2 + (x - INK_X0) * M_WORD, 3):g}")
                if y is not None: xy.append(f"{round(21 - (226 - y) * M_WORD, 3):g}")
            d.append(c + " ".join(xy))
    return "".join(d)
WORD_D = (word_d(FERRIS), word_d(IDEA))
def word_paths(fg, acc):
    return f'<path d="{WORD_D[0]}" fill="{fg}"/><path d="{WORD_D[1]}" fill="{acc}"/>'
write("idecraft-symbol-dark.svg",
      f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="100%" height="100%" fill="none">'
      f'<rect width="32" height="32" fill="{DARK}"/><path d="{crab_abs(2, 6.5)}" fill="{FG}"/>'
      f'<path d="{cursor_abs(2, 6.5)}" fill="{CYAN}"/></svg>')
write("idecraft-wordmark.svg",
      f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 76.5 32" width="100%" height="100%" fill="none">'
      f'{word_paths(FG, CYAN)}</svg>')
write("idecraft-horizontal.svg",
      f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 109 32" width="100%" height="100%" fill="none">'
      f'<path d="{crab_abs(2, 6.5)}" fill="{FG}"/><path d="{cursor_abs(2, 6.5)}" fill="{CYAN}"/>'
      f'<g transform="translate(32.5, 0)">{word_paths(FG, CYAN)}</g></svg>')
print("ok", W, H)

