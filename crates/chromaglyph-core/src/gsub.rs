//! GlyphPlan → a write-fonts `Gsub` table.
//!
//! Layout of the generated GSUB:
//! - Lookup 0..P-1 (P = palette size): `SingleSubst` format 2, one per
//!   palette color, mapping every colored base glyph to its colored copy.
//!   These are *nested-only* lookups: they are not referenced by any feature,
//!   so they never fire outside a contextual rule.
//! - Lookup P: every contextual rule, grouped into as many chain-context
//!   format-2 subtables as needed. Class defs put a base glyph and all of its
//!   colored copies into the same class, which makes backtrack/lookahead
//!   matching insensitive to whether neighboring glyphs have already been
//!   substituted. When the rules do not fit one subtable (u16 offsets cap a
//!   subtable at 64 KiB), the lookup becomes an extension lookup (type 7)
//!   whose 32-bit offsets allow a GSUB of any practical size.
//! - Feature `calt` (applied by default by HarfBuzz and browsers), registered
//!   under `DFLT` and `latn` default langsys.

use std::collections::HashMap;

use write_fonts::tables::gsub::{
    ExtensionSubstFormat1, ExtensionSubtable, Gsub, SingleSubst, SubstitutionChainContext,
    SubstitutionLookup,
};
use write_fonts::tables::layout::builders::CoverageTableBuilder;
use write_fonts::tables::layout::{
    ChainedClassSequenceRule, ChainedClassSequenceRuleSet, ChainedSequenceContext,
    ChainedSequenceContextFormat2, ClassDef, ClassDefFormat2, ClassRangeRecord, Feature,
    FeatureList, FeatureRecord, LangSys, Lookup, LookupFlag, LookupList, Script, ScriptList,
    ScriptRecord, SequenceLookupRecord,
};
use write_fonts::types::{GlyphId16, Tag};

use crate::compile::{GlyphPlan, Rule};

/// Conservative per-subtable budget: subtable-internal offsets are u16.
const MAX_SUBTABLE_BYTES: usize = 36_000;

/// Build the GSUB table; `None` when the plan colors nothing (font keeps no
/// GSUB and no color tables).
pub fn build_gsub(plan: &GlyphPlan) -> Option<Gsub> {
    if plan.rules.is_empty() || plan.alt_glyphs.is_empty() {
        return None;
    }
    let num_palettes = plan.palette.len() as u16;

    // ---- nested single-substitution lookups, one per palette entry --------
    let mut pairs_by_pal: HashMap<u16, Vec<(u16, u16)>> = HashMap::new();
    for &(base, pal, alt) in &plan.alt_glyphs {
        pairs_by_pal.entry(pal).or_default().push((base, alt));
    }
    let mut singles: Vec<SubstitutionLookup> = Vec::new();
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
        singles.push(SubstitutionLookup::Single(Lookup::new(
            LookupFlag::empty(),
            vec![single],
        )));
    }
    let chain_lookup_index = num_palettes;

    // ---- shared class material -------------------------------------------
    // One class per base glyph; alt copies share their base's class.
    let mut class_glyphs: Vec<(u16, u16)> = plan
        .class_of
        .iter()
        .map(|(&gid, &class)| (gid, class))
        .collect();
    class_glyphs.sort_unstable();
    let base_gid_of_class: HashMap<u16, u16> = class_glyphs
        .iter()
        .filter(|&(gid, _)| *gid < plan.num_source_glyphs)
        .map(|&(gid, class)| (class, gid))
        .collect();

    // ---- chunk rules into subtable-sized groups ---------------------------
    // Grouped by first input class; classes themselves may be split.
    let mut rules_by_class: HashMap<u16, Vec<&Rule>> = HashMap::new();
    for rule in &plan.rules {
        rules_by_class.entry(rule.input[0]).or_default().push(rule);
    }
    let mut classes: Vec<u16> = rules_by_class.keys().copied().collect();
    classes.sort_unstable();

    let mut chunks: Vec<Vec<(u16, Vec<&Rule>)>> = Vec::new();
    let mut current: Vec<(u16, Vec<&Rule>)> = Vec::new();
    let mut current_bytes = 0usize;
    for class in classes {
        let rules = &rules_by_class[&class];
        for rule_group in split_class_rules(rules) {
            let group_bytes: usize = rule_group.iter().map(|r| est_rule_bytes(r)).sum();
            if !current.is_empty() && current_bytes + group_bytes > MAX_SUBTABLE_BYTES {
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
                .map(|r| to_chain_rule(r, chain_lookup_index))
                .collect();
            sets[class as usize] = Some(ChainedClassSequenceRuleSet::new(seq_rules));
            if let Some(&gid) = base_gid_of_class.get(&class) {
                coverage.add(GlyphId16::new(gid));
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

    // ---- assemble the chain lookup ----------------------------------------
    let mut lookups = singles;
    if chain_contexts.len() == 1 {
        // small case: a plain (non-extension) chain lookup
        let chain = SubstitutionChainContext::from(chain_contexts.pop().expect("one chunk"));
        lookups.push(SubstitutionLookup::ChainContextual(Lookup::new(
            LookupFlag::empty(),
            vec![chain],
        )));
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
        lookups.push(SubstitutionLookup::Extension(Lookup::new(
            LookupFlag::empty(),
            extensions,
        )));
    }
    let lookup_list = LookupList::new(lookups);

    // ---- feature + scripts -----------------------------------------------
    let feature_list = FeatureList::new(vec![FeatureRecord::new(
        Tag::new(b"calt"),
        Feature::new(None, vec![chain_lookup_index]),
    )]);
    let lang_sys = LangSys::new(vec![0]);
    let script = Script::new(Some(lang_sys.clone()), Vec::new());
    let script_list = ScriptList::new(vec![
        ScriptRecord::new(Tag::new(b"DFLT"), script.clone()),
        ScriptRecord::new(Tag::new(b"latn"), script),
    ]);

    Some(Gsub::new(script_list, feature_list, lookup_list))
}

/// Split one class's rules into groups that each fit the subtable budget.
fn split_class_rules<'r>(rules: &[&'r Rule]) -> Vec<Vec<&'r Rule>> {
    let mut groups: Vec<Vec<&'r Rule>> = Vec::new();
    let mut current: Vec<&'r Rule> = Vec::new();
    let mut bytes = 0usize;
    for &rule in rules {
        let b = est_rule_bytes(rule);
        if !current.is_empty() && bytes + b > MAX_SUBTABLE_BYTES {
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

fn est_rule_bytes(rule: &Rule) -> usize {
    // backtrack u16s + input-1 u16s + lookahead u16s + one 4-byte lookup
    // record per input glyph + counts
    8 + 2 * (rule.backtrack.len() + rule.lookahead.len() + rule.input.len()) + 4 * rule.input.len()
}

fn to_chain_rule(rule: &Rule, chain_lookup_index: u16) -> ChainedClassSequenceRule {
    debug_assert!(rule.pal < chain_lookup_index);
    let records: Vec<SequenceLookupRecord> = (0..rule.input.len() as u16)
        .map(|i| SequenceLookupRecord::new(i, rule.pal))
        .collect();
    // input_sequence begins at the *second* input position; the first glyph's
    // class is the rule-set index.
    ChainedClassSequenceRule::new(
        rule.backtrack.clone(),
        rule.input[1..].to_vec(),
        rule.lookahead.clone(),
        records,
    )
}
