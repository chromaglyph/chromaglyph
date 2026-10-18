//! Built-in CSS spec: the property-sheet (`BlockKind`) construct.
//! Selector words color after any boundary or `.`/`:` (pseudo-classes
//! color as selectors, their names plain); property names are keywords
//! that fire only when a `:` follows (`a:hover` never matches — `hover`
//! is not a property and the colon-check is per word); values ride in
//! an uncolored regime until `;` `{` `}` with numbers, strings and
//! comments colored by the ordinary machinery. Multi-line rule blocks
//! cannot carry state across lines — continuation property lines work
//! through the property keywords' colon lookahead, not block state.

use crate::generic::{BlockKind, KeywordGroup, LanguageSpec, SpanKind, StringKind};

pub fn spec() -> LanguageSpec {
    LanguageSpec {
        name: "css",
        keywords: &[
            KeywordGroup {
                // standard property names — these fire only when a ':'
                // follows, so a selector word that happens to spell a
                // property (`a:hover`) stays a selector
                scope: "support.type.property-name.css",
                followed_by: Some(':'),
                value: false,
                words: PROPERTIES,
            },
            KeywordGroup {
                scope: "keyword.control.at-rule.css",
                followed_by: None,
                value: false,
                words: &[
                    "charset",
                    "container",
                    "counter-style",
                    "document",
                    "font-face",
                    "font-feature-values",
                    "import",
                    "keyframes",
                    "layer",
                    "media",
                    "namespace",
                    "page",
                    "property",
                    "scope",
                    "supports",
                ],
            },
            KeywordGroup {
                scope: "keyword.other.important.css",
                followed_by: None,
                value: true,
                words: &["important"],
            },
            KeywordGroup {
                scope: "support.constant.property-value.css",
                followed_by: None,
                value: true,
                words: &["inherit", "initial", "revert", "unset"],
            },
        ],
        strings: &[
            StringKind {
                quote: '"',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.double.css",
            },
            StringKind {
                quote: '\'',
                escapes: true,
                interpolation: None,
                scope: "string.quoted.single.css",
            },
        ],
        spans: &[SpanKind {
            open: "/*",
            close: "*/",
            scope: "comment.block.css",
        }],
        tags: &[],
        blocks: &[BlockKind {
            colon: ':',
            open: '{',
            ends: &[';', '{', '}'],
            anchors: &['.', ':'],
            sel_scope: "entity.name.tag.css",
        }],
        numbers: true,
        number_scope: "constant.numeric.css",
        ident_extra: &['-', '@'],
    }
}

