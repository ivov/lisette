use rustc_hash::FxHashSet as HashSet;

use syntax::ast::Literal;

use super::types::*;

pub enum ScrutineeSignature {
    Constructors {
        type_name: TypeName,
        union: Union,
        seen_tags: HashSet<TagId>,
    },
    WildcardsOrLiterals,
}

pub fn specialize_by_constructor(rows: &[Row], tag_id: &str, arity: usize) -> Vec<Row> {
    rows.iter()
        .filter_map(|row| {
            let first = row.first()?;
            match first {
                NormalizedPattern::Constructor { tag, args, .. } if tag == tag_id => {
                    let mut new_row = args.clone();
                    new_row.extend_from_slice(&row[1..]);
                    Some(new_row)
                }
                NormalizedPattern::Wildcard => {
                    let mut new_row = vec![NormalizedPattern::Wildcard; arity];
                    new_row.extend_from_slice(&row[1..]);
                    Some(new_row)
                }
                _ => None,
            }
        })
        .collect()
}

pub fn specialize_by_wildcard(rows: &[Row]) -> Vec<Row> {
    rows.iter()
        .filter_map(|row| {
            let first = row.first()?;
            if matches!(first, NormalizedPattern::Wildcard) {
                Some(row[1..].to_vec())
            } else {
                None
            }
        })
        .collect()
}

pub fn specialize_by_literal(rows: &[Row], literal: &Literal) -> Vec<Row> {
    rows.iter()
        .filter_map(|row| {
            let first = row.first()?;
            match first {
                NormalizedPattern::Literal(lit) if lit == literal => Some(row[1..].to_vec()),
                NormalizedPattern::Wildcard => Some(row[1..].to_vec()),
                _ => None,
            }
        })
        .collect()
}

pub fn get_scrutinee_signature(rows: &[Row], unions: &UnionTable) -> Option<ScrutineeSignature> {
    if rows.is_empty() || rows[0].is_empty() {
        return None;
    }

    let mut type_name: Option<TypeName> = None;
    let mut union: Option<Union> = None;
    let mut seen_tags = HashSet::default();

    for row in rows {
        if let Some(NormalizedPattern::Constructor {
            type_name: tn, tag, ..
        }) = row.first()
        {
            if type_name.is_none() {
                type_name = Some(tn.clone());
                union = unions.get(tn).cloned();
            }
            seen_tags.insert(tag.clone());
        }
    }

    match (type_name, union) {
        (Some(type_name), Some(union)) => Some(ScrutineeSignature::Constructors {
            type_name,
            union,
            seen_tags,
        }),
        _ => Some(ScrutineeSignature::WildcardsOrLiterals),
    }
}

pub fn is_complete(rows: &[Row], unions: &UnionTable) -> bool {
    match get_scrutinee_signature(rows, unions) {
        Some(ScrutineeSignature::Constructors {
            union, seen_tags, ..
        }) => seen_tags.len() == union.len(),
        _ => false,
    }
}
