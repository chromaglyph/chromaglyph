//! GlyphPlan → a write-fonts `Gsub` table.
//!
//! Layout of the generated GSUB (lookup indices are the application order
//! within `calt` — shapers apply lookups by increasing index):
//! - Lookup 0..P-1 (P = palette size): `SingleSubst` format 2, one per
//!   palette color, mapping every colored base glyph to its colored copy.
//!   These are *nested-only* lookups: they are not referenced by any feature,
//!   so they never fire outside a contextual rule.
//! - One nested lookup per ligature site: a `SingleSubst` for 1:1 restores,
//!   a `LigatureSubst` for ligatures (also nested-only). One lookup per
//!   site keeps sites with identical components but different outputs
//!   distinguishable — the chain rule picks the exact substitution.
//! - The ligature-restore chain lookup: every ligature rule, grouped into
//!   as many chain-context format-2 subtables as needed. It runs FIRST so
//!   the coloring rules see the restored (ligated) stream.
//! - The coloring chain lookup: same construction, runs after the restore.
//!
//! Class defs put a base glyph and all of its colored copies into the same
//! class, which makes backtrack/lookahead matching insensitive to whether
//! neighboring glyphs have already been substituted. When a lookup's rules
//! do not fit one subtable (u16 offsets cap a subtable at 64 KiB), it
//! becomes an extension lookup (type 7) whose 32-bit offsets allow a GSUB
//! of any practical size. Both chain lookups are registered under `calt`
//! (applied by default by HarfBuzz and browsers), `DFLT` and `latn` default
//! langsys.

use std::collections::HashMap;

use write_fonts::tables::gsub::{
    ExtensionSubstFormat1, ExtensionSubtable, Gsub, Ligature, LigatureSet, LigatureSubstFormat1,
    SingleSubst, SubstitutionChainContext, SubstitutionLookup,
};
use write_fonts::tables::layout::builders::CoverageTableBuilder;
use write_fonts::tables::layout::{
    ChainedClassSequenceRule, ChainedClassSequenceRuleSet, ChainedSequenceContext,
    ChainedSequenceContextFormat2, ClassDef, ClassDefFormat2, ClassRangeRecord, Feature,
    FeatureList, FeatureRecord, LangSys, Lookup, LookupFlag, LookupList, Script, ScriptList,
    ScriptRecord, SequenceLookupRecord,
};
use write_fonts::types::{GlyphId16, Tag};

use crate::compile::GlyphPlan;

/// Conservative per-subtable budget: subtable-internal offsets are u16.
const MAX_SUBTABLE_BYTES: usize = 24_000;