#[rustfmt::skip]
const PROPERTIES: &[&str] = &[
    "align-content", "align-items", "align-self", "all", "animation", "animation-delay",
    "animation-direction", "animation-duration", "animation-fill-mode", "animation-iteration-count",
    "animation-name", "animation-play-state", "animation-timing-function", "aspect-ratio",
    "backdrop-filter", "backface-visibility", "background", "background-attachment",
    "background-blend-mode", "background-clip", "background-color", "background-image",
    "background-origin", "background-position", "background-repeat", "background-size",
    "block-size", "border", "border-block", "border-block-color", "border-block-end",
    "border-block-end-color", "border-block-end-style", "border-block-end-width",
    "border-block-start", "border-block-start-color", "border-block-start-style",
    "border-block-start-width", "border-block-style", "border-block-width", "border-bottom",
    "border-bottom-color", "border-bottom-left-radius", "border-bottom-right-radius",
    "border-bottom-style", "border-bottom-width", "border-collapse", "border-color",
    "border-end-end-radius", "border-end-start-radius", "border-image", "border-image-outset",
    "border-image-repeat", "border-image-slice", "border-image-source", "border-image-width",
    "border-inline", "border-inline-color", "border-inline-end", "border-inline-end-color",
    "border-inline-end-style", "border-inline-end-width", "border-inline-start",
    "border-inline-start-color", "border-inline-start-style", "border-inline-start-width",
    "border-inline-style", "border-inline-width", "border-left", "border-left-color",
    "border-left-style", "border-left-width", "border-radius", "border-right",
    "border-right-color", "border-right-style", "border-right-width", "border-spacing",
    "border-start-end-radius", "border-start-start-radius", "border-style", "border-top",
    "border-top-color", "border-top-left-radius", "border-top-right-radius", "border-top-style",
    "border-top-width", "border-width", "bottom", "box-decoration-break", "box-shadow",
    "box-sizing", "break-after", "break-before", "break-inside", "caption-side", "caret-color",
    "clear", "clip", "clip-path", "clip-rule", "color", "color-scheme", "column-count",
    "column-fill", "column-gap", "column-rule", "column-rule-color", "column-rule-style",
    "column-rule-width", "column-span", "column-width", "columns", "contain",
    "content", "counter-increment", "counter-reset", "cursor", "direction", "display",
    "empty-cells", "filter", "flex", "flex-basis", "flex-direction", "flex-flow", "flex-grow",
    "flex-shrink", "flex-wrap", "float", "font", "font-family", "font-feature-settings",
    "font-kerning", "font-language-override", "font-optical-sizing", "font-size",
    "font-size-adjust", "font-stretch", "font-style", "font-synthesis", "font-variant",
    "font-variant-alternates", "font-variant-caps", "font-variant-east-asian",
    "font-variant-ligatures", "font-variant-numeric", "font-variant-position", "font-weight",
    "gap", "grid", "grid-area", "grid-auto-columns", "grid-auto-flow", "grid-auto-rows",
    "grid-column", "grid-column-end", "grid-column-start", "grid-row", "grid-row-end",
    "grid-row-start", "grid-template", "grid-template-areas", "grid-template-columns",
    "grid-template-rows", "hanging-punctuation", "height", "hyphens", "image-orientation",
    "image-rendering", "inline-size", "inset", "inset-block", "inset-block-end",
    "inset-block-start", "inset-inline", "inset-inline-end", "inset-inline-start",
    "isolation", "justify-content", "justify-items", "justify-self", "left",
    "letter-spacing", "line-break", "line-height", "line-height-step", "list-style",
    "list-style-image", "list-style-position", "list-style-type", "margin",
    "margin-block", "margin-block-end", "margin-block-start", "margin-bottom",
    "margin-inline", "margin-inline-end", "margin-inline-start", "margin-left",
    "margin-right", "margin-top", "mask", "mask-border", "mask-border-mode",
    "mask-border-outset", "mask-border-repeat", "mask-border-slice", "mask-border-source",
    "mask-border-width", "mask-clip", "mask-composite", "mask-image", "mask-mode",
    "mask-origin", "mask-position", "mask-repeat", "mask-size", "mask-type", "max-block-size",
    "max-height", "max-inline-size", "max-width", "min-block-size", "min-height",
    "min-inline-size", "min-width", "mix-blend-mode", "object-fit", "object-position",
    "offset", "offset-anchor", "offset-distance", "offset-path", "offset-rotate",
    "opacity", "order", "orphans", "outline", "outline-color", "outline-offset",
    "outline-style", "outline-width", "overflow", "overflow-anchor", "overflow-block",
    "overflow-inline", "overflow-wrap", "overflow-x", "overflow-y", "overscroll-behavior",
    "padding", "padding-block", "padding-block-end", "padding-block-start", "padding-bottom",
    "padding-inline", "padding-inline-end", "padding-inline-start", "padding-left",
    "padding-right", "padding-top", "page-break-after", "page-break-before",
    "page-break-inside", "paint-order", "perspective", "perspective-origin",
    "place-content", "place-items", "place-self", "pointer-events", "position", "quotes",
    "resize", "right", "rotate", "row-gap", "ruby-align", "ruby-merge", "ruby-position",
    "scale", "scroll-behavior", "scroll-margin", "scroll-margin-block",
    "scroll-margin-block-end", "scroll-margin-block-start", "scroll-margin-bottom",
    "scroll-margin-inline", "scroll-margin-inline-end", "scroll-margin-inline-start",
    "scroll-margin-left", "scroll-margin-right", "scroll-margin-top", "scroll-padding",
    "scroll-padding-block", "scroll-padding-block-end", "scroll-padding-block-start",
    "scroll-padding-bottom", "scroll-padding-inline", "scroll-padding-inline-end",
    "scroll-padding-inline-start", "scroll-padding-left", "scroll-padding-right",
    "scroll-padding-top", "scroll-snap-align", "scroll-snap-stop", "scroll-snap-type",
    "scrollbar-color", "scrollbar-gutter", "scrollbar-width", "shape-image-threshold",
    "shape-margin", "shape-outside", "tab-size", "table-layout", "text-align",
    "text-align-last", "text-combine-upright", "text-decoration", "text-decoration-color",
    "text-decoration-line", "text-decoration-skip-ink", "text-decoration-style",
    "text-decoration-thickness", "text-emphasis", "text-emphasis-color",
    "text-emphasis-position", "text-emphasis-style", "text-indent", "text-justify",
    "text-orientation", "text-overflow", "text-rendering", "text-shadow",
    "text-transform", "text-underline-offset", "text-underline-position", "text-wrap",
    "top", "touch-action", "transform", "transform-box", "transform-origin",
    "transform-style", "transition", "transition-delay", "transition-duration",
    "transition-property", "transition-timing-function", "translate", "unicode-bidi",
    "user-select", "vertical-align", "visibility", "white-space", "widows", "width",
    "will-change", "word-break", "word-spacing", "word-wrap", "writing-mode", "z-index",
    "zoom",
];

/// Curated oracle samples: every state transition order, unterminated
/// states, delimiter soup (plus whatever the spec-derived random
/// assembly in `oracle.rs` adds on top).
pub(in crate::generic) fn samples() -> Vec<String> {
    [
        "a { color: red; }",
        ".card > .title { margin: 0 auto; }",
        "#main p, .item:hover { display: flex; }",
        "/* comment */ .x { color: blue }",
        "a::before { content: \"x\"; }",
        "@media (min-width: 600px) { .y { padding: 4px 8px; } }",
        "@import url(\"theme.css\");",
        "div { background: url('a.png') no-repeat; }",
        "p { font: 12px/1.5 sans-serif; }",
        ":root { --gap: 8px; }",
        ".btn { border-radius: 9999px !important; }",
        "width: 50%;",
        "a:hover span { color: inherit; }",
        ".a .b > .c + .d ~ .e { top: 0; }",
        "background:hover",
        "grid-template-columns: repeat(3, 1fr);",
        "x { y: z",
        "sel { prop:value;compact:true }",
        "\ncolor at_line_start\n42 at_line_start",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
