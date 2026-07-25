mod generics;
mod unused_expressions;

use diagnostics::LisetteDiagnostic;
use rayon::prelude::*;

use semantics::context::AnalysisContext;

use super::{PARALLEL_THRESHOLD, source_file_work};

pub(crate) fn run_all(analysis: &AnalysisContext) -> Vec<LisetteDiagnostic> {
    let store = analysis.store;

    let work = source_file_work(store);

    if work.len() < PARALLEL_THRESHOLD {
        let mut local = Vec::new();
        for (module, file) in &work {
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
        }
        return local;
    }

    let locals: Vec<Vec<LisetteDiagnostic>> = work
        .par_iter()
        .map(|(module, file)| {
            let mut local = Vec::new();
            generics::run(&file.items, &mut local);
            unused_expressions::run(&file.items, &module.id, store, &mut local);
            local
        })
        .collect();

    locals.into_iter().flatten().collect()
}
