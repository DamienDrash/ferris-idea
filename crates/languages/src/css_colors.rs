//! Colours written in stylesheets, for the swatches in the editor's gutter and the MCP tool
//! `document_colors` (`SPEC-0042` v27 behaviour 48): hex colours, `rgb()`/`rgba()`,
//! `hsl()`/`hsla()` and the named colours of CSS Color Level 4, in the values of declarations of
//! CSS, SCSS and Less files and of an HTML file's `<style>` blocks and `style` attributes.
//!
//! Ferris IDEA reads them itself, without a language server: the swatches are there as soon as the
//! file is open and change with every edit. Only declaration values are read: a selector such as
//! `.red` or `#fff`, a comment, a string, a `url(…)` and SCSS or Less variables (`$red`, `@red`)
//! never give a colour.

/// Files longer than this many bytes are not read (the editor's limit for syntax trees).
pub const MAX_BYTES: usize = 2 * 1024 * 1024;

/// A colour in a text: 0-based line, char columns of its text on that line, the text as written
/// and its value as `0xRRGGBBAA`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DocumentColor {
    /// 0-based line.
    pub line: usize,
    /// Char column where the colour's text starts.
    pub start: usize,
    /// Char column after the colour's text.
    pub end: usize,
    /// The colour as written (`red`, `#f00`, `rgb(0 0 255)`).
    pub text: String,
    /// The value as `0xRRGGBBAA`.
    pub rgba: u32,
}

impl DocumentColor {
    /// `#rrggbb`, or `#rrggbbaa` when the colour is not opaque.
    pub fn hex(&self) -> String {
        hex(self.rgba)
    }
}

/// `#rrggbb`, or `#rrggbbaa` when `rgba` is not opaque.
pub fn hex(rgba: u32) -> String {
    if rgba & 0xff == 0xff {
        format!("#{:06x}", rgba >> 8)
    } else {
        format!("#{rgba:08x}")
    }
}

/// Whether the language (an LSP id) has colours read by [`document_colors`].
pub fn has_colors(language_id: &str) -> bool {
    matches!(language_id, "css" | "scss" | "less" | "html")
}

/// The colours of `text` in the language with LSP id `language_id`, in text order; empty for
/// another language and for a text longer than [`MAX_BYTES`].
pub fn document_colors(language_id: &str, text: &str) -> Vec<DocumentColor> {
    if text.len() > MAX_BYTES {
        return Vec::new();
    }
    let mut found: Vec<(usize, usize, u32)> = Vec::new();
    match language_id {
        "css" => stylesheet(text, 0, text.len(), false, &mut found),
        "scss" | "less" => stylesheet(text, 0, text.len(), true, &mut found),
        "html" => html(text, &mut found),
        _ => return Vec::new(),
    }
    found.sort_by_key(|f| f.0);
    found.dedup_by_key(|f| f.0);
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    found
        .into_iter()
        .map(|(from, to, rgba)| {
            let line = starts.partition_point(|&s| s <= from) - 1;
            let start = text[starts[line]..from].chars().count();
            let shown = &text[from..to];
            DocumentColor {
                line,
                start,
                end: start + shown.chars().count(),
                text: shown.to_owned(),
                rgba,
            }
        })
        .collect()
}

fn ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b >= 0x80
}

/// The index after the string that starts with the quote at `i`.
fn skip_string(s: &[u8], i: usize, end: usize) -> usize {
    let quote = s[i];
    let mut j = i + 1;
    while j < end {
        match s[j] {
            b'\\' => j += 2,
            b'\n' => return j,
            c if c == quote => return j + 1,
            _ => j += 1,
        }
    }
    end
}