/// Build the GSUB table; `None` when the plan colors nothing and restores
/// no ligatures (font keeps no GSUB and no color tables).
pub fn build_gsub(plan: &GlyphPlan) -> Option<Gsub> {
    if plan.lig_rules.is_empty() && (plan.rules.is_empty() || plan.alt_glyphs.is_empty()) {
        return None;
    }
    let num_palettes = plan.slots.len() as u16; // one nested single-subst lookup per slot

    // ---- nested single-substitution lookups, one per palette entry --------
    let mut pairs_by_pal: HashMap<u16, Vec<(u16, u16)>> = HashMap::new();
    for &(base, pal, alt) in &plan.alt_glyphs {
        pairs_by_pal.entry(pal).or_default().push((base, alt));
    }
    let mut lookups: Vec<SubstitutionLookup> = Vec::new();
    for pal in 0..num_palettes {
        let mut pairs = pairs_by_pal.remove(&pal).unwrap_or_default();
        pairs.sort_unstable();
        let mut coverage = CoverageTableBuilder::default();
        let substitutes: Vec<GlyphId16> = pairs
            .iter()
            .map(|&(base, alt)| {
                coverage.add(GlyphId16::new(base));
                GlyphId16::new(alt)
            })
            .collect();
        let single = SingleSubst::format_2(coverage.build(), substitutes);
        lookups.push(SubstitutionLookup::Single(Lookup::new(
            LookupFlag::empty(),
            vec![single],
        )));
    }

    // ---- nested per-site restore lookups ----------------------------------
    // one lookup per distinct site: the chain rule's record picks the exact
    // substitution, so identical components with different outputs stay
    // distinguishable (a shared table could not tell them apart)
    let mut next = num_palettes;
    for site in &plan.lig_sites {
        let lookup = match site.components.as_slice() {
            // 1:1 restore: a contextual single substitution
            [only] => {
                let mut coverage = CoverageTableBuilder::default();
                coverage.add(GlyphId16::new(*only));
                SubstitutionLookup::Single(Lookup::new(
                    LookupFlag::empty(),
                    vec![SingleSubst::format_2(
                        coverage.build(),
                        vec![GlyphId16::new(site.out)],
                    )],
                ))
            }
            // ligature: coverage carries the first component, the Ligature
            // table lists the rest
            [first, rest @ ..] => {
                let mut coverage = CoverageTableBuilder::default();
                coverage.add(GlyphId16::new(*first));
                let ligature = Ligature::new(
                    GlyphId16::new(site.out),
                    rest.iter().map(|&g| GlyphId16::new(g)).collect(),
                );
                SubstitutionLookup::Ligature(Lookup::new(
                    LookupFlag::empty(),
                    vec![LigatureSubstFormat1::new(
                        coverage.build(),
                        vec![LigatureSet::new(vec![ligature])],
                    )],
                ))
            }
            [] => unreachable!("a site has at least one component"),
        };
        lookups.push(lookup);
        next += 1;
    }

    // ---- shared class material -------------------------------------------
    // Classes may lump many glyphs (generic mode partitions the whole
    // charset), so coverage must enumerate every base glyph of a class:
    // the first input glyph's coverage entry selects its class's rule set.
    // In exact mode classes are 1:1 with glyphs, so this is one each.
    let mut class_glyphs: Vec<(u16, u16)> = plan
        .class_of
        .iter()
        .map(|(&gid, &class)| (gid, class))
        .collect();
    class_glyphs.sort_unstable();
    // coverage lists every glyph a rule can START on: base glyphs and
    // injected plain glyphs (the generic-mode newline), never alt copies
    let alt_gids: std::collections::HashSet<u16> =
        plan.alt_glyphs.iter().map(|&(_, _, alt)| alt).collect();
    let glyphs_of_class: HashMap<u16, Vec<u16>> = {
        let mut map: HashMap<u16, Vec<u16>> = HashMap::new();
        for &(gid, class) in &class_glyphs {
            if !alt_gids.contains(&gid) {
                map.entry(class).or_default().push(gid);
            }
        }
        map
    };

    // ---- the two chain lookups, restore first -----------------------------
    let mut feature_lookups: Vec<u16> = Vec::new();
    if !plan.lig_rules.is_empty() {
        let packed: Vec<PackedRule<'_>> = plan
            .lig_rules
            .iter()
            .map(|rule| PackedRule {
                backtrack: &rule.backtrack,
                input: &rule.input,
                lookahead: &rule.lookahead,
                records: vec![SequenceLookupRecord::new(
                    0,
                    num_palettes + rule.site, // the site's nested lookup
                )],
            })
            .collect();
        lookups.push(chain_lookup(&packed, &class_glyphs, &glyphs_of_class));
        feature_lookups.push(next);
        next += 1;
    }

    if !plan.rules.is_empty() {
        let packed: Vec<PackedRule<'_>> = plan
            .rules
            .iter()
            .map(|rule| PackedRule {
                backtrack: &rule.backtrack,
                input: &rule.input,
                lookahead: &rule.lookahead,
                records: (0..rule.input.len() as u16)
                    .map(|i| SequenceLookupRecord::new(i, rule.pal))
                    .collect(),
            })
            .collect();
        lookups.push(chain_lookup(&packed, &class_glyphs, &glyphs_of_class));
        feature_lookups.push(next);
    }

    let lookup_list = LookupList::new(lookups);

    // ---- feature + scripts -----------------------------------------------
    let feature_list = FeatureList::new(vec![FeatureRecord::new(
        Tag::new(b"calt"),
        Feature::new(None, feature_lookups),
    )]);
    let lang_sys = LangSys::new(vec![0]);
    let script = Script::new(Some(lang_sys.clone()), Vec::new());
    let script_list = ScriptList::new(vec![
        ScriptRecord::new(Tag::new(b"DFLT"), script.clone()),
        ScriptRecord::new(Tag::new(b"latn"), script),
    ]);

    Some(Gsub::new(script_list, feature_list, lookup_list))
}

/// One chain-context rule flattened for subtable packing: class windows
/// plus the precomputed nested-lookup records.
struct PackedRule<'r> {
    backtrack: &'r [u16],
    input: &'r [u16],
    lookahead: &'r [u16],
    records: Vec<SequenceLookupRecord>,
}

