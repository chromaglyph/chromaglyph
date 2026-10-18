//! Built-in HTML spec: tags via the markup state machine (`<` opens a
//! name → attrs → values machine, line-scoped like everything else —
//! an unclosed tag dies at the newline), `<!-- -->` comments and
//! `&...;` entities as spans. Text between tags stays plain, exactly
//! like an editor renders it. `<script>`/`<style>` embedded languages
//! are not modeled (that would need cross-line state); `<3` and a lone
//! `< b` stay plain — the opener requires an identifier lookahead.

use crate::generic::{LanguageSpec, SpanKind, TagKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "html",
        keywords: &[],
        strings: &[],
        spans: &[
            SpanKind {
                open: "<!--",
                close: "-->",
                scope: "comment.block.html",
            },
            // entities: `&amp;` `&#39;` — recognized in text only (inside
            // a tag or a quoted value the tag/string body wins, matching
            // the engine's rule order)
            SpanKind {
                open: "&",
                close: ";",
                scope: "constant.character.entity.html",
            },
        ],
        blocks: &[],
        tags: &[TagKind {
            open: "<",
            close: ">",
            self_close: "/>",
            value_quotes: &['"', '\''],
            equals: '=',
            name_scope: "entity.name.tag.html",
            attr_scope: "entity.other.attribute-name.html",
            value_scope: "string.quoted.double.html",
        }],
        numbers: false,
        number_scope: "constant.numeric.html",
        // `-` for data-attributes and custom elements, `:` for
        // namespaced attributes (xlink:, xml:)
        ident_extra: &['-', ':'],
    }
}

/// Curated oracle samples: every state transition order, unterminated
/// states, delimiter soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "<div>text</div>",
        "<p class=\"note\">hello &amp; bye</p>",
        "<img src='a.png' alt=\"x\"/>",
        "<!-- comment --> <br>",
        "a < b and c > d",
        "<input type=checkbox disabled>",
        "<my-widget data-x=\"1\" data-y='2'>",
        "<a href=\"https://x.io/?a=1&amp;b=2\">link</a>",
        "<div\n  class=\"multi line\">",
        "<span title='has \"dq\" inside'>q</span>",
        "< unclosed <div class=\"x\"",
        "</closing> </ div> <3> <>",
        "text with <b>bold</b> and &lt;esc&gt;",
        "<div =x b=\"c\">",
        "<div a<b=\"c\">",
        "plain \"quotes\" in text, not values",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