/// The index after the block comment that starts at `i`.
fn skip_comment(s: &[u8], i: usize, end: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < end {
        if s[j] == b'*' && s[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    end
}

/// Properties whose values are names, not colours: a named colour there is no colour
/// (`font-family: Tan`).
const NAME_PROPERTIES: &[&str] = &[
    "font",
    "font-family",
    "animation",
    "animation-name",
    "grid-area",
    "grid-row",
    "grid-column",
    "grid-template-areas",
    "counter-reset",
    "counter-increment",
    "list-style-type",
    "transition-property",
    "will-change",
    "content",
];

/// The declarations of a stylesheet between `from` and `to` of `text`: a value starts after the
/// `:` of a declaration (inside a block, or of a SCSS/Less variable anywhere) and ends at `;` or
/// `}`; one that ends at `{` was a selector (`a:hover {`) and is not read.
fn stylesheet(
    text: &str,
    from: usize,
    to: usize,
    line_comments: bool,
    out: &mut Vec<(usize, usize, u32)>,
) {
    let s = text.as_bytes();
    let mut depth = 0usize;
    // The open value: where it starts and whether named colours count in it.
    let mut value: Option<(usize, bool)> = None;
    let mut i = from;
    while i < to {
        let b = s[i];
        if b == b'/' && s.get(i + 1) == Some(&b'*') {
            i = skip_comment(s, i, to);
            continue;
        }
        if line_comments
            && b == b'/'
            && s.get(i + 1) == Some(&b'/')
            && (i == from
                || s[i - 1].is_ascii_whitespace()
                || matches!(s[i - 1], b';' | b'{' | b'}'))
        {
            while i < to && s[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => {
                i = skip_string(s, i, to);
                continue;
            }
            b':' if value.is_none()
                && s.get(i + 1) != Some(&b':')
                && (i == from || s[i - 1] != b':') =>
            {
                let mut k = i;
                while k > from && s[k - 1] == b' ' {
                    k -= 1;
                }
                let name_end = k;
                while k > from && ident_char(s[k - 1]) {
                    k -= 1;
                }
                let variable = k > from && matches!(s[k - 1], b'$' | b'@');
                if (depth > 0 || variable) && name_end > k {
                    let name = text[k..name_end].to_ascii_lowercase();
                    let named = variable || !NAME_PROPERTIES.contains(&name.as_str());
                    value = Some((i + 1, named));
                }
            }
            b'#' if s.get(i + 1) == Some(&b'{') => {
                // SCSS interpolation `#{…}` inside a value or a selector.
                let mut j = i + 2;
                while j < to && s[j] != b'}' {
                    j += 1;
                }
                i = j + 1;
                continue;
            }
            b';' | b'}' => {
                if let Some((start, named)) = value.take() {
                    colors_in_value(text, start, i, named, out);
                }
                if b == b'}' {
                    depth = depth.saturating_sub(1);
                }
            }
            b'{' => {
                value = None;
                depth += 1;
            }
            _ => {}
        }
        i += 1;
    }
    if let Some((start, named)) = value {
        colors_in_value(text, start, to, named, out);
    }
}

/// The colours in the value between `from` and `to` of `text`.
fn colors_in_value(
    text: &str,
    from: usize,
    to: usize,
    named: bool,
    out: &mut Vec<(usize, usize, u32)>,
) {
    let s = text.as_bytes();
    let mut i = from;
    while i < to {
        let b = s[i];
        if b == b'/' && s.get(i + 1) == Some(&b'*') {
            i = skip_comment(s, i, to);
            continue;
        }
        if b == b'"' || b == b'\'' {
            i = skip_string(s, i, to);
            continue;
        }
        let before = (i > from).then(|| s[i - 1]);
        let free =
            before.is_none_or(|c| !ident_char(c) && !matches!(c, b'$' | b'@' | b'.' | b'#' | b'%'));
        if b == b'#' && free {
            let mut j = i + 1;
            while j < to && s[j].is_ascii_hexdigit() {
                j += 1;
            }
            if !(j < to && ident_char(s[j]))
                && let Some(rgba) = parse_hex(&text[i + 1..j])
            {
                out.push((i, j, rgba));
            }
            i = j.max(i + 1);
            continue;
        }
        if (b.is_ascii_alphabetic() || b == b'-') && free {
            let mut j = i;
            while j < to && ident_char(s[j]) {
                j += 1;
            }
            let word = text[i..j].to_ascii_lowercase();
            if j < to && s[j] == b'(' {
                let close = text[j..to].find(')').map(|k| j + k);
                match (word.as_str(), close) {
                    ("url", Some(c)) => {
                        i = c + 1;
                        continue;
                    }
                    ("url", None) => return,
                    ("rgb" | "rgba" | "hsl" | "hsla", Some(c)) => {
                        if let Some(rgba) = parse_function(&word, &text[j + 1..c]) {
                            out.push((i, c + 1, rgba));
                        }
                        i = c + 1;
                        continue;
                    }
                    // Other functions (`var(`, `calc(`, `darken(`) are read inside.
                    _ => {
                        i = j + 1;
                        continue;
                    }
                }
            }
            if named && let Some(rgba) = named_color(&word) {
                out.push((i, j, rgba));
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

/// The `<style>` blocks and `style` attributes of an HTML text.
fn html(text: &str, out: &mut Vec<(usize, usize, u32)>) {
    let s = text.as_bytes();
    let lower = text.to_ascii_lowercase();
    let low = lower.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if low[i..].starts_with(b"<!--") {
            i = lower[i..].find("-->").map_or(s.len(), |k| i + k + 3);
            continue;
        }
        if s[i] != b'<' {
            i += 1;
            continue;
        }
        // A tag: its attributes up to `>`, strings skipped.
        let name_end = (i + 1..s.len())
            .find(|&k| !s[k].is_ascii_alphanumeric())
            .unwrap_or(s.len());
        let name = &lower[i + 1..name_end];
        let mut j = name_end;
        while j < s.len() && s[j] != b'>' {
            if matches!(s[j], b'"' | b'\'') {
                j = skip_string_any_line(s, j);
                continue;
            }
            if low[j..].starts_with(b"style")
                && s[j - 1].is_ascii_whitespace()
                && let Some((open, close)) = attribute_value(s, j + 5)
            {
                stylesheet_declarations(text, open, close, out);
                j = close + 1;
                continue;
            }
            j += 1;
        }
        let after = (j + 1).min(s.len());
        if name == "style" {
            let end = lower[after..]
                .find("</style")
                .map_or(s.len(), |k| after + k);
            stylesheet(text, after, end, false, out);
            i = end;
        } else if name == "script" {
            i = lower[after..]
                .find("</script")
                .map_or(s.len(), |k| after + k);
        } else {
            i = after;
        }
    }
}

/// The index after a quoted attribute value that may span lines.
fn skip_string_any_line(s: &[u8], i: usize) -> usize {
    let quote = s[i];
    (i + 1..s.len())
        .find(|&k| s[k] == quote)
        .map_or(s.len(), |k| k + 1)
}

/// The quoted value of an attribute whose name ends before `i`: the indices inside the quotes.
fn attribute_value(s: &[u8], mut i: usize) -> Option<(usize, usize)> {
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    if s.get(i) != Some(&b'=') {
        return None;
    }
    i += 1;
    while i < s.len() && s[i].is_ascii_whitespace() {
        i += 1;
    }
    let quote = *s.get(i).filter(|q| matches!(q, b'"' | b'\''))?;
    let close = (i + 1..s.len()).find(|&k| s[k] == quote)?;
    Some((i + 1, close))
}

/// Declarations without braces (a `style` attribute) between `from` and `to`.
fn stylesheet_declarations(text: &str, from: usize, to: usize, out: &mut Vec<(usize, usize, u32)>) {
    // Read as the inside of a block: the same rules, with the depth one.
    let s = text.as_bytes();
    let mut start = from;
    for k in from..=to {
        if k == to || s[k] == b';' {
            if let Some(colon) = text[start..k].find(':') {
                let name = text[start..start + colon].trim().to_ascii_lowercase();
                let named = !NAME_PROPERTIES.contains(&name.as_str());
                colors_in_value(text, start + colon + 1, k, named, out);
            }
            start = k + 1;
        }
    }
}

/// `rgb`, `rgba`, `hsl` or `hsla` with the text between its parentheses.
fn parse_function(name: &str, args: &str) -> Option<u32> {
    let (main, alpha) = match args.split_once('/') {
        Some((m, a)) => (m, Some(a.trim())),
        None => (args, None),
    };
    let mut parts: Vec<&str> = main
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .collect();
    let alpha = match (alpha, parts.len()) {
        (Some(a), 3) => Some(a),
        (None, 4) => parts.pop(),
        (None, 3) => None,
        _ => return None,
    };
    let a = match alpha {
        Some(a) => unit_or_percent(a)?,
        None => 1.0,
    };
    let (r, g, b) = if name.starts_with("rgb") {
        let channel = |p: &str| -> Option<f64> {
            match p.strip_suffix('%') {
                Some(n) => Some(n.parse::<f64>().ok()? / 100.0),
                None => Some(p.parse::<f64>().ok()? / 255.0),
            }
        };
        (channel(parts[0])?, channel(parts[1])?, channel(parts[2])?)
    } else {
        let hue = parse_hue(parts[0])?;
        let percent = |p: &str| -> Option<f64> {
            Some(p.strip_suffix('%').unwrap_or(p).parse::<f64>().ok()? / 100.0)
        };
        hsl_to_rgb(hue, percent(parts[1])?, percent(parts[2])?)
    };
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    Some(byte(r) << 24 | byte(g) << 16 | byte(b) << 8 | byte(a))
}

/// A number from 0 to 1, or a percentage.
fn unit_or_percent(p: &str) -> Option<f64> {
    match p.strip_suffix('%') {
        Some(n) => Some(n.parse::<f64>().ok()? / 100.0),
        None => p.parse::<f64>().ok(),
    }
}

/// A hue in degrees: a plain number or with `deg`, `rad`, `grad` or `turn`.
fn parse_hue(p: &str) -> Option<f64> {
    let p = p.to_ascii_lowercase();
    let (number, factor) = if let Some(n) = p.strip_suffix("deg") {
        (n, 1.0)
    } else if let Some(n) = p.strip_suffix("grad") {
        (n, 0.9)
    } else if let Some(n) = p.strip_suffix("rad") {
        (n, 180.0 / std::f64::consts::PI)
    } else if let Some(n) = p.strip_suffix("turn") {
        (n, 360.0)
    } else {
        (p.as_str(), 1.0)
    };
    Some(number.parse::<f64>().ok()? * factor)
}

/// HSL to red, green and blue from 0 to 1 (CSS Color Level 4).
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (f64, f64, f64) {
    let s = saturation.clamp(0.0, 1.0);
    let l = lightness.clamp(0.0, 1.0);
    let h = hue.rem_euclid(360.0);
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let a = s * l.min(1.0 - l);
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    (f(0.0), f(8.0), f(4.0))
}

/// The hex digits after `#`: three, four, six or eight.
fn parse_hex(digits: &str) -> Option<u32> {
    let wide = |d: &str| -> Option<String> { Some(d.chars().flat_map(|c| [c, c]).collect()) };
    let full = match digits.len() {
        3 | 4 => wide(digits)?,
        6 | 8 => digits.to_owned(),
        _ => return None,
    };
    let v = u32::from_str_radix(&full, 16).ok()?;
    Some(if full.len() == 6 { v << 8 | 0xff } else { v })
}

/// The value of a colour written as text (`red`, `#f00`, `rgb(255 0 0)`), as `0xRRGGBBAA`.
pub fn parse_color(text: &str) -> Option<u32> {
    let t = text.trim();
    if let Some(d) = t.strip_prefix('#') {
        return parse_hex(d);
    }
    if let Some(open) = t.find('(')
        && t.ends_with(')')
    {
        let name = t[..open].to_ascii_lowercase();
        return matches!(name.as_str(), "rgb" | "rgba" | "hsl" | "hsla")
            .then(|| parse_function(&name, &t[open + 1..t.len() - 1]))
            .flatten();
    }
    named_color(&t.to_ascii_lowercase())
}

/// The named colours of CSS Color Level 4 (`transparent` and `currentcolor` are no swatch).
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// The value of a named colour (lower case).
fn named_color(name: &str) -> Option<u32> {
    NAMED
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| NAMED[i].1 << 8 | 0xff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(language: &str, text: &str) -> Vec<(usize, usize, String, String)> {
        document_colors(language, text)
            .into_iter()
            .map(|c| (c.line, c.start, c.text.clone(), c.hex()))
            .collect()
    }

    fn t(line: usize, start: usize, text: &str, hex: &str) -> (usize, usize, String, String) {
        (line, start, text.to_owned(), hex.to_owned())
    }

    /// SPEC-0042 AK-52 (v27): the forms of a colour and their values.
    #[test]
    fn colour_values() {
        assert_eq!(NAMED.len(), 148);
        assert!(NAMED.windows(2).all(|w| w[0].0 < w[1].0), "sorted");
        assert_eq!(parse_color("red"), Some(0xff0000ff));
        assert_eq!(parse_color("RebeccaPurple"), Some(0x663399ff));
        assert_eq!(parse_color("#f00"), Some(0xff0000ff));
        assert_eq!(parse_color("#f008"), Some(0xff000088));
        assert_eq!(parse_color("#00ff00"), Some(0x00ff00ff));
        assert_eq!(parse_color("#0000ff80"), Some(0x0000ff80));
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("rgb(255, 0, 0)"), Some(0xff0000ff));
        assert_eq!(parse_color("rgba(0, 0, 255, 0.5)"), Some(0x0000ff80));
        assert_eq!(parse_color("rgb(0 128 0 / 50%)"), Some(0x00800080));
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some(0xff0000ff));
        assert_eq!(parse_color("hsl(120, 100%, 50%)"), Some(0x00ff00ff));
        assert_eq!(parse_color("hsl(240deg 100% 50%)"), Some(0x0000ffff));
        assert_eq!(parse_color("hsla(0.5turn, 100%, 25%, 1)"), Some(0x008080ff));
        assert_eq!(parse_color("hsl(0, 0%, 100%)"), Some(0xffffffff));
        assert_eq!(parse_color("rgb(var(--r), 0, 0)"), None);
        assert_eq!(parse_color("transparent"), None);
        assert_eq!(parse_color("currentColor"), None);
        assert_eq!(hex(0xff0000ff), "#ff0000");
        assert_eq!(hex(0x0000ff80), "#0000ff80");
    }

    /// SPEC-0042 AK-52 (v27): only declaration values give colours.
    #[test]
    fn colours_of_a_stylesheet() {
        let css = ".card {\n  padding: 4px;\n  color: red;\n}\n#hello, .red:hover {\n  background: #00f url(img/red.png) no-repeat;\n  border: 1px solid rgb(0 128 0); /* blue */\n  font-family: Tan, sans-serif;\n  box-shadow: 0 0 2px hsl(0, 0%, 0%), 0 0 1px white;\n  content: \"red\";\n}\n@media (max-width: 600px) { a:hover { color: Blue } }\n";
        assert_eq!(
            found("css", css),
            [
                t(2, 9, "red", "#ff0000"),
                t(5, 14, "#00f", "#0000ff"),
                t(6, 20, "rgb(0 128 0)", "#008000"),
                t(8, 22, "hsl(0, 0%, 0%)", "#000000"),
                t(8, 46, "white", "#ffffff"),
                t(11, 45, "Blue", "#0000ff"),
            ]
        );
        assert_eq!(found("css", "a { color: #abcdeg; border-color: #ab }"), []);
        assert_eq!(found("rust", "let red = 1;"), []);
    }

    /// SPEC-0042 AK-52 (v27): SCSS and Less variables, line comments and interpolation.
    #[test]
    fn colours_of_scss_and_less() {
        let scss = "$brand: #336699; // red\n$red: 1px;\n.a {\n  color: $brand;\n  &:hover { color: darken(orange, 10%); }\n  margin: #{$red};\n}\n";
        assert_eq!(
            found("scss", scss),
            [t(0, 8, "#336699", "#336699"), t(4, 26, "orange", "#ffa500")]
        );
        let less = "@accent: teal;\n.b { color: @accent; background: url(//cdn/red.png) }\n";
        assert_eq!(found("less", less), [t(0, 9, "teal", "#008080")]);
    }

    /// SPEC-0042 AK-52 (v27): an HTML file's `<style>` blocks and `style` attributes.
    #[test]
    fn colours_of_html() {
        let html = "<!-- <p style=\"color: red\"> -->\n<style>\n  .x { color: green; }\n</style>\n<p class=\"red\" style=\"color: #fff; font-family: Navy\">red</p>\n<script>let s = 'style=\"color: blue\"';</script>\n<div STYLE='background:hsla(120,100%,50%,.5)'></div>\n";
        assert_eq!(
            found("html", html),
            [
                t(2, 14, "green", "#008000"),
                t(4, 29, "#fff", "#ffffff"),
                t(6, 23, "hsla(120,100%,50%,.5)", "#00ff0080"),
            ]
        );
    }

    /// Columns count chars, not bytes.
    #[test]
    fn columns_count_chars() {
        assert_eq!(
            found("css", "/* ü */ a { color: lime }"),
            [t(0, 19, "lime", "#00ff00")]
        );
    }
}
