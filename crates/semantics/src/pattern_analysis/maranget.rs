use rustc_hash::FxHashSet as HashSet;

use super::NormalizedPattern::{Literal, Wildcard};
use super::pattern_matrix::{
    ScrutineeSignature, get_scrutinee_signature, is_complete, specialize_by_constructor,
    specialize_by_literal, specialize_by_wildcard,
};
use super::types::*;

enum AlgorithmCase {
    NoColumnsLeft {
        has_rows: bool,
    },
    NoRows {
        column_count: usize,
    },
    OnlyWildcardsOrLiterals,
    IncompleteConstructors {
        type_name: TypeName,
        union: Union,
        seen_tags: HashSet<TagId>,
    },
    AllConstructors {
        type_name: TypeName,
        union: Union,
    },
}

pub fn check_exhaustiveness(
    rows: &[Row],
    unions: &UnionTable,
) -> Result<(), Vec<NormalizedPattern>> {
    let witnesses = find_witnesses(rows, unions);

    if witnesses.is_empty() {
        return Ok(());
    }

    let heads: Vec<NormalizedPattern> = witnesses
        .into_iter()
        .map(|w| w.into_iter().next().unwrap_or(Wildcard))
        .collect();

    Err(heads)
}

fn column_count(rows: &[Row]) -> usize {
    rows.first().map(|r| r.len()).unwrap_or(0)
}

fn classify_case(rows: &[Row], unions: &UnionTable) -> AlgorithmCase {
    let columns = column_count(rows);

    if columns == 0 {
        return AlgorithmCase::NoColumnsLeft {
            has_rows: !rows.is_empty(),
        };
    }

    if rows.is_empty() {
        return AlgorithmCase::NoRows {
            column_count: columns,
        };
    }

    match get_scrutinee_signature(rows, unions) {
        None | Some(ScrutineeSignature::WildcardsOrLiterals) => {
            AlgorithmCase::OnlyWildcardsOrLiterals
        }
        Some(ScrutineeSignature::Constructors {
            type_name,
            union,
            seen_tags,
        }) => {
            if seen_tags.len() < union.len() {
                return AlgorithmCase::IncompleteConstructors {
                    type_name,
                    union,
                    seen_tags,
                };
            }

            AlgorithmCase::AllConstructors { type_name, union }
        }
    }
}

fn find_witnesses(rows: &[Row], unions: &UnionTable) -> Vec<Row> {
    match classify_case(rows, unions) {
        AlgorithmCase::NoColumnsLeft { has_rows } => {
            if has_rows {
                vec![]
            } else {
                vec![vec![]]
            }
        }

        AlgorithmCase::NoRows { column_count } => {
            vec![vec![Wildcard; column_count]]
        }

        AlgorithmCase::OnlyWildcardsOrLiterals => {
            let specialized = specialize_by_wildcard(rows);
            find_witnesses(&specialized, unions)
                .into_iter()
                .map(|mut witness| {
                    witness.insert(0, Wildcard);
                    witness
                })
                .collect()
        }

        AlgorithmCase::IncompleteConstructors {
            type_name,
            union,
            seen_tags,
        } => {
            let specialized = specialize_by_wildcard(rows);
            let rest = find_witnesses(&specialized, unions);

            if rest.is_empty() {
                return vec![];
            }

            let missing: Vec<_> = union
                .iter()
                .filter(|c| !seen_tags.contains(&c.tag_id))
                .map(|c| NormalizedPattern::Constructor {
                    type_name: type_name.clone(),
                    tag: c.tag_id.clone(),
                    args: vec![Wildcard; c.arity],
                })
                .collect();

            let mut result = Vec::new();
            for pattern in missing {
                for witness in &rest {
                    let mut new_witness = vec![pattern.clone()];
                    new_witness.extend(witness.iter().cloned());
                    result.push(new_witness);
                }
            }
            result
        }

        AlgorithmCase::AllConstructors { type_name, union } => union
            .iter()
            .flat_map(|Constructor { arity, tag_id, .. }| {
                let specialized = specialize_by_constructor(rows, tag_id, *arity);
                find_witnesses(&specialized, unions)
                    .into_iter()
                    .map(|witness| {
                        let (args, rest) = witness.split_at(witness.len().min(*arity));
                        let constructor = NormalizedPattern::Constructor {
                            type_name: type_name.clone(),
                            tag: tag_id.clone(),
                            args: args.to_vec(),
                        };
                        let mut result = vec![constructor];
                        result.extend(rest.iter().cloned());
                        result
                    })
                    .collect::<Vec<_>>()
            })
            .collect(),
    }
}

pub fn is_useful(rows: &[Row], pattern: &Row, unions: &UnionTable) -> bool {
    if pattern.is_empty() {
        return rows.is_empty();
    }

    if rows.is_empty() {
        return true;
    }

    match &pattern[0] {
        NormalizedPattern::Constructor {
            type_name,
            tag,
            args,
        } => {
            if let Some(union) = unions.get(type_name)
                && !union.iter().any(|c| c.tag_id == *tag)
            {
                return false;
            }

            let specialized_rows = specialize_by_constructor(rows, tag, args.len());
            let mut specialized_pattern = args.clone();
            specialized_pattern.extend_from_slice(&pattern[1..]);
            is_useful(&specialized_rows, &specialized_pattern, unions)
        }

        Wildcard => {
            if !is_complete(rows, unions) {
                let specialized_rows = specialize_by_wildcard(rows);
                let specialized_pattern = pattern[1..].to_vec();
                is_useful(&specialized_rows, &specialized_pattern, unions)
            } else {
                let Some(ScrutineeSignature::Constructors { union, .. }) =
                    get_scrutinee_signature(rows, unions)
                else {
                    return false;
                };

                for Constructor { arity, tag_id, .. } in &union {
                    let specialized_rows = specialize_by_constructor(rows, tag_id, *arity);
                    let mut specialized_pattern = vec![Wildcard; *arity];
                    specialized_pattern.extend_from_slice(&pattern[1..]);

                    if is_useful(&specialized_rows, &specialized_pattern, unions) {
                        return true;
                    }
                }
                false
            }
        }

        Literal(literal) => {
            let specialized_rows = specialize_by_literal(rows, literal);
            let specialized_pattern = pattern[1..].to_vec();
            is_useful(&specialized_rows, &specialized_pattern, unions)
        }
    }
}