/// Pack rules into chain-context format-2 subtables (one per ~36 KiB
/// chunk, grouped by first input class) and wrap them into one lookup —
/// a plain ChainContextual lookup when a single subtable suffices, an
/// Extension lookup otherwise.
fn chain_lookup(
    packed: &[PackedRule<'_>],
    class_glyphs: &[(u16, u16)],
    glyphs_of_class: &HashMap<u16, Vec<u16>>,
) -> SubstitutionLookup {
    // ---- chunk rules into subtable-sized groups ---------------------------
    // Grouped by first input class; classes themselves may be split. The
    // budget must subtract the format-2 subtable's fixed overhead: it
    // carries THREE ClassDef copies (backtrack/input/lookahead), and a
    // state class that covers most of the font (string bodies, value
    // carriers) makes each ~6 bytes/glyph — unchecked, a "36 KiB" chunk
    // serializes past 64 KiB and the rule-set offsets (u16) overflow.
    let classdef_bytes = {
        // contiguous same-class glyph runs merge into one range record
        let mut ranges = 1usize;
        for w in class_glyphs.windows(2) {
            if w[0].1 != w[1].1 || w[1].0 != w[0].0 + 1 {
                ranges += 1;
            }
        }
        4 + 6 * ranges.max(1)
    };
    let budget = MAX_SUBTABLE_BYTES
        .saturating_sub(3 * classdef_bytes)
        .max(2048);
    let mut rules_by_class: HashMap<u16, Vec<&PackedRule<'_>>> = HashMap::new();
    for rule in packed {
        rules_by_class.entry(rule.input[0]).or_default().push(rule);
    }
    let mut classes: Vec<u16> = rules_by_class.keys().copied().collect();
    classes.sort_unstable();

    let mut chunks: Vec<Vec<(u16, Vec<&PackedRule<'_>>)>> = Vec::new();
    let mut current: Vec<(u16, Vec<&PackedRule<'_>>)> = Vec::new();
    let mut current_bytes = 0usize;
    for &class in &classes {
        let rules = &rules_by_class[&class];
        for rule_group in split_class_rules(rules, budget) {
            let group_bytes: usize = rule_group.iter().map(|r| est_rule_bytes(r)).sum();
            if !current.is_empty() && current_bytes + group_bytes > budget {
                chunks.push(std::mem::take(&mut current));
                current_bytes = 0;
            }
            current_bytes += group_bytes + 2 + 2 * rule_group.len();
            current.push((class, rule_group));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    // ---- one chain-context subtable per chunk -----------------------------
    let mut chain_contexts: Vec<ChainedSequenceContext> = Vec::with_capacity(chunks.len());
    for chunk in &chunks {
        let max_class = chunk.iter().map(|(class, _)| *class).max().unwrap_or(0);
        let mut sets: Vec<Option<ChainedClassSequenceRuleSet>> = vec![None; max_class as usize + 1];
        let mut coverage = CoverageTableBuilder::default();
        for &(class, ref rules) in chunk {
            let seq_rules: Vec<ChainedClassSequenceRule> = rules
                .iter()
                .map(|r| to_chain_rule(r.backtrack, r.input, r.lookahead, &r.records))
                .collect();
            sets[class as usize] = Some(ChainedClassSequenceRuleSet::new(seq_rules));
            if let Some(gids) = glyphs_of_class.get(&class) {
                for &gid in gids {
                    coverage.add(GlyphId16::new(gid));
                }
            }
        }
        let ranges: Vec<ClassRangeRecord> = class_glyphs
            .iter()
            .map(|&(gid, class)| {
                ClassRangeRecord::new(GlyphId16::new(gid), GlyphId16::new(gid), class)
            })
            .collect();
        let class_def = ClassDef::Format2(ClassDefFormat2::new(ranges));
        chain_contexts.push(ChainedSequenceContext::Format2(
            ChainedSequenceContextFormat2::new(
                coverage.build(),
                class_def.clone(),
                class_def.clone(),
                class_def,
                sets,
            ),
        ));
    }

    // ---- wrap into one lookup ---------------------------------------------
    if chain_contexts.len() == 1 {
        // small case: a plain (non-extension) chain lookup
        let chain = SubstitutionChainContext::from(chain_contexts.pop().expect("one chunk"));
        SubstitutionLookup::ChainContextual(Lookup::new(LookupFlag::empty(), vec![chain]))
    } else {
        let extensions: Vec<ExtensionSubtable> = chain_contexts
            .into_iter()
            .map(|ctx| {
                ExtensionSubtable::ChainContextual(ExtensionSubstFormat1::new(
                    6,
                    SubstitutionChainContext::from(ctx),
                ))
            })
            .collect();
        SubstitutionLookup::Extension(Lookup::new(LookupFlag::empty(), extensions))
    }
}

/// Split one class's rules into groups that each fit the subtable budget.
fn split_class_rules<'r>(
    rules: &[&'r PackedRule<'r>],
    budget: usize,
) -> Vec<Vec<&'r PackedRule<'r>>> {
    let mut groups: Vec<Vec<&'r PackedRule<'r>>> = Vec::new();
    let mut current: Vec<&'r PackedRule<'r>> = Vec::new();
    let mut bytes = 0usize;
    for &rule in rules {
        let b = est_rule_bytes(rule);
        if !current.is_empty() && bytes + b > budget {
            groups.push(std::mem::take(&mut current));
            bytes = 0;
        }
        bytes += b;
        current.push(rule);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn est_rule_bytes(rule: &PackedRule<'_>) -> usize {
    // backtrack u16s + input-1 u16s + lookahead u16s + one 4-byte lookup
    // record per nested application + counts
    8 + 2 * (rule.backtrack.len() + rule.lookahead.len() + rule.input.len())
        + 4 * rule.records.len()
}

fn to_chain_rule(
    backtrack: &[u16],
    input: &[u16],
    lookahead: &[u16],
    records: &[SequenceLookupRecord],
) -> ChainedClassSequenceRule {
    // input_sequence begins at the *second* input position; the first glyph's
    // class is the rule-set index.
    ChainedClassSequenceRule::new(
        backtrack.to_vec(),
        input[1..].to_vec(),
        lookahead.to_vec(),
        records.to_vec(),
    )
}
