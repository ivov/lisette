pub(crate) mod generics;
pub(crate) mod unused_expressions;

use rayon::prelude::*;
use syntax::program::{File, Module};

use crate::context::AnalysisContext;
use crate::facts::{Facts, LocalFacts};

use super::PARALLEL_THRESHOLD;

pub(crate) fn run_all(analysis: &AnalysisContext, facts: &mut Facts) {
    let store = analysis.store;

    let mut work: Vec<(&Module, &File)> = store
        .modules
        .values()
        .flat_map(|m| m.files.values().map(move |f| (m, f)))
        .collect();
    work.sort_unstable_by(|a, b| {
        a.0.id
            .cmp(&b.0.id)
            .then_with(|| a.1.name.cmp(&b.1.name))
            .then_with(|| a.1.id.cmp(&b.1.id))
    });

    if work.len() < PARALLEL_THRESHOLD {
        let mut local = LocalFacts::default();
        for (module, file) in &work {
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
        }
        facts.absorb_local_facts(local);
        return;
    }

    let locals: Vec<LocalFacts> = work
        .par_iter()
        .map(|(module, file)| {
            let mut local = LocalFacts::default();
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
            local
        })
        .collect();

    for local in locals {
        facts.absorb_local_facts(local);
    }
}
